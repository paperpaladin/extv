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
use crate::err::{bad, Result};
use crate::group::{self, Group};
use crate::img::Img;
use crate::inode;
use crate::dir;
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
    pub links_checked: u64,
    pub dirents: u64,
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

    let mut links: Vec<u32> = vec![0; sb.inodes_count as usize];
    let mut dirents = 0u64;
    let mut inodes_used = Bits::new(sb.inodes_count as u64);
    let mut dirs_in = Bits::new(sb.inodes_count as u64);
    let mut owner: Vec<u32> = Vec::new();
    let mut inodes_walked = 0u64;
    let mut blocks_wrong = 0u64;
    let mut csums_wrong = 0u64;
    let mut blocks_claimed = 0u64;
    let mut shared = 0u64;
    // Who claimed each cluster, so that a block claimed twice names
    // both inodes rather than just saying the count is wrong. e2fsck
    // calls this a multiply-claimed block and it is its pass 1b.
    owner.resize(n as usize, 0);

    for num in 1..=sb.inodes_count {
        let ino = inode::read(img, sb, num)?;
        // A free inode whose deletion time is small enough to be an
        // inode number was left on an orphan list that nobody
        // finished: the list stores the next entry in i_dtime, so a
        // real deletion time is a Unix timestamp and this is not one.
        // e2fsck calls it an inode that was part of the orphaned
        // inode list, and the single-bit sweep found extv calling
        // such an image clean.
        if ino.links_count == 0 && ino.dtime != 0 && ino.dtime < sb.inodes_count {
            problems.push(format!(
                "inode {} is free and its deletion time is {}, which is an \
                 inode number: it was left on the orphan list",
                num, ino.dtime
            ));
        }
        if !in_use(sb, &ino) {
            continue;
        }
        inodes_walked += 1;
        inodes_used.set(num as u64 - 1);
        if ino.is_dir() {
            dirs_in.set(num as u64 - 1);
            // Every entry counts towards the link count of the inode
            // it names, "." and ".." included: that is why a directory
            // with no subdirectories has two links and gains one per
            // child directory.
            for e in dir::read(img, sb, &ino)?.entries {
                dirents += 1;
                if e.inode >= 1 && e.inode <= sb.inodes_count {
                    links[e.inode as usize - 1] += 1;
                }
            }
        }
        let mut mine: Vec<u64> = Vec::new();
        let mut claim = |b: u64, problems: &mut Vec<String>, mine: &mut Vec<u64>| {
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
            mine.push(c);
            used.set(c);
        };
        if ino.file_acl != 0 {
            claim(ino.file_acl, &mut problems, &mut mine);
        }
        let (entries, _) = map::walk(img, sb, &ino)?;
        for e in entries {
            match e {
                Entry::Data { plo, phi, .. } => {
                    for b in plo..=phi {
                        claim(b, &mut problems, &mut mine);
                    }
                }
                Entry::Meta { phys, .. } => claim(phys, &mut problems, &mut mine),
            }
        }
        // i_blocks counts 512-byte sectors, not blocks, and it covers
        // the indirect and extent tree blocks and the attribute block
        // as well as the data. With huge_file and the inode's own huge
        // flag it is in filesystem blocks instead, which no image here
        // has, so that case is left alone rather than guessed at.
        if !ino.counts_blocks_in_clusters() {
            // Under bigalloc a file owns whole clusters and i_blocks
            // counts every sector of them, used or not, so the count
            // is of distinct clusters. Without bigalloc it is a count
            // of block references and not of distinct blocks: an
            // inode that names the same block twice has it counted
            // twice. Deduplicating in both cases hid a corruption
            // that added a pointer to a block the inode already had,
            // where e2fsck saw two more sectors and extv saw none.
            let want = if ratio == 1 {
                mine.len() as u64 * (sb.block_size as u64 / 512)
            } else {
                mine.sort_unstable();
                mine.dedup();
                mine.len() as u64 * (sb.cluster_size as u64 / 512)
            };
            if want != ino.blocks {
                blocks_wrong += 1;
                if blocks_wrong <= 8 {
                    problems.push(format!(
                        "inode {} says it owns {} sectors and it owns {}",
                        num, ino.blocks, want
                    ));
                }
            }
        }
        // An inode with links pointing at it and a deletion time set
        // is a contradiction. e2fsck reports it as an inode in use
        // with dtime set, or, when the orphan list reaches it, as a
        // corrupted orphan linked list. The single-bit sweep found
        // this: it set a dtime on a live inode and extv called the
        // image clean.
        if num >= sb.first_ino && ino.links_count > 0 && ino.dtime != 0 {
            problems.push(format!(
                "inode {} has {} links and a deletion time of {}",
                num, ino.links_count, ino.dtime
            ));
        }
        // The inode's own checksum. e2fsck reports a mismatch as
        // "passes checks, but checksum does not match inode", which
        // is a different thing from the inode being wrong, and extv
        // reports it as a problem for the same reason.
        if sb.has_ro(crate::sb::RO_METADATA_CSUM) {
            let c = crate::csum::inode(sb, num, &ino.raw, ino.extra_isize);
            if c != ino.checksum {
                csums_wrong += 1;
                if csums_wrong <= 8 {
                    problems.push(format!(
                        "inode {} has checksum 0x{:x} and its contents give 0x{:x}",
                        num, ino.checksum, c
                    ));
                }
            }
        }
    }
    if blocks_wrong > 8 {
        problems.push(format!(
            "and {} more inodes whose sector count is not what they own",
            blocks_wrong - 8
        ));
    }
    if csums_wrong > 8 {
        problems.push(format!(
            "and {} more inodes whose checksum does not match",
            csums_wrong - 8
        ));
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

    // Link counts. e2fsck's pass 4, and it needs the whole directory
    // tree walked first, which the loop above has just done. The
    // reserved inodes are left out: nothing links to the journal or to
    // the resize inode and e2fsck does not expect anything to.
    // Some inodes past first_ino are still named by the superblock
    // rather than by a directory: the orphan file and the three quota
    // files. ext4 with the orphan_file feature puts one at inode 12,
    // and treating it as an unattached inode made every image with
    // that feature read as inconsistent while e2fsck called it clean.
    let unlinked_by_design = [
        sb.journal_inum,
        sb.orphan_file_inum,
        sb.usr_quota_inum,
        sb.grp_quota_inum,
        sb.prj_quota_inum,
    ];
    let mut links_checked = 0u64;
    let mut lwrong = 0u64;
    for num in sb.first_ino..=sb.inodes_count {
        if !inodes_used.get(num as u64 - 1) {
            continue;
        }
        if unlinked_by_design.contains(&num) {
            continue;
        }
        let want = links[num as usize - 1];
        let have = inode::read(img, sb, num)?.links_count;
        links_checked += 1;
        if want as u16 == have {
            continue;
        }
        lwrong += 1;
        if lwrong <= 8 {
            problems.push(if want == 0 {
                format!(
                    "inode {} is in use with {} links and no directory names it",
                    num, have
                )
            } else {
                format!(
                    "inode {} has a link count of {} and {} directory entries name it",
                    num, have, want
                )
            });
        }
    }
    if lwrong > 8 {
        problems.push(format!(
            "and {} more inodes whose link count is not the number of names",
            lwrong - 8
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

    // The bits past the end of the filesystem in the last group's two
    // bitmaps have to be set, so that nothing ever allocates them.
    // e2fsck's pass 5 calls a clear one "padding at end of block
    // bitmap is not set", and the single-bit sweep found extv calling
    // such an image clean. An uninitialised last group is left alone:
    // its bitmap is not written at all and e2fsck does not look.
    if let Some(gd) = gs.last() {
        let last = sb.group_count - 1;
        let cpg = sb.clusters_per_group as u64;
        if gd.flags & group::BG_BLOCK_UNINIT == 0 {
            let raw = bitmap::read_one(img, gd.block_bitmap, cpg)?;
            let from = bitmap::clusters_count(sb) - last * cpg;
            for i in from..cpg {
                if raw[(i / 8) as usize] & (1 << (i % 8)) == 0 {
                    problems.push(format!(
                        "the block bitmap of the last group has bit {} clear, \
                         past the end of the filesystem",
                        i
                    ));
                    break;
                }
            }
        }
        let ipg = sb.inodes_per_group as u64;
        if gd.flags & group::BG_INODE_UNINIT == 0 {
            let raw = bitmap::read_one(img, gd.inode_bitmap, ipg)?;
            let from = sb.inodes_count as u64 - last * ipg;
            for i in from..ipg {
                if raw[(i / 8) as usize] & (1 << (i % 8)) == 0 {
                    problems.push(format!(
                        "the inode bitmap of the last group has bit {} clear, \
                         past the last inode",
                        i
                    ));
                    break;
                }
            }
        }
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
        links_checked,
        dirents,
    })
}

pub const ORPHAN_BLOCK_MAGIC: u32 = 0x0b10_ca04;

pub struct Orphans {
    pub head: u32,
    pub chain: Vec<u32>,
    pub file_inum: u32,
    pub file_blocks: u64,
    pub file_entries: Vec<u32>,
    pub bad_magic: u64,
    pub problems: Vec<String>,
}

// The orphan list and the orphan file: the two places ext4 records
// inodes that were unlinked while still open, so that a fsck after a
// crash knows to finish deleting them.
//
// On a cleanly unmounted filesystem both are empty, which is every
// image here, because nothing in this project mounts anything. The
// head of the list is the one part with an outside oracle: dumpe2fs
// prints it as "First orphan inode". The rest of the chain has none,
// and the README says so.
//
// The chain is the interesting part to get wrong. Each orphan's
// i_dtime holds the number of the next one, so a damaged image can
// point an inode at itself and a reader that just follows the
// pointers never stops.
pub fn orphans(img: &Img, sb: &Sb) -> Result<Orphans> {
    let mut o = Orphans {
        head: sb.last_orphan,
        chain: Vec::new(),
        file_inum: sb.orphan_file_inum,
        file_blocks: 0,
        file_entries: Vec::new(),
        bad_magic: 0,
        problems: Vec::new(),
    };
    let mut seen = Bits::new(sb.inodes_count as u64 + 1);
    let mut num = sb.last_orphan;
    while num != 0 {
        // A loop and an inode number past the end of the table are
        // both refusals rather than reported problems. They are not
        // states a filesystem can be in, they are states an image can
        // claim, and following either one is how a reader is made to
        // run forever or read out of bounds.
        if num > sb.inodes_count {
            return bad(format!(
                "the orphan list names inode {}, past the last inode {}",
                num, sb.inodes_count
            ));
        }
        if seen.get(num as u64) {
            return bad(format!(
                "the orphan list reaches inode {} twice: it is a loop",
                num
            ));
        }
        seen.set(num as u64);
        o.chain.push(num);
        num = inode::read(img, sb, num)?.dtime;
    }
    if sb.orphan_file_inum != 0 {
        let ino = inode::read(img, sb, sb.orphan_file_inum)?;
        let per = (sb.block_size as usize - 8) / 4;
        for (_, phys) in map::data_blocks(img, sb, &ino)? {
            o.file_blocks += 1;
            let buf = img.block(phys)?;
            let v = crate::img::V::new(&buf, "orphan file block");
            if v.u32(sb.block_size as usize - 8)? != ORPHAN_BLOCK_MAGIC {
                o.bad_magic += 1;
                o.problems.push(format!(
                    "orphan file block {} does not carry the orphan magic",
                    phys
                ));
                continue;
            }
            for i in 0..per {
                let n = v.u32(i * 4)?;
                if n != 0 {
                    o.file_entries.push(n);
                }
            }
        }
    }
    Ok(o)
}
