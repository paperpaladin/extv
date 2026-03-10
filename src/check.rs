#![allow(dead_code)]
// The consistency check. What e2fsck's pass 5 does: work out which
// blocks are actually in use by walking every inode, and compare that
// against the bitmaps the filesystem carries.
//
// Nothing here repairs anything, and nothing here guesses. An image
// whose structures cannot be read at all is a refusal (exit 2) and not
// an inconsistency (exit 1), because the two mean different things:
// the first says extv cannot tell, the second says extv can tell and
// the answer is no.
use crate::bitmap::{self, Bits};
use crate::err::Result;
use crate::group::{self, Group};
use crate::img::Img;
use crate::inode;
use crate::map::{self, Entry};
use crate::sb::Sb;

pub struct Report {
    pub problems: Vec<String>,
    pub blocks_in_use: u64,
    pub inodes_walked: u64,
    pub blocks_claimed: u64,
    pub shared_blocks: u64,
    pub counters_checked: u64,
    pub dirs: u64,
}

impl Report {
    pub fn clean(&self) -> bool {
        self.problems.is_empty()
    }
}

// Which inodes hold data. A reserved inode is in use whatever it says
// about itself: the bitmap marks inodes 1 to first_ino - 1 and so does
// e2fsck, even for the ones mke2fs leaves entirely blank. Every other
// inode is in use when something links to it and it has not been
// deleted.
pub fn in_use(sb: &Sb, i: &inode::Inode) -> bool {
    if i.num < sb.first_ino {
        return true;
    }
    i.links_count > 0 && i.dtime == 0
}

// Every block the filesystem's own structures occupy, before any inode
// is looked at.
fn mark_metadata(sb: &Sb, gs: &[Group], used: &mut Bits, ratio: u64) {
    for (g, gd) in gs.iter().enumerate() {
        for b in bitmap::group_metadata_blocks(sb, g as u64, gd) {
            used.set(b / ratio);
        }
    }
}

pub fn run(img: &Img, sb: &Sb) -> Result<Report> {
    let gs = group::read_all(img, sb)?;
    let ratio = bitmap::cluster_ratio(sb);
    let first = sb.first_data_block as u64 / ratio;
    let n = first + bitmap::clusters_count(sb);
    let mut used = Bits::new(n);
    let mut problems = Vec::new();
    mark_metadata(sb, &gs, &mut used, ratio);
    // The superblock itself, which in a 1024-byte-block filesystem is
    // block 1 and in every other one shares block 0 with the boot
    // area. has_super covers group 0 already; this is here for the
    // block that holds nothing else.
    used.set(sb.first_data_block as u64 / ratio);

    let mut inodes_used = Bits::new(sb.inodes_count as u64);
    let mut dirs_in = Bits::new(sb.inodes_count as u64);
    let mut owner: Vec<u32> = Vec::new();
    let mut inodes_walked = 0u64;
    let mut blocks_claimed = 0u64;
    let mut shared = 0u64;
    // Who claimed each cluster, so that a block claimed twice names
    // both inodes rather than just saying the count is wrong. e2fsck
    // calls this a multiply-claimed block and it is its pass 1b.
    owner.resize(n as usize, 0);

    for num in 1..=sb.inodes_count {
        let ino = inode::read(img, sb, num)?;
        if !in_use(sb, &ino) {
            continue;
        }
        inodes_walked += 1;
        inodes_used.set(num as u64 - 1);
        if ino.is_dir() {
            dirs_in.set(num as u64 - 1);
        }
        let mut claim = |b: u64, problems: &mut Vec<String>| {
            if b < sb.first_data_block as u64 || b >= sb.blocks_count {
                problems.push(format!(
                    "inode {} claims block {}, which is outside the filesystem",
                    num, b
                ));
                return;
            }
            let c = b / ratio;
            let prev = owner[c as usize];
            if prev != 0 && prev != num {
                shared += 1;
                problems.push(format!(
                    "block {} is claimed by inode {} and by inode {}",
                    b, prev, num
                ));
            }
            owner[c as usize] = num;
            blocks_claimed += 1;
            used.set(c);
        };
        if ino.file_acl != 0 {
            claim(ino.file_acl, &mut problems);
        }
        let (entries, _) = map::walk(img, sb, &ino)?;
        for e in entries {
            match e {
                Entry::Data { plo, phi, .. } => {
                    for b in plo..=phi {
                        claim(b, &mut problems);
                    }
                }
                Entry::Meta { phys, .. } => claim(phys, &mut problems),
            }
        }
    }

    let on_disk = bitmap::block_bitmap(img, sb, &gs)?;
    let mut wrong = 0u64;
    for c in first..n {
        let a = used.get(c);
        let b = on_disk.get(c);
        if a == b {
            continue;
        }
        wrong += 1;
        if wrong <= 8 {
            let blk = c * ratio;
            if a {
                // Owner 0 means nothing claimed it: it is one of the
                // filesystem's own blocks, a bitmap or a piece of the
                // inode table, which no inode owns.
                let by = match owner[c as usize] {
                    0 => "the filesystem's own metadata".to_string(),
                    n => format!("inode {}", n),
                };
                problems.push(format!(
                    "block {} is in use by {} and the block bitmap says it is free",
                    blk, by
                ));
            } else {
                problems.push(format!(
                    "block {} is marked in use in the block bitmap and nothing claims it",
                    blk
                ));
            }
        }
    }
    if wrong > 8 {
        problems.push(format!(
            "and {} more blocks where the bitmap and the inodes disagree",
            wrong - 8
        ));
    }

    // The inode bitmap, and then the counters in every group
    // descriptor and in the superblock. A counter is a summary of a
    // bitmap, so a filesystem can have the bitmap right and the
    // counter wrong, and e2fsck reports exactly that as a problem.
    let ib = bitmap::inode_bitmap(img, sb, &gs)?;
    let mut iwrong = 0u64;
    for i in 0..sb.inodes_count as u64 {
        if inodes_used.get(i) == ib.get(i) {
            continue;
        }
        iwrong += 1;
        if iwrong <= 8 {
            problems.push(if inodes_used.get(i) {
                format!(
                    "inode {} is in use and the inode bitmap says it is free",
                    i + 1
                )
            } else {
                format!(
                    "inode {} is marked in use in the inode bitmap and nothing links to it",
                    i + 1
                )
            });
        }
    }
    if iwrong > 8 {
        problems.push(format!(
            "and {} more inodes where the bitmap and the links disagree",
            iwrong - 8
        ));
    }

    let mut counters = 0u64;
    let mut total_free_clusters = 0u64;
    let mut total_free_inodes = 0u64;
    for (g, gd) in gs.iter().enumerate() {
        let g = g as u64;
        let base = first + g * sb.clusters_per_group as u64;
        let span = (sb.clusters_per_group as u64).min(n.saturating_sub(base));
        let free = (0..span).filter(|&i| !used.get(base + i)).count() as u64;
        total_free_clusters += free;
        if free != gd.free_blocks as u64 {
            problems.push(format!(
                "group {} says it has {} free blocks and it has {}",
                g, gd.free_blocks, free
            ));
        }
        counters += 1;

        let ibase = g * sb.inodes_per_group as u64;
        let ispan = (sb.inodes_per_group as u64)
            .min((sb.inodes_count as u64).saturating_sub(ibase));
        let ifree = (0..ispan).filter(|&i| !inodes_used.get(ibase + i)).count() as u64;
        total_free_inodes += ifree;
        if ifree != gd.free_inodes as u64 {
            problems.push(format!(
                "group {} says it has {} free inodes and it has {}",
                g, gd.free_inodes, ifree
            ));
        }
        counters += 1;

        let d = (0..ispan)
            .filter(|&i| dirs_in.get(ibase + i))
            .count() as u64;
        if d != gd.used_dirs as u64 {
            problems.push(format!(
                "group {} says it holds {} directories and it holds {}",
                g, gd.used_dirs, d
            ));
        }
        counters += 1;

        // itable_unused counts inodes at the END of the group that
        // have never been used, so the real constraint is that no
        // inode inside that tail is in use. A value that is too small
        // is allowed and costs only a slower fsck.
        if sb.has_ro(crate::sb::RO_GDT_CSUM) || sb.has_ro(crate::sb::RO_METADATA_CSUM) {
            let unused = gd.unused_inodes as u64;
            if unused > ispan {
                problems.push(format!(
                    "group {} says {} of its inodes are unused and it has {}",
                    g, unused, ispan
                ));
            } else {
                for i in (ispan - unused)..ispan {
                    if inodes_used.get(ibase + i) {
                        problems.push(format!(
                            "group {} says its last {} inodes are unused and inode {} is in use",
                            g,
                            unused,
                            ibase + i + 1
                        ));
                        break;
                    }
                }
            }
            counters += 1;
        }
    }
    // The group descriptors count free clusters and the superblock
    // counts free blocks, so under bigalloc the two differ by the
    // cluster ratio. Comparing them as the same unit made the bigalloc
    // image the only one that failed, with the superblock's 12316
    // against the groups' 3079.
    if total_free_clusters * ratio != sb.free_blocks {
        problems.push(format!(
            "the superblock says {} free blocks and the groups hold {}",
            sb.free_blocks,
            total_free_clusters * ratio
        ));
    }
    counters += 1;
    if total_free_inodes != sb.free_inodes as u64 {
        problems.push(format!(
            "the superblock says {} free inodes and the groups hold {}",
            sb.free_inodes, total_free_inodes
        ));
    }
    counters += 1;

    Ok(Report {
        problems,
        blocks_in_use: used.count_set(),
        inodes_walked,
        blocks_claimed,
        shared_blocks: shared,
        counters_checked: counters,
        dirs: dirs_in.count_set(),
    })
}
