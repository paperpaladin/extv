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
                b.data(e.block as u64 + k, e.start + k)?;
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
        let ch = header(&cv, sb.block_size as usize)?;
        // The depth has to fall by exactly one at each step. Anything
        // else is either a loop or a lie, and both of them are how a
        // damaged image gets a reader to run for ever.
        if ch.depth + 1 != h.depth {
            return bad(format!(
                "extent node at block {} has depth {} under a node of depth {}",
                ix.leaf, ch.depth, h.depth
            ));
        }
        b.counts.depth = b.counts.depth.max(h.depth as u64);
        node(img, sb, b, &cv, &ch, level + 1)?;
    }
    Ok(())
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
