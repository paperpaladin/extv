#![allow(dead_code)]
use crate::dir;
use crate::err::{bad, Result};
use crate::hash;
use crate::img::{Img, V};
use crate::inode::{Inode, FL_INDEX};
use crate::sb::{Sb, RO_METADATA_CSUM};

// A hash-indexed directory hides its index inside directory entries
// that look empty: the first block holds "." and "..", then a fake
// entry with inode 0 that covers the rest of the block and holds the
// index root, and every interior node is a whole block disguised the
// same way. That is why a linear reader works on an indexed directory
// without knowing any of this, and why reading the index has to be a
// separate pass.
pub struct Root {
    pub reserved_zero: u32,
    pub hash_version: u8,
    pub info_length: u8,
    pub indirect_levels: u8,
    pub flags: u8,
}

pub struct Node {
    pub count: u16,
    pub limit: u16,
    pub checksum: Option<u32>,
    pub entries: Vec<(u32, u32)>,
}

pub struct Leaf {
    pub logical: u64,
    pub physical: u64,
    pub names: Vec<(u32, Vec<u8>, u32)>,
}

pub enum Visit {
    Node(Node),
    Leaf(Leaf),
}

pub fn is_indexed(ino: &Inode) -> bool {
    ino.flags & FL_INDEX != 0
}

// The entry array of a node, which begins with a header occupying the
// space of one entry: a limit and a count, with the first entry's block
// following and its hash taken to be zero.
fn entries(sb: &Sb, buf: &[u8], at: usize, what: &'static str) -> Result<Node> {
    let v = V::new(buf, what);
    let limit = v.u16(at)?;
    let count = v.u16(at + 2)?;
    let room = (sb.block_size as usize - at) / 8;
    if limit as usize > room {
        return bad(format!(
            "{}: index says it has room for {} entries, the block holds {}",
            what, limit, room
        ));
    }
    if count > limit || count == 0 {
        return bad(format!(
            "{}: index has {} entries and room for {}",
            what, count, limit
        ));
    }
    let mut out = Vec::with_capacity(count as usize);
    out.push((0u32, v.u32(at + 4)?));
    for i in 1..count as usize {
        out.push((v.u32(at + i * 8)?, v.u32(at + i * 8 + 4)?));
    }
    // With metadata_csum the last slot is a tail rather than an entry,
    // which is why the limit is one less than the block would allow.
    let checksum = if sb.has_ro(RO_METADATA_CSUM) {
        Some(v.u32(at + limit as usize * 8 + 4)?)
    } else {
        None
    };
    Ok(Node {
        count,
        limit,
        checksum,
        entries: out,
    })
}

pub fn read(img: &Img, sb: &Sb, ino: &Inode) -> Result<(Root, Vec<Visit>)> {
    if !ino.is_dir() {
        return bad(format!("inode {} is not a directory", ino.num));
    }
    if !is_indexed(ino) {
        return bad(format!("inode {} is not hash indexed", ino.num));
    }
    let blocks = crate::map::data_blocks(img, sb, ino)?;
    let find = |logical: u64| -> Result<u64> {
        match blocks.iter().find(|(l, _)| *l == logical) {
            Some((_, p)) => Ok(*p),
            None => bad(format!(
                "hash tree points at logical block {}, which the directory does not have",
                logical
            )),
        }
    };
    let first = img.block(find(0)?)?;
    let v = V::new(&first, "hash tree root");
    let root = Root {
        reserved_zero: v.u32(24)?,
        hash_version: v.u8(28)?,
        info_length: v.u8(29)?,
        indirect_levels: v.u8(30)?,
        flags: v.u8(31)?,
    };
    if root.info_length != 8 {
        return bad(format!(
            "hash tree root says its info is {} bytes, not 8",
            root.info_length
        ));
    }
    if root.indirect_levels > 3 {
        return bad(format!(
            "hash tree root claims {} levels of indirection",
            root.indirect_levels
        ));
    }
    let mut out = Vec::new();
    let node = entries(sb, &first, 32, "hash tree root")?;
    let children: Vec<u32> = node.entries.iter().map(|(_, b)| *b).collect();
    out.push(Visit::Node(node));
    for b in children {
        descend(img, sb, ino, &find, b as u64, root.indirect_levels, &mut out)?;
    }
    Ok((root, out))
}

fn descend(
    img: &Img,
    sb: &Sb,
    ino: &Inode,
    find: &dyn Fn(u64) -> Result<u64>,
    logical: u64,
    levels: u8,
    out: &mut Vec<Visit>,
) -> Result<()> {
    let physical = find(logical)?;
    let buf = img.block(physical)?;
    if levels == 0 {
        let mut names = Vec::new();
        let mut d = dir::DirRead {
            entries: Vec::new(),
            blocks: 0,
            tails: 0,
            empty_slots: 0,
            wide_rec_len: 0,
            inline: false,
        };
        dir::parse_block_pub(sb, &buf, logical, physical, &mut d)?;
        for e in d.entries {
            names.push((e.inode, e.name, e.rec_len));
        }
        out.push(Visit::Leaf(Leaf {
            logical,
            physical,
            names,
        }));
        return Ok(());
    }
    let node = entries(sb, &buf, 8, "hash tree node")?;
    let children: Vec<u32> = node.entries.iter().map(|(_, b)| *b).collect();
    out.push(Visit::Node(node));
    for b in children {
        descend(img, sb, ino, find, b as u64, levels - 1, out)?;
    }
    Ok(())
}

// The hash of a name under this directory's own hash version and the
// filesystem's seed and signedness.
pub fn effective_version(sb: &Sb, root: &Root) -> u8 {
    let mut version = root.hash_version;
    // The superblock says whether the filesystem was written by a
    // machine whose char is signed. Only the three base versions have
    // an unsigned twin.
    if version <= hash::TEA && sb.flags & 0x0002 != 0 {
        version += 3;
    }
    version
}

pub fn name_hash(sb: &Sb, root: &Root, name: &[u8]) -> (u32, u32) {
    hash::dirhash(effective_version(sb, root), name, &sb.hash_seed)
}
