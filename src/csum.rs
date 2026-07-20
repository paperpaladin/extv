#![allow(dead_code)]
use crate::crc::{crc16, crc32c};
use crate::err::Result;
use crate::group;
use crate::img::Img;
use crate::sb::{Sb, RO_GDT_CSUM, RO_METADATA_CSUM};

// The seed every metadata_csum checksum starts from. With
// metadata_csum_seed the superblock carries it directly, so the UUID can
// be changed on a mounted filesystem without rewriting every checksum;
// otherwise it is the crc32c of the UUID.
pub fn seed(sb: &Sb) -> u32 {
    if sb.has_incompat(crate::sb::INCOMPAT_CSUM_SEED) {
        sb.checksum_seed
    } else {
        crc32c(!0u32, &sb.uuid)
    }
}

// crc32c of the superblock up to but not including s_checksum. Note the
// seed is ~0 and not the filesystem seed, and the result is not
// inverted: e2fsprogs stores the running value.
pub fn superblock(sb: &Sb) -> u32 {
    crc32c(!0u32, &sb.raw[..1020])
}

// The group descriptor checksum, in both of its forms. metadata_csum
// takes the crc32c of the whole descriptor with the checksum field
// zeroed, seeded with the filesystem seed and then the little-endian
// group number. The older uninit_bg form is a crc16 over the UUID, the
// group number, and the descriptor in two pieces with the checksum
// field skipped rather than zeroed, and it covers the 64-bit half only
// when the 64bit feature is on.
pub fn group_desc(sb: &Sb, g: u64, raw: &[u8]) -> u16 {
    let le_group = (g as u32).to_le_bytes();
    if sb.has_ro(RO_METADATA_CSUM) {
        let mut d = raw.to_vec();
        d[30] = 0;
        d[31] = 0;
        let c = crc32c(seed(sb), &le_group);
        (crc32c(c, &d) & 0xffff) as u16
    } else {
        let mut c = crc16(!0u16, &sb.uuid);
        c = crc16(c, &le_group);
        c = crc16(c, &raw[..30]);
        if sb.is_64bit() && raw.len() > 32 {
            c = crc16(c, &raw[32..]);
        }
        c
    }
}

// The inode checksum: crc32c over the whole inode with both halves of
// the checksum field zeroed, seeded with the filesystem seed, then the
// little-endian inode number, then the generation. The inode number is
// in the seed rather than the data, which is what stops a copied inode
// from validating in another slot.
pub fn inode(sb: &Sb, num: u32, raw: &[u8], extra_isize: u16) -> u32 {
    let mut d = raw.to_vec();
    d[124] = 0;
    d[125] = 0;
    let has_hi = sb.inode_size > 128 && extra_isize >= 4;
    if has_hi {
        d[130] = 0;
        d[131] = 0;
    }
    let gen = u32::from_le_bytes([raw[100], raw[101], raw[102], raw[103]]);
    let mut c = crc32c(seed(sb), &num.to_le_bytes());
    c = crc32c(c, &gen.to_le_bytes());
    c = crc32c(c, &d);
    if has_hi {
        c
    } else {
        c & 0xffff
    }
}

pub fn group_desc_applies(sb: &Sb) -> bool {
    sb.has_ro(RO_METADATA_CSUM) || sb.has_ro(RO_GDT_CSUM)
}

// A bitmap checksum. The block bitmap's covers clusters_per_group / 8
// bytes and the inode bitmap's covers (inodes_per_group + 7) / 8, which
// is not the whole block in either case, and getting the length wrong
// is the easy mistake because for most of the corpus the two happen to
// be equal.
//
// Only the low sixteen bits are stored in a 32-byte descriptor; a
// 64-byte one keeps the high half as well, so the comparison has to be
// masked to what the filesystem can actually hold.
pub fn bitmap(sb: &Sb, bits: &[u8]) -> u32 {
    crc32c(seed(sb), bits)
}

pub fn bitmap_csum_width(sb: &Sb) -> u32 {
    if sb.is_64bit() && sb.desc_size >= 64 {
        0xffff_ffff
    } else {
        0xffff
    }
}

pub fn read_group_csum(img: &Img, sb: &Sb, g: u64) -> Result<(u16, u16)> {
    let raw = group::read_group_raw(img, sb, g)?;
    let stored = u16::from_le_bytes([raw[30], raw[31]]);
    Ok((stored, group_desc(sb, g, &raw)))
}

// A directory block's checksum and an extent tree block's checksum are
// built the same way: the filesystem seed, then the owning inode's
// number and generation, then the bytes. The inode number is in the
// seed rather than in the data, which is what stops a block copied
// from one directory into another from validating.
fn inode_seeded(sb: &Sb, inum: u32, generation: u32) -> u32 {
    let c = crc32c(seed(sb), &inum.to_le_bytes());
    crc32c(c, &generation.to_le_bytes())
}

// The tail of a directory block pretends to be an entry with inode 0
// and occupies the last twelve bytes, and the checksum covers
// everything before it.
pub fn dirent_block(sb: &Sb, inum: u32, generation: u32, block: &[u8]) -> u32 {
    let c = inode_seeded(sb, inum, generation);
    crc32c(c, &block[..block.len() - 12])
}

// An extent tree block's tail sits after the space for eh_max entries,
// whether or not that many are in use, so the length comes from the
// maximum and not from the count. Using the count instead agrees on a
// full node and disagrees on every other one.
pub fn extent_block(sb: &Sb, inum: u32, generation: u32, block: &[u8], max: u16) -> u32 {
    let n = 12 + 12 * max as usize;
    let c = inode_seeded(sb, inum, generation);
    crc32c(c, &block[..n])
}
