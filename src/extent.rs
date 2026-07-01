#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::inode::Inode;
use crate::map::{check_block, Builder};
use crate::sb::Sb;

pub const MAGIC: u16 = 0xF30A;
pub const HEADER_SIZE: usize = 12;
pub const ENTRY_SIZE: usize = 12;
pub const MAX_LEN_INIT: u16 = 32768;

pub struct Header {
    pub entries: u16,
    pub max: u16,
    pub depth: u16,
    pub generation: u32,
}

pub struct Extent {
    pub block: u32,
    pub len: u16,
    pub start: u64,
    pub uninit: bool,
}

pub struct Index {
    pub block: u32,
    pub leaf: u64,
}

// The header is the only place an extent tree can be rejected cheaply,
// so everything that can be checked here is: the magic, the entry count
// against the maximum, the maximum against the space the node actually
// has, and the depth against the five levels the format allows.
pub fn header(v: &V, space: usize) -> Result<Header> {
    let magic = v.u16(0)?;
    if magic != MAGIC {
        return bad(format!(
            "extent node magic is 0x{:04X}, not 0x{:04X}",
            magic, MAGIC
        ));
    }
    let entries = v.u16(2)?;
    let max = v.u16(4)?;
    let depth = v.u16(6)?;
    if depth > 5 {
        return bad(format!("extent node depth is {}, the format allows 5", depth));
    }
    let fits = (space - HEADER_SIZE) / ENTRY_SIZE;
    if max as usize > fits {
        return bad(format!(
            "extent node says it has room for {} entries, but {} bytes hold {}",
            max, space, fits
        ));
    }
    if entries > max {
        return bad(format!(
            "extent node has {} entries and room for {}",
            entries, max
        ));
    }
    Ok(Header {
        entries,
        max,
        depth,
        generation: v.u32(8)?,
    })
}

pub fn leaf(v: &V, i: usize) -> Result<Extent> {
    let o = HEADER_SIZE + i * ENTRY_SIZE;
    let raw_len = v.u16(o + 4)?;
    // The top bit of the length says the extent is allocated but has
    // never been written. A reader that ignores it reports a 32769-block
    // extent where there is a 1-block one.
    let (len, uninit) = if raw_len > MAX_LEN_INIT {
        (raw_len - MAX_LEN_INIT, true)
    } else {
        (raw_len, false)
    };
    Ok(Extent {
        block: v.u32(o)?,
        len,
        start: v.u32(o + 8)? as u64 | ((v.u16(o + 6)? as u64) << 32),
        uninit,
    })
}

pub fn index(v: &V, i: usize) -> Result<Index> {
    let o = HEADER_SIZE + i * ENTRY_SIZE;
    Ok(Index {
        block: v.u32(o)?,
        leaf: v.u32(o + 4)? as u64 | ((v.u16(o + 8)? as u64) << 32),
    })
}

pub fn walk(img: &Img, sb: &Sb, ino: &Inode, b: &mut Builder) -> Result<()> {
    let v = V::new(&ino.block, "inode extent header");
    let h = header(&v, 60)?;
    b.counts.depth = b.counts.depth.max(h.depth as u64);
    node(img, sb, b, &v, &h, 0)
}

fn node(img: &Img, sb: &Sb, b: &mut Builder, v: &V, h: &Header, level: u16) -> Result<()> {
    if h.depth == 0 {
        let mut last_end: Option<u64> = None;
        for i in 0..h.entries as usize {
            let e = leaf(v, i)?;
            // Entries are in increasing logical order and must not
            // overlap. A tree that breaks it is not a tree we can
            // report a file from.
            if let Some(end) = last_end {
                if (e.block as u64) < end {
                    return bad(format!(
                        "extent at logical block {} starts before the end of the one before it ({})",
                        e.block, end
                    ));
                }
            }
            last_end = Some(e.block as u64 + e.len as u64);
            if e.uninit {
                b.counts.uninit += 1;
            }
            if e.len == 0 {
                continue;
            }
            check_block(sb, e.start, "extent")?;
            check_block(sb, e.start + e.len as u64 - 1, "extent end")?;
            for k in 0..e.len as u64 {
                b.data(e.block as u64 + k, e.start + k, e.uninit)?;
            }
        }
        return Ok(());
    }
    for i in 0..h.entries as usize {
        let ix = index(v, i)?;
        check_block(sb, ix.leaf, "extent index")?;
        b.meta(etb_name(level), ix.leaf)?;
        let buf = img.block(ix.leaf)?;
        let cv = V::new(&buf, "extent node");
        let ch = child_header(&cv, sb, h, ix.leaf)?;
        b.counts.depth = b.counts.depth.max(h.depth as u64);
        node(img, sb, b, &cv, &ch, level + 1)?;
    }
    Ok(())
}

// The header of a child node, with the one rule that keeps the
// recursion finite: the depth has to fall by exactly one at each step.
// Anything else is either a loop or a lie, and both are how a damaged
// image gets a reader to run for ever.
//
// This lived in two places, the block walk and the row listing, and a
// mutation of one of them left the other refusing with the same
// message, so the mutant survived while the rule was gone from half
// the program. One rule, one function.
fn child_header(cv: &V, sb: &Sb, parent: &Header, leaf: u64) -> Result<Header> {
    let ch = header(cv, sb.block_size as usize)?;
    if ch.depth + 1 != parent.depth {
        return bad(format!(
            "extent node at block {} has depth {} under a node of depth {}",
            leaf, ch.depth, parent.depth
        ));
    }
    Ok(ch)
}

// debugfs names an extent tree block by the level it sits at: ETB0 for
// the first level below the inode.
fn etb_name(level: u16) -> &'static str {
    match level {
        0 => "ETB0",
        1 => "ETB1",
        2 => "ETB2",
        3 => "ETB3",
        _ => "ETB4",
    }
}

// The rows debugfs dump_extents prints, in the same order and with the
// same arithmetic. Two parts of it are not in the on-disk structure and
// have to be reconstructed:
//
// the logical range an index entry covers, which runs to the next
// sibling's first block minus one, or, for the last entry at a level,
// to wherever the parent's range ends;
//
// and the root's own end, which e2fsprogs takes from the inode size as
// ceil(size / block size) - 1. It is not the last block the tree
// actually maps, and on a file whose size is not a whole number of
// blocks the two differ.
pub struct Row {
    pub level: u16,
    pub max_depth: u16,
    pub index: u16,
    pub entries: u16,
    pub log_first: u64,
    pub log_last: u64,
    pub phys_first: u64,
    pub phys_last: u64,
    pub len: u64,
    pub uninit: bool,
    pub leaf: bool,
}

pub fn rows(img: &Img, sb: &Sb, ino: &Inode) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    let v = V::new(&ino.block, "inode extent header");
    let h = header(&v, 60)?;
    let end = if ino.size == 0 {
        0
    } else {
        (ino.size + sb.block_size as u64 - 1) / sb.block_size as u64 - 1
    };
    rows_node(img, sb, &v, &h, h.depth, 0, end, &mut out)?;
    Ok(out)
}

fn rows_node(
    img: &Img,
    sb: &Sb,
    v: &V,
    h: &Header,
    max_depth: u16,
    level: u16,
    end: u64,
    out: &mut Vec<Row>,
) -> Result<()> {
    for i in 0..h.entries as usize {
        if h.depth == 0 {
            let e = leaf(v, i)?;
            out.push(Row {
                level,
                max_depth,
                index: i as u16 + 1,
                entries: h.entries,
                log_first: e.block as u64,
                log_last: e.block as u64 + e.len.max(1) as u64 - 1,
                phys_first: e.start,
                phys_last: e.start + e.len.max(1) as u64 - 1,
                len: e.len as u64,
                uninit: e.uninit,
                leaf: true,
            });
        } else {
            let ix = index(v, i)?;
            let child_end = if i + 1 < h.entries as usize {
                index(v, i + 1)?.block as u64 - 1
            } else {
                end
            };
            out.push(Row {
                level,
                max_depth,
                index: i as u16 + 1,
                entries: h.entries,
                log_first: ix.block as u64,
                log_last: child_end,
                phys_first: ix.leaf,
                phys_last: ix.leaf,
                len: child_end + 1 - ix.block as u64,
                uninit: false,
                leaf: false,
            });
            check_block(sb, ix.leaf, "extent index")?;
            let buf = img.block(ix.leaf)?;
            let cv = V::new(&buf, "extent node");
            let ch = child_header(&cv, sb, h, ix.leaf)?;
            rows_node(img, sb, &cv, &ch, max_depth, level + 1, child_end, out)?;
        }
    }
    Ok(())
}
