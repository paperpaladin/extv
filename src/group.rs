#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::sb::{Sb, INCOMPAT_META_BG, RO_SPARSE_SUPER};

pub const BG_INODE_UNINIT: u16 = 0x0001;
pub const BG_BLOCK_UNINIT: u16 = 0x0002;
pub const BG_INODE_ZEROED: u16 = 0x0004;

pub struct Group {
    pub block_bitmap: u64,
    pub inode_bitmap: u64,
    pub inode_table: u64,
    pub free_blocks: u32,
    pub free_inodes: u32,
    pub used_dirs: u32,
    pub unused_inodes: u32,
    pub exclude_bitmap: u64,
    pub flags: u16,
    pub checksum: u16,
    pub block_bitmap_csum: u32,
    pub inode_bitmap_csum: u32,
}

impl Group {
    pub fn flag_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.flags & BG_INODE_UNINIT != 0 {
            v.push("INODE_UNINIT");
        }
        if self.flags & BG_BLOCK_UNINIT != 0 {
            v.push("BLOCK_UNINIT");
        }
        if self.flags & BG_INODE_ZEROED != 0 {
            v.push("ITABLE_ZEROED");
        }
        v
    }
}

// Where group g's descriptor lives. Without meta_bg the descriptors are
// one contiguous array in the blocks after the superblock. With it, each
// meta block group keeps its own descriptors in its first block, which
// is how a filesystem grows past the point where the whole array fits in
// one group. Getting this wrong reads a descriptor out of a data block
// and reports plausible nonsense, which is why the corpus has an image
// with two meta block groups rather than one.
pub fn desc_location(sb: &Sb, g: u64) -> Result<(u64, usize)> {
    let per_block = (sb.block_size / sb.desc_size as u32) as u64;
    if per_block == 0 {
        return bad("block smaller than a group descriptor");
    }
    let metagroup = g / per_block;
    let block = if sb.has_incompat(INCOMPAT_META_BG) && metagroup >= sb.first_meta_bg as u64 {
        let first = metagroup * per_block;
        group_first_block(sb, first) + if has_super(sb, first) { 1 } else { 0 }
    } else {
        sb.first_data_block as u64 + 1 + metagroup
    };
    let off = (g % per_block) as usize * sb.desc_size as usize;
    Ok((block, off))
}

pub fn group_first_block(sb: &Sb, g: u64) -> u64 {
    sb.first_data_block as u64 + g * sb.blocks_per_group as u64
}

// Groups that carry a backup superblock. With sparse_super that is 0, 1
// and the powers of 3, 5 and 7; without it, all of them.
pub fn has_super(sb: &Sb, g: u64) -> bool {
    if !sb.has_ro(RO_SPARSE_SUPER) {
        return true;
    }
    if g <= 1 {
        return true;
    }
    if g & 1 == 0 {
        return false;
    }
    for base in [3u64, 5, 7] {
        let mut n = base;
        while n < g {
            n *= base;
        }
        if n == g {
            return true;
        }
    }
    false
}

pub fn read_group(img: &Img, sb: &Sb, g: u64) -> Result<Group> {
    if g >= sb.group_count {
        return bad(format!(
            "group {} is past the last group ({})",
            g,
            sb.group_count - 1
        ));
    }
    let (block, off) = desc_location(sb, g)?;
    let buf = img.block(block)?;
    let d = V::new(
        match buf.get(off..off + sb.desc_size as usize) {
            Some(s) => s,
            None => return bad(format!("group {} descriptor runs past block {}", g, block)),
        },
        "group descriptor",
    );
    let wide = sb.is_64bit() && sb.desc_size >= 64;
    Ok(Group {
        block_bitmap: d.u32_pair(0, 32, wide)?,
        inode_bitmap: d.u32_pair(4, 36, wide)?,
        inode_table: d.u32_pair(8, 40, wide)?,
        free_blocks: d.u16_pair(12, 44, wide)?,
        free_inodes: d.u16_pair(14, 46, wide)?,
        used_dirs: d.u16_pair(16, 48, wide)?,
        flags: d.u16(18)?,
        exclude_bitmap: d.u32_pair(20, 52, wide)?,
        block_bitmap_csum: d.u16_pair(24, 56, wide)?,
        inode_bitmap_csum: d.u16_pair(26, 58, wide)?,
        unused_inodes: d.u16_pair(28, 50, wide)?,
        checksum: d.u16(30)?,
    })
}

pub fn read_all(img: &Img, sb: &Sb) -> Result<Vec<Group>> {
    let mut v = Vec::with_capacity(sb.group_count as usize);
    for g in 0..sb.group_count {
        v.push(read_group(img, sb, g)?);
    }
    Ok(v)
}

// Blocks the whole descriptor table occupies in one group. Used by the
// consistency check, which has to know which blocks are metadata.
pub fn gdt_blocks(sb: &Sb) -> u64 {
    let per_block = (sb.block_size / sb.desc_size as u32) as u64;
    if sb.has_incompat(INCOMPAT_META_BG) {
        1
    } else {
        (sb.group_count + per_block - 1) / per_block
    }
}
