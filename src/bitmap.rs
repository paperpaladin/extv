#![allow(dead_code)]
// The block and inode bitmaps, read the way the e2fsprogs library reads
// them, because that is what every oracle here prints.
//
// Two things make this more than a copy of some bytes. A group marked
// BLOCK_UNINIT or INODE_UNINIT has no meaningful bitmap on disk at all:
// the library synthesises one from where that group's metadata lies,
// and a reader that prints the bytes that happen to be there disagrees
// with dumpe2fs on every such group. And with bigalloc the block bitmap
// has one bit per cluster rather than per block, so every offset in it
// is divided by the cluster ratio.
use crate::err::{bad, Result};
use crate::group::{self, Group, BG_BLOCK_UNINIT, BG_INODE_UNINIT};
use crate::img::Img;
use crate::sb::Sb;

pub struct Bits {
    pub v: Vec<u8>,
    pub n: u64,
}

impl Bits {
    pub fn new(n: u64) -> Bits {
        Bits {
            v: vec![0u8; ((n + 7) / 8) as usize],
            n,
        }
    }

    pub fn get(&self, i: u64) -> bool {
        i < self.n && self.v[(i / 8) as usize] & (1 << (i % 8)) != 0
    }

    pub fn set(&mut self, i: u64) {
        if i < self.n {
            self.v[(i / 8) as usize] |= 1 << (i % 8);
        }
    }

    pub fn count_set(&self) -> u64 {
        (0..self.n).filter(|&i| self.get(i)).count() as u64
    }
}

// One group's bitmap, in a buffer of exactly as many bytes as the group
// has bits. Reading a whole block instead would hide a read that runs
// off the end of the bitmap into the block's padding, which on a 1024
// byte block with 8192 clusters per group is where the bitmap happens
// to end anyway.
pub fn read_one(img: &Img, block: u64, bits: u64) -> Result<Vec<u8>> {
    let bytes = ((bits + 7) / 8) as usize;
    if bytes > img.bsize as usize {
        return bad(format!(
            "a bitmap of {} bits does not fit in a {}-byte block",
            bits, img.bsize
        ));
    }
    let off = block
        .checked_mul(img.bsize as u64)
        .ok_or_else(|| crate::err::Error::Malformed(format!("block {} overflows", block)))?;
    img.read_at(off, bytes)
}

pub fn cluster_ratio(sb: &Sb) -> u64 {
    (sb.cluster_size / sb.block_size) as u64
}

pub fn clusters_count(sb: &Sb) -> u64 {
    let r = cluster_ratio(sb);
    (sb.blocks_count - sb.first_data_block as u64 + r - 1) / r
}

// The global block bitmap, in cluster units, indexed from
// first_data_block / ratio the way the library indexes it.
//
// The order matters and is the library's: every initialised group's
// bytes first, then the synthesised marks for the uninitialised ones.
// With flex_bg a group's inode table can lie inside a different group,
// so the marks are applied to the whole map rather than to one group's
// slice of it.
pub fn block_bitmap(img: &Img, sb: &Sb, gs: &[Group]) -> Result<Bits> {
    let ratio = cluster_ratio(sb);
    let first = sb.first_data_block as u64 / ratio;
    let mut bits = Bits::new(first + clusters_count(sb));
    for (g, gd) in gs.iter().enumerate() {
        if gd.flags & BG_BLOCK_UNINIT != 0 {
            continue;
        }
        let base = first + g as u64 * sb.clusters_per_group as u64;
        let raw = read_one(img, gd.block_bitmap, sb.clusters_per_group as u64)?;
        for i in 0..sb.clusters_per_group as u64 {
            if raw[(i / 8) as usize] & (1 << (i % 8)) != 0 {
                bits.set(base + i);
            }
        }
    }
    for (g, gd) in gs.iter().enumerate() {
        if gd.flags & BG_BLOCK_UNINIT == 0 {
            continue;
        }
        for b in group_metadata_blocks(sb, g as u64, gd) {
            bits.set(b / ratio);
        }
    }
    Ok(bits)
}

// The blocks a group's own metadata occupies: the superblock backup and
// the descriptor table where the group carries one, then the two
// bitmaps and the inode table, which with flex_bg may be somewhere
// else entirely. This is the set the library marks for a group whose
// bitmap was never written.
pub fn group_metadata_blocks(sb: &Sb, g: u64, gd: &Group) -> Vec<u64> {
    let mut out = group::super_and_gdt_blocks(sb, g);
    out.push(gd.block_bitmap);
    out.push(gd.inode_bitmap);
    for i in 0..inode_table_blocks(sb) {
        out.push(gd.inode_table + i);
    }
    out
}

pub fn inode_table_blocks(sb: &Sb) -> u64 {
    let per_block = sb.block_size as u64 / sb.inode_size as u64;
    (sb.inodes_per_group as u64 + per_block - 1) / per_block
}

// The global inode bitmap, one bit per inode, indexed from zero for
// inode 1.
pub fn inode_bitmap(img: &Img, sb: &Sb, gs: &[Group]) -> Result<Bits> {
    let mut bits = Bits::new(sb.inodes_count as u64);
    for (g, gd) in gs.iter().enumerate() {
        if gd.flags & BG_INODE_UNINIT != 0 {
            continue;
        }
        let base = g as u64 * sb.inodes_per_group as u64;
        let raw = read_one(img, gd.inode_bitmap, sb.inodes_per_group as u64)?;
        for i in 0..sb.inodes_per_group as u64 {
            if raw[(i / 8) as usize] & (1 << (i % 8)) != 0 {
                bits.set(base + i);
            }
        }
    }
    Ok(bits)
}

// Runs of clear bits, as a pair of bitmap indices per run. The caller
// turns them into the block numbers dumpe2fs prints.
pub fn free_runs(bits: &Bits, from: u64, count: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut i = 0u64;
    while i < count {
        if bits.get(from + i) {
            i += 1;
            continue;
        }
        let start = i;
        while i < count && !bits.get(from + i) {
            i += 1;
        }
        out.push((from + start, from + i - 1));
    }
    out
}
