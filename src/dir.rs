#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::inode::Inode;
use crate::map;
use crate::sb::{Sb, INCOMPAT_FILETYPE};

// The checksum tail of a directory block pretends to be an entry with
// inode 0 and this file type, which is how it stays invisible to a
// reader that does not know about it.
pub const DIRENT_TAIL_TYPE: u8 = 0xDE;

pub struct DirEntry {
    pub inode: u32,
    pub rec_len: u32,
    pub name_len: u16,
    pub file_type: u8,
    pub name: Vec<u8>,
    pub logical: u64,
    pub physical: u64,
    pub offset: usize,
}

pub struct DirRead {
    pub entries: Vec<DirEntry>,
    pub blocks: u64,
    pub tails: u64,
    pub empty_slots: u64,
    // How many entries were a whole 65536-byte block long, which is
    // the only rec_len the sixteen-bit field cannot hold. The suite
    // asserts this is not zero over the corpus: an image with a
    // 65536-byte block whose directories all happen to fit their
    // entries inside one proves nothing about the encoding.
    pub wide_rec_len: u64,
    pub inline: bool,
}

pub fn file_type_name(t: u8) -> &'static str {
    match t {
        1 => "regular",
        2 => "directory",
        3 => "character special",
        4 => "block special",
        5 => "FIFO",
        6 => "socket",
        7 => "symlink",
        _ => "unknown",
    }
}

// A rec_len of a whole 65536-byte block does not fit in the sixteen
// bits the field has. ext4 spends the two low bits on it, which are
// free because every rec_len is a multiple of four: a stored 0xffff
// (or 0) means one whole block, and otherwise bits 0 and 1 are bits 16
// and 17 of the length. The encoding is live only when the block size
// is 65536, and no other block size can reach it, so every other image
// takes the field as it stands.
//
// mke2fs warns that a 65536-byte block is too big for a machine with
// 4096-byte pages and builds the image anyway, which is how the corpus
// gets one. Without the decode extv refuses a valid image, saying the
// rec_len 65535 is not a multiple of four.
pub fn rec_len_from_disk(raw: u16, block_size: u32) -> u32 {
    if block_size < 65536 {
        raw as u32
    } else if raw == 0xffff || raw == 0 {
        block_size
    } else {
        ((raw & 0xfffc) as u32) | (((raw & 3) as u32) << 16)
    }
}

// One directory block. The three ways this walk can be made to misbehave
// by a damaged image are a rec_len of zero, which never advances; a
// rec_len that reaches past the end of the block, which reads the next
// one; and a name_len longer than the entry that holds it. All three are
// refusals.
fn parse_block(
    sb: &Sb,
    buf: &[u8],
    logical: u64,
    physical: u64,
    start: usize,
    out: &mut DirRead,
) -> Result<()> {
    let v = V::new(buf, "directory block");
    // The limit is the buffer, not the block size: an inline directory
    // lives in a buffer of sixty bytes plus whatever the system.data
    // attribute adds, and is parsed by this same loop.
    let bsize = buf.len();
    let filetype = sb.has_incompat(INCOMPAT_FILETYPE);
    let mut off = start;
    while off + 8 <= bsize {
        let inode = v.u32(off)?;
        let raw_rec_len = v.u16(off + 4)?;
        let rec_len = rec_len_from_disk(raw_rec_len, sb.block_size);
        if rec_len as usize > u16::MAX as usize {
            out.wide_rec_len += 1;
        }
        let (name_len, file_type) = if filetype {
            (v.u8(off + 6)? as u16, v.u8(off + 7)?)
        } else {
            (v.u16(off + 6)?, 0u8)
        };
        if rec_len < 8 {
            return bad(format!(
                "directory entry at offset {} of block {} has rec_len {}, \
                 which cannot hold a header",
                off, physical, rec_len
            ));
        }
        if rec_len % 4 != 0 {
            return bad(format!(
                "directory entry at offset {} of block {} has rec_len {}, not a multiple of 4",
                off, physical, rec_len
            ));
        }
        if off + rec_len as usize > bsize {
            return bad(format!(
                "directory entry at offset {} of block {} has rec_len {}, \
                 which runs past the end of the block",
                off, physical, rec_len
            ));
        }
        if 8 + name_len as usize > rec_len as usize {
            return bad(format!(
                "directory entry at offset {} of block {} has a {}-byte name \
                 in a {}-byte entry",
                off, physical, name_len, rec_len
            ));
        }
        if inode == 0 {
            if file_type == DIRENT_TAIL_TYPE && name_len == 0 && rec_len == 12 {
                out.tails += 1;
            } else {
                out.empty_slots += 1;
            }
        } else {
            if inode as u64 > sb.inodes_count as u64 {
                return bad(format!(
                    "directory entry at offset {} of block {} names inode {}, \
                     past the last inode {}",
                    off, physical, inode, sb.inodes_count
                ));
            }
            out.entries.push(DirEntry {
                inode,
                rec_len,
                name_len,
                file_type,
                name: v.bytes(off + 8, name_len as usize)?.to_vec(),
                logical,
                physical,
                offset: off,
            });
        }
        #[cfg(feature = "plant_overread_dirent")]
        {
            println!("plant: dirent byte {} = {}", bsize, buf[bsize]);
        }
        off += rec_len as usize;
    }
    Ok(())
}

// The hash tree reader parses leaf blocks with the same loop.
pub fn parse_block_pub(
    sb: &Sb,
    buf: &[u8],
    logical: u64,
    physical: u64,
    out: &mut DirRead,
) -> Result<()> {
    parse_block(sb, buf, logical, physical, 0, out)
}

pub fn read(img: &Img, sb: &Sb, ino: &Inode) -> Result<DirRead> {
    let mut out = DirRead {
        entries: Vec::new(),
        blocks: 0,
        tails: 0,
        empty_slots: 0,
        wide_rec_len: 0,
        inline: false,
    };
    if !ino.is_dir() {
        return bad(format!("inode {} is not a directory", ino.num));
    }
    if ino.has_inline_data() {
        out.inline = true;
        read_inline(img, sb, ino, &mut out)?;
        return Ok(out);
    }
    for (logical, physical) in map::data_blocks(img, sb, ino)? {
        let buf = img.block(physical)?;
        out.blocks += 1;
        parse_block(sb, &buf, logical, physical, 0, &mut out)?;
    }
    Ok(out)
}

// An inline directory keeps its entries in the inode, in the sixty
// bytes that would otherwise hold block pointers, and spills the rest
// into the system.data extended attribute. The two pieces are one
// contiguous area and an entry may straddle the join, so they are
// concatenated before anything is parsed.
//
// The first four bytes are the parent's inode number, and the "." and
// ".." entries are not stored at all. debugfs synthesises them with a
// file type of 0, and so does this, because a directory listing that
// is missing its own two entries is not the same listing.
fn read_inline(img: &Img, sb: &Sb, ino: &Inode, out: &mut DirRead) -> Result<()> {
    // The size bound lives in xattr::inline_data, which both inline
    // readers go through: a size larger than the inline area is a
    // corrupt inode and not a directory to be parsed.
    let buf = crate::xattr::inline_data(img, sb, ino)?;
    if buf.len() < 4 {
        return bad("inline directory is shorter than its parent pointer");
    }
    let parent = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    out.entries.push(DirEntry {
        inode: ino.num,
        rec_len: 0,
        name_len: 1,
        file_type: 0,
        name: b".".to_vec(),
        logical: 0,
        physical: 0,
        offset: 0,
    });
    out.entries.push(DirEntry {
        inode: parent,
        rec_len: 0,
        name_len: 2,
        file_type: 0,
        name: b"..".to_vec(),
        logical: 0,
        physical: 0,
        offset: 0,
    });
    parse_block(sb, &buf, 0, 0, 4, out)
}

// Resolve a path to an inode number, one component at a time from the
// root. Symbolic links are not followed: debugfs does not follow them
// in the middle of a path either, and a reader that did could be sent
// round a loop by an image it is supposed to be inspecting.
pub fn resolve(img: &Img, sb: &Sb, path: &str) -> Result<u32> {
    let mut cur = 2u32;
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        let ino = crate::inode::read(img, sb, cur)?;
        if !ino.is_dir() {
            return bad(format!("{}: not a directory", part));
        }
        let d = read(img, sb, &ino)?;
        match d.entries.iter().find(|e| e.name == part.as_bytes()) {
            Some(e) => cur = e.inode,
            None => {
                return Err(crate::err::Error::Io(format!(
                    "{}: no such file or directory in the image",
                    path
                )))
            }
        }
    }
    Ok(cur)
}

// The name as debugfs prints it: printable ASCII as itself, everything
// else as a hex escape. Directory names are bytes, not text, and the
// corpus has one whose bytes are not ASCII for that reason.
pub fn escape(name: &[u8]) -> String {
    let mut s = String::new();
    for &b in name {
        if (0x20..0x7f).contains(&b) && b != b'\\' {
            s.push(b as char);
        } else {
            s.push_str(&format!("\\x{:02x}", b));
        }
    }
    s
}
