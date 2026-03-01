#![allow(dead_code)]
// The journal superblock. It is the first block of the journal file,
// which for an internal journal is inode 8, and it is the one structure
// in an ext4 image that is big-endian: JBD2 came from a different
// lineage and kept its byte order.
//
// Nothing here replays anything. Reading the journal's state is a
// different job from recovering it, and recovery is a non-goal (see
// PLAN.md). What this does report is whether a replay is needed, which
// is a fact e2fsck also has an opinion about.
use crate::err::{bad, Result};
use crate::img::{Img, V};
use crate::inode;
use crate::map;
use crate::sb::{Sb, COMPAT_HAS_JOURNAL};

pub const MAGIC: u32 = 0xc03b_3998;

// Block types. The superblock is the only one read here; the rest are
// named so that a journal whose first block is a descriptor rather than
// a superblock produces a message saying what it actually found.
pub fn block_type_name(t: u32) -> &'static str {
    match t {
        1 => "descriptor",
        2 => "commit",
        3 => "superblock v1",
        4 => "superblock v2",
        5 => "revoke",
        _ => "unknown",
    }
}

pub const COMPAT_FLAGS: &[(u32, &str)] = &[(0x0001, "journal_checksum")];
pub const INCOMPAT_FLAGS: &[(u32, &str)] = &[
    (0x0001, "journal_incompat_revoke"),
    (0x0002, "journal_64bit"),
    (0x0004, "journal_async_commit"),
    (0x0008, "journal_checksum_v2"),
    (0x0010, "journal_checksum_v3"),
    (0x0020, "fast_commit"),
];

pub struct Journal {
    pub inum: u32,
    pub block_type: u32,
    pub blocksize: u32,
    pub maxlen: u32,
    pub first: u32,
    pub sequence: u32,
    pub start: u32,
    pub errno: i32,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    pub nr_users: u32,
    pub dynsuper: u32,
    pub max_transaction: u32,
    pub max_trans_data: u32,
    pub checksum_type: u8,
    pub num_fc_blks: u32,
    pub head: u32,
    pub checksum: u32,
}

pub fn names(flags: u32, table: &[(u32, &'static str)]) -> Vec<&'static str> {
    table
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .map(|(_, n)| *n)
        .collect()
}

// Read the journal superblock out of the journal inode's first block.
// Returns None when the filesystem has no journal at all, which is
// every ext2 image in the corpus and not an error.
pub fn read(img: &Img, sb: &Sb) -> Result<Option<Journal>> {
    if !sb.has_compat(COMPAT_HAS_JOURNAL) {
        return Ok(None);
    }
    if sb.journal_inum == 0 {
        // An external journal lives on another device, which this
        // cannot open and does not pretend to.
        return bad("the journal is on an external device, which extv does not read");
    }
    let ino = inode::read(img, sb, sb.journal_inum)?;
    let blocks = map::data_blocks(img, sb, &ino)?;
    let first = match blocks.iter().find(|(l, _)| *l == 0) {
        Some((_, p)) => *p,
        None => return bad("the journal inode has no block 0"),
    };
    let buf = img.block(first)?;
    let v = V::new(&buf, "journal superblock");
    let magic = v.u32be(0)?;
    if magic != MAGIC {
        return bad(format!(
            "the journal's first block has magic 0x{:08x}, not 0x{:08x}",
            magic, MAGIC
        ));
    }
    let block_type = v.u32be(4)?;
    if block_type != 3 && block_type != 4 {
        return bad(format!(
            "the journal's first block is a {} block, not a superblock",
            block_type_name(block_type)
        ));
    }
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(v.bytes(48, 16)?);
    let j = Journal {
        inum: sb.journal_inum,
        block_type,
        blocksize: v.u32be(12)?,
        maxlen: v.u32be(16)?,
        first: v.u32be(20)?,
        sequence: v.u32be(24)?,
        start: v.u32be(28)?,
        errno: v.u32be(32)? as i32,
        feature_compat: v.u32be(36)?,
        feature_incompat: v.u32be(40)?,
        feature_ro_compat: v.u32be(44)?,
        uuid,
        nr_users: v.u32be(64)?,
        dynsuper: v.u32be(68)?,
        max_transaction: v.u32be(72)?,
        max_trans_data: v.u32be(76)?,
        checksum_type: v.u8(80)?,
        num_fc_blks: v.u32be(84)?,
        head: v.u32be(88)?,
        checksum: v.u32be(252)?,
    };
    if j.blocksize != sb.block_size {
        return bad(format!(
            "the journal says its block size is {}, the filesystem says {}",
            j.blocksize, sb.block_size
        ));
    }
    if j.first == 0 || j.first >= j.maxlen {
        return bad(format!(
            "the journal's first log block is {} and the journal is {} blocks long",
            j.first, j.maxlen
        ));
    }
    Ok(Some(j))
}
