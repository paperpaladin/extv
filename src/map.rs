#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::Img;
use crate::inode::Inode;
use crate::sb::Sb;

// One run of blocks, collapsed the way debugfs collapses them: a run
// grows while both the logical and the physical block number advance by
// one, and any metadata block ends the run in progress. Matching that
// rule exactly is what lets the two outputs be compared as facts rather
// than as text.
#[derive(Debug, PartialEq)]
pub enum Entry {
    Data {
        lo: u64,
        hi: u64,
        plo: u64,
        phi: u64,
        uninit: bool,
    },
    Meta {
        kind: &'static str,
        phys: u64,
    },
}

#[derive(Default)]
pub struct Counts {
    pub data: u64,
    pub ind: u64,
    pub dind: u64,
    pub tind: u64,
    pub etb: u64,
    pub depth: u64,
    pub uninit: u64,
    pub inline: u64,
    pub fast_symlink: u64,
}

pub struct Builder {
    pub out: Vec<Entry>,
    pub counts: Counts,
    cur: Option<(u64, u64, u64, u64, bool)>,
    // A file cannot own more blocks than the filesystem has. Without a
    // bound a damaged double indirect block that points at itself makes
    // this walk quadratic in the number of entries per block, which on a
    // 4096-byte block is a million reads per level.
    limit: u64,
    seen: u64,
}

impl Builder {
    pub fn new(sb: &Sb) -> Builder {
        Builder {
            out: Vec::new(),
            counts: Counts::default(),
            cur: None,
            limit: sb.blocks_count + 16,
            seen: 0,
        }
    }

    fn bump(&mut self) -> Result<()> {
        self.seen += 1;
        if self.seen > self.limit {
            return bad(format!(
                "block map names more than {} blocks, which is more than the filesystem has",
                self.limit
            ));
        }
        Ok(())
    }

    // A run also breaks when the uninitialised flag changes, which is
    // what debugfs does: it prints the marker on the logical range, so
    // an initialised and an uninitialised extent that happen to be
    // adjacent are two entries and not one.
    pub fn data(&mut self, logical: u64, phys: u64, uninit: bool) -> Result<()> {
        self.bump()?;
        self.counts.data += 1;
        match self.cur {
            Some((lo, hi, plo, phi, u))
                if hi + 1 == logical && phi + 1 == phys && u == uninit =>
            {
                self.cur = Some((lo, logical, plo, phys, u));
            }
            Some((lo, hi, plo, phi, u)) => {
                self.out.push(Entry::Data {
                    lo,
                    hi,
                    plo,
                    phi,
                    uninit: u,
                });
                self.cur = Some((logical, logical, phys, phys, uninit));
            }
            None => self.cur = Some((logical, logical, phys, phys, uninit)),
        }
        Ok(())
    }

    pub fn meta(&mut self, kind: &'static str, phys: u64) -> Result<()> {
        self.bump()?;
        match kind {
            "IND" => self.counts.ind += 1,
            "DIND" => self.counts.dind += 1,
            "TIND" => self.counts.tind += 1,
            _ => self.counts.etb += 1,
        }
        self.flush();
        self.out.push(Entry::Meta { kind, phys });
        Ok(())
    }

    fn flush(&mut self) {
        if let Some((lo, hi, plo, phi, uninit)) = self.cur.take() {
            self.out.push(Entry::Data {
                lo,
                hi,
                plo,
                phi,
                uninit,
            });
        }
    }

    pub fn finish(mut self) -> (Vec<Entry>, Counts) {
        self.flush();
        (self.out, self.counts)
    }
}

pub fn check_block(sb: &Sb, b: u64, what: &str) -> Result<()> {
    if b >= sb.blocks_count {
        return bad(format!(
            "{} points at block {}, past the end of a {}-block filesystem",
            what, b, sb.blocks_count
        ));
    }
    if b < sb.first_data_block as u64 {
        return bad(format!(
            "{} points at block {}, before the first data block {}",
            what, b, sb.first_data_block
        ));
    }
    Ok(())
}

pub fn walk(img: &Img, sb: &Sb, ino: &Inode) -> Result<(Vec<Entry>, Counts)> {
    let mut b = Builder::new(sb);
    if ino.has_inline_data() {
        b.counts.inline = 1;
    } else if ino.is_fast_symlink(sb.block_size) {
        b.counts.fast_symlink = 1;
    } else if ino.has_extents() {
        crate::extent::walk(img, sb, ino, &mut b)?;
    } else {
        crate::blockmap::walk(img, sb, ino, &mut b)?;
    }
    Ok(b.finish())
}
