#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::sb::{Sb, RO_HUGE_FILE};

pub const S_IFMT: u16 = 0xF000;
pub const S_IFSOCK: u16 = 0xC000;
pub const S_IFLNK: u16 = 0xA000;
pub const S_IFREG: u16 = 0x8000;
pub const S_IFBLK: u16 = 0x6000;
pub const S_IFDIR: u16 = 0x4000;
pub const S_IFCHR: u16 = 0x2000;
pub const S_IFIFO: u16 = 0x1000;

pub const FL_EXTENTS: u32 = 0x0008_0000;
pub const FL_INLINE_DATA: u32 = 0x1000_0000;
pub const FL_INDEX: u32 = 0x0000_1000;
pub const FL_HUGE_FILE: u32 = 0x0004_0000;
pub const FL_EA_INODE: u32 = 0x0020_0000;

pub struct Inode {
    pub num: u32,
    pub raw: Vec<u8>,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub crtime: u32,
    pub atime_extra: u32,
    pub ctime_extra: u32,
    pub mtime_extra: u32,
    pub crtime_extra: u32,
    pub links_count: u16,
    pub blocks: u64,
    pub flags: u32,
    pub generation: u32,
    pub file_acl: u64,
    pub extra_isize: u16,
    pub checksum: u32,
    pub projid: u32,
    pub version: u64,
    pub block: [u8; 60],
}

impl Inode {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    pub fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
    pub fn is_device(&self) -> bool {
        matches!(self.mode & S_IFMT, S_IFBLK | S_IFCHR)
    }
    // A device, a FIFO or a socket owns no data at all. The sixty
    // bytes that hold block pointers in an ordinary inode hold the
    // device number for the first two and nothing for the others, so
    // walking them as a block map invents a block that debugfs does
    // not report and that the bitmap does not have marked.
    // With the huge_file feature and this flag, i_blocks is counted
    // in filesystem blocks rather than in 512-byte sectors. No image
    // here has one, so the sector check leaves those inodes alone
    // rather than guessing.
    pub fn counts_blocks_in_clusters(&self) -> bool {
        self.flags & 0x0004_0000 != 0
    }
    pub fn is_special(&self) -> bool {
        matches!(self.mode & S_IFMT, S_IFBLK | S_IFCHR | S_IFIFO | S_IFSOCK)
    }
    pub fn in_use(&self) -> bool {
        // An inode that was never allocated is all zeroes; a deleted one
        // keeps its mode but has a deletion time and no links.
        self.mode != 0 && self.links_count != 0
    }
    pub fn type_name(&self) -> &'static str {
        match self.mode & S_IFMT {
            S_IFSOCK => "socket",
            S_IFLNK => "symlink",
            S_IFREG => "regular",
            S_IFBLK => "block special",
            S_IFDIR => "directory",
            S_IFCHR => "character special",
            S_IFIFO => "FIFO",
            _ => "bad type",
        }
    }
    pub fn has_extents(&self) -> bool {
        self.flags & FL_EXTENTS != 0
    }
    pub fn has_inline_data(&self) -> bool {
        self.flags & FL_INLINE_DATA != 0
    }
    // A symlink short enough to fit keeps its target in the sixty bytes
    // that would otherwise hold block pointers, so walking them reads
    // the text as block numbers. The test is not the length: e2fsprogs
    // asks whether the inode owns any data blocks at all, counting out
    // the one an extended attribute block would use, because a long
    // target with a big xattr is still a slow link.
    pub fn is_fast_symlink(&self, block_size: u32) -> bool {
        if !self.is_symlink() || self.has_inline_data() {
            return false;
        }
        let acl_blocks = if self.file_acl != 0 {
            (block_size / 512) as u64
        } else {
            0
        };
        self.blocks.saturating_sub(acl_blocks) == 0
    }
}

// Byte offset of an inode in the image. The inode table of the group is
// found through the descriptor, and the inode's slot inside it is a
// plain multiplication, so the two ways this goes wrong are a group
// index off by one (inode numbers start at 1, not 0) and forgetting
// that inode_size is not 128 on anything modern.
pub fn inode_offset(img: &Img, sb: &Sb, num: u32) -> Result<u64> {
    if num == 0 || num as u64 > sb.inodes_count as u64 {
        return bad(format!(
            "inode {} is outside 1..{}",
            num, sb.inodes_count
        ));
    }
    let g = (num as u64 - 1) / sb.inodes_per_group as u64;
    let index = (num as u64 - 1) % sb.inodes_per_group as u64;
    let desc = crate::group::read_group(img, sb, g)?;
    let off = desc
        .inode_table
        .checked_mul(sb.block_size as u64)
        .and_then(|b| b.checked_add(index * sb.inode_size as u64))
        .ok_or_else(|| {
            crate::err::Error::Malformed(format!("inode {} offset overflows", num))
        })?;
    Ok(off)
}

pub fn read(img: &Img, sb: &Sb, num: u32) -> Result<Inode> {
    let off = inode_offset(img, sb, num)?;
    // Exactly inode_size bytes. Nothing in an inode may be read from
    // past its own slot, and a buffer sized to the block would hide a
    // read that walked into the next inode.
    let raw = img.read_at(off, sb.inode_size as usize)?;
    #[cfg(feature = "plant_overread_inode")]
    {
        println!(
            "plant: inode byte {} = {}",
            sb.inode_size,
            raw[sb.inode_size as usize]
        );
    }
    let v = V::new(&raw, "inode");
    let mode = v.u16(0)?;
    let flags = v.u32(32)?;
    let size_lo = v.u32(4)? as u64;
    let size_high = v.u32(108)? as u64;
    // In ext2 the high half of the size field meant something else for
    // directories (i_dir_acl), and large_file is what says it is a size.
    let size = if mode & S_IFMT == S_IFDIR {
        size_lo
    } else {
        size_lo | (size_high << 32)
    };
    let blocks_lo = v.u32(28)? as u64;
    let blocks_hi = v.u16(116)? as u64;
    let blocks = if sb.has_ro(RO_HUGE_FILE) {
        blocks_lo | (blocks_hi << 32)
    } else {
        blocks_lo
    };
    let extra_isize = if sb.inode_size > 128 { v.u16(128)? } else { 0 };
    // Every field past 128 bytes exists only if i_extra_isize says it
    // does. Reading crtime out of an inode that does not declare it
    // gives whatever the next structure left there.
    let has = |need: u16| extra_isize >= need && sb.inode_size as u16 >= 128 + need;
    let mut block = [0u8; 60];
    block.copy_from_slice(v.bytes(40, 60)?);
    Ok(Inode {
        num,
        mode,
        uid: v.u16(2)? as u32 | ((v.u16(120)? as u32) << 16),
        gid: v.u16(24)? as u32 | ((v.u16(122)? as u32) << 16),
        size,
        atime: v.u32(8)?,
        ctime: v.u32(12)?,
        mtime: v.u32(16)?,
        dtime: v.u32(20)?,
        links_count: v.u16(26)?,
        blocks,
        flags,
        generation: v.u32(100)?,
        file_acl: v.u32(104)? as u64 | ((v.u16(118)? as u64) << 32),
        extra_isize,
        // The low half lives at 124, inside the 128-byte base inode, so
        // it is there whatever the inode size: the first version here
        // returned 0 for a 128-byte inode and disagreed with debugfs on
        // every inode of two images. The high half is at 130 and exists
        // when i_extra_isize reaches 4, which e2fsprogs calls
        // EXT4_INODE_CSUM_HI_EXTRA_END.
        checksum: v.u16(124)? as u32
            | if has(4) { (v.u16(130)? as u32) << 16 } else { 0 },
        ctime_extra: if has(8) { v.u32(132)? } else { 0 },
        mtime_extra: if has(12) { v.u32(136)? } else { 0 },
        atime_extra: if has(16) { v.u32(140)? } else { 0 },
        crtime: if has(20) { v.u32(144)? } else { 0 },
        crtime_extra: if has(24) { v.u32(148)? } else { 0 },
        version: v.u32(36)? as u64 | if has(28) { (v.u32(152)? as u64) << 32 } else { 0 },
        projid: if has(32) { v.u32(156)? } else { 0 },
        raw,
        block,
    })
}

// A timestamp and its extra word together. The low two bits of the
// extra word extend the epoch past 2038 and the rest is nanoseconds.
pub fn time_seconds(base: u32, extra: u32) -> i64 {
    let epoch_bits = (extra & 3) as i64;
    if epoch_bits == 0 {
        base as i32 as i64
    } else {
        base as i64 + (epoch_bits << 32)
    }
}

pub fn time_nanos(extra: u32) -> u32 {
    extra >> 2
}
