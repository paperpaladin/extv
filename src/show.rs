use crate::err::Result;
use crate::csum;
use crate::group;
use crate::inode;
use crate::journal;
use crate::dir;
use crate::dx;
use crate::extent;
use crate::map;
use crate::img::Img;
use crate::sb::Sb;
use crate::xattr;
use std::io::Write;

// Output is one "key value" pair per line, flat, with no alignment or
// headings. It is read by the test harness far more often than by a
// person, and a field-by-field comparison against dumpe2fs needs a shape
// that splits on the first space. Values that e2fsprogs prints in hex are
// printed in hex here for the same reason.
pub fn super_block(w: &mut impl Write, img: &Img, sb: &Sb) -> Result<()> {
    let mut p = |k: &str, v: String| -> Result<()> {
        writeln!(w, "{} {}", k, v)?;
        Ok(())
    };
    p("magic", format!("0x{:04X}", crate::sb::MAGIC))?;
    p("rev_level", sb.rev_level.to_string())?;
    p("volume_name", sb.volume_name.clone())?;
    p("last_mounted", sb.last_mounted.clone())?;
    p("uuid", sb.uuid_string())?;
    p("features", sb.feature_names().join(" "))?;
    p("feature_compat", format!("0x{:x}", sb.feature_compat))?;
    p("feature_incompat", format!("0x{:x}", sb.feature_incompat))?;
    p("feature_ro_compat", format!("0x{:x}", sb.feature_ro_compat))?;
    p("flags", format!("0x{:x}", sb.flags))?;
    p("default_mount_opts", format!("0x{:x}", sb.default_mount_opts))?;
    p("state", sb.state.to_string())?;
    p("state_name", sb.state_name())?;
    p("errors_name", sb.errors_name().to_string())?;
    p("os_name", sb.os_name().to_string())?;
    p("mount_opts", sb.mount_opt_names().join(" "))?;
    p("flag_names", sb.flag_names().join(" "))?;
    p("errors", sb.errors.to_string())?;
    p("creator_os", sb.creator_os.to_string())?;
    p("inode_count", sb.inodes_count.to_string())?;
    p("block_count", sb.blocks_count.to_string())?;
    p("r_blocks_count", sb.r_blocks_count.to_string())?;
    p("overhead_clusters", sb.overhead_clusters.to_string())?;
    p("free_blocks", sb.free_blocks.to_string())?;
    p("free_inodes", sb.free_inodes.to_string())?;
    p("first_data_block", sb.first_data_block.to_string())?;
    p("block_size", sb.block_size.to_string())?;
    p("cluster_size", sb.cluster_size.to_string())?;
    p("desc_size", sb.desc_size.to_string())?;
    p("reserved_gdt_blocks", sb.reserved_gdt_blocks.to_string())?;
    p("blocks_per_group", sb.blocks_per_group.to_string())?;
    p("clusters_per_group", sb.clusters_per_group.to_string())?;
    p("inodes_per_group", sb.inodes_per_group.to_string())?;
    p("inode_blocks_per_group", sb.inode_blocks_per_group().to_string())?;
    p("group_count", sb.group_count.to_string())?;
    p("flex_bg_size", sb.flex_bg_size().to_string())?;
    p("mkfs_time", sb.mkfs_time.to_string())?;
    p("mtime", sb.mtime.to_string())?;
    p("wtime", sb.wtime.to_string())?;
    p("mnt_count", sb.mnt_count.to_string())?;
    p("max_mnt_count", sb.max_mnt_count.to_string())?;
    p("lastcheck", sb.lastcheck.to_string())?;
    p("checkinterval", sb.checkinterval.to_string())?;
    p("kbytes_written", sb.kbytes_written.to_string())?;
    p("def_resuid", sb.def_resuid.to_string())?;
    p("def_resgid", sb.def_resgid.to_string())?;
    p("first_ino", sb.first_ino.to_string())?;
    p("inode_size", sb.inode_size.to_string())?;
    p("min_extra_isize", sb.min_extra_isize.to_string())?;
    p("want_extra_isize", sb.want_extra_isize.to_string())?;
    p("journal_inum", sb.journal_inum.to_string())?;
    p("last_orphan", sb.last_orphan.to_string())?;
    p("orphan_file_inum", sb.orphan_file_inum.to_string())?;
    p("lpf_ino", sb.lpf_ino.to_string())?;
    p("usr_quota_inum", sb.usr_quota_inum.to_string())?;
    p("grp_quota_inum", sb.grp_quota_inum.to_string())?;
    p("prj_quota_inum", sb.prj_quota_inum.to_string())?;
    p("hash_version", sb.hash_version_name().to_string())?;
    p("hash_seed", sb.hash_seed_string())?;
    p("checksum_type", sb.checksum_type.to_string())?;
    p("checksum_seed", format!("0x{:x}", sb.checksum_seed))?;
    p("checksum", format!("0x{:x}", sb.checksum))?;
    p("image_bytes", img.len.to_string())?;
    Ok(())
}

pub fn groups(w: &mut impl Write, img: &Img, sb: &Sb) -> Result<()> {
    for g in 0..sb.group_count {
        let d = group::read_group(img, sb, g)?;
        let first = group::group_first_block(sb, g);
        let last = (first + sb.blocks_per_group as u64 - 1).min(sb.blocks_count - 1);
        let mut p = |k: &str, v: String| -> Result<()> {
            writeln!(w, "group {} {} {}", g, k, v)?;
            Ok(())
        };
        p("first_block", first.to_string())?;
        p("last_block", last.to_string())?;
        p("has_super", if group::has_super(sb, g) { "1" } else { "0" }.to_string())?;
        p("block_bitmap", d.block_bitmap.to_string())?;
        p("inode_bitmap", d.inode_bitmap.to_string())?;
        p("inode_table", d.inode_table.to_string())?;
        p("free_blocks", d.free_blocks.to_string())?;
        p("free_inodes", d.free_inodes.to_string())?;
        p("used_dirs", d.used_dirs.to_string())?;
        p("unused_inodes", d.unused_inodes.to_string())?;
        let flags = if sb.has_group_desc_csum() {
            d.flag_names().join(",")
        } else {
            String::new()
        };
        p("flags", flags)?;
        p("checksum", format!("0x{:04x}", d.checksum))?;
        p("block_bitmap_csum", format!("0x{:x}", d.block_bitmap_csum))?;
        p("inode_bitmap_csum", format!("0x{:x}", d.inode_bitmap_csum))?;
    }
    Ok(())
}

// Checksums, recomputed and compared against what is on disk. mke2fs
// wrote those numbers with an implementation written by somebody else,
// so agreement is an external check and not a comparison of extv with
// itself.
pub fn checksums(w: &mut impl Write, img: &Img, sb: &Sb) -> Result<()> {
    // Which form is in use is printed first, so the suite can assert
    // that the corpus reaches both. A crc16 implementation that is never
    // run agrees with everything.
    writeln!(
        w,
        "form {}",
        if sb.has_ro(crate::sb::RO_METADATA_CSUM) {
            "crc32c"
        } else if sb.has_ro(crate::sb::RO_GDT_CSUM) {
            "crc16"
        } else {
            "none"
        }
    )?;
    let s = csum::superblock(sb);
    writeln!(
        w,
        "super stored 0x{:08x} computed 0x{:08x} {}",
        sb.checksum,
        s,
        if !sb.has_ro(crate::sb::RO_METADATA_CSUM) {
            "n/a"
        } else if s == sb.checksum {
            "ok"
        } else {
            "BAD"
        }
    )?;
    if csum::group_desc_applies(sb) {
        for g in 0..sb.group_count {
            let (stored, computed) = csum::read_group_csum(img, sb, g)?;
            writeln!(
                w,
                "group {} stored 0x{:04x} computed 0x{:04x} {}",
                g,
                stored,
                computed,
                if stored == computed { "ok" } else { "BAD" }
            )?;
        }
    }
    Ok(())
}

pub fn stat(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let i = inode::read(img, sb, num)?;
    let mut p = |k: &str, v: String| -> Result<()> {
        writeln!(w, "{} {}", k, v)?;
        Ok(())
    };
    p("inode", i.num.to_string())?;
    p("in_use", if i.in_use() { "yes" } else { "no" }.to_string())?;
    p("type", i.type_name().to_string())?;
    p("mode", format!("{:04o}", i.mode & 0o7777))?;
    p("mode_raw", format!("0x{:04x}", i.mode))?;
    p("flags", format!("0x{:x}", i.flags))?;
    p("generation", i.generation.to_string())?;
    p("version", format!("0x{:08x}:{:08x}", i.version >> 32, i.version as u32))?;
    p("uid", i.uid.to_string())?;
    p("gid", i.gid.to_string())?;
    p("projid", i.projid.to_string())?;
    p("size", i.size.to_string())?;
    p("file_acl", i.file_acl.to_string())?;
    p("links", i.links_count.to_string())?;
    p("blockcount", i.blocks.to_string())?;
    p("atime", format!("0x{:08x}:{:08x}", i.atime, i.atime_extra))?;
    p("ctime", format!("0x{:08x}:{:08x}", i.ctime, i.ctime_extra))?;
    p("mtime", format!("0x{:08x}:{:08x}", i.mtime, i.mtime_extra))?;
    p("crtime", format!("0x{:08x}:{:08x}", i.crtime, i.crtime_extra))?;
    p("dtime", format!("0x{:08x}", i.dtime))?;
    p("extra_isize", i.extra_isize.to_string())?;
    p("checksum", format!("0x{:x}", i.checksum))?;
    if sb.has_ro(crate::sb::RO_METADATA_CSUM) {
        let c = csum::inode(sb, num, &i.raw, i.extra_isize);
        p("checksum_computed", format!("0x{:x}", c))?;
        p("checksum_ok", if c == i.checksum { "yes" } else { "no" }.to_string())?;
    }
    Ok(())
}

// The block map of one inode, in the order and with the collapsing
// debugfs uses, plus a summary line the suite reads to assert that the
// deep paths were actually taken by something in the corpus.
pub fn blockmap(w: &mut impl Write, img: &Img, sb: &Sb, num: u32, flat: bool) -> Result<()> {
    let i = inode::read(img, sb, num)?;
    let (entries, counts) = map::walk(img, sb, &i)?;
    // The inode number is printed even for one inode, because the
    // harness asks for a range and otherwise could not tell where one
    // map ends and the next begins.
    writeln!(w, "inode {}", num)?;
    for e in &entries {
        match e {
            map::Entry::Data {
                lo,
                hi,
                plo,
                phi,
                uninit,
            } => {
                if flat {
                    for k in 0..=(hi - lo) {
                        writeln!(w, "{}", plo + k)?;
                    }
                } else {
                    writeln!(
                        w,
                        "data {} {} {} {} {}",
                        lo,
                        hi,
                        plo,
                        phi,
                        if *uninit { "u" } else { "-" }
                    )?;
                }
            }
            map::Entry::Meta { kind, phys } => {
                if flat {
                    writeln!(w, "{}", phys)?;
                } else {
                    writeln!(w, "meta {} {}", kind, phys)?;
                }
            }
        }
    }
    if !flat {
        writeln!(
            w,
            "summary data {} ind {} dind {} tind {} etb {} depth {} uninit {} \
inline {} fast_symlink {}",
            counts.data,
            counts.ind,
            counts.dind,
            counts.tind,
            counts.etb,
            counts.depth,
            counts.uninit,
            counts.inline,
            counts.fast_symlink
        )?;
    }
    Ok(())
}

// One line per row of what debugfs calls dump_extents. An inode with no
// extent tree prints nothing, the way debugfs prints only its heading.
pub fn extents(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let i = inode::read(img, sb, num)?;
    writeln!(w, "inode {}", num)?;
    if !i.has_extents() {
        return Ok(());
    }
    for r in extent::rows(img, sb, &i)? {
        writeln!(
            w,
            "row {} {} {} {} {} {} {} {} {} {}",
            r.level,
            r.max_depth,
            r.index,
            r.entries,
            r.log_first,
            r.log_last,
            r.phys_first,
            r.phys_last,
            r.len,
            if r.uninit { "Uninit" } else { "-" }
        )?;
    }
    Ok(())
}

// One line per directory entry: the fields debugfs ls -l prints, with
// the inode's own fields joined on, and the name escaped the way
// debugfs escapes it.
pub fn ls(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let ino = inode::read(img, sb, num)?;
    let d = dir::read(img, sb, &ino)?;
    writeln!(w, "dir {}", num)?;
    for e in &d.entries {
        let child = inode::read(img, sb, e.inode)?;
        writeln!(
            w,
            "entry {} {:o} {} {} {} {} {}",
            e.inode,
            child.mode,
            e.file_type,
            child.uid,
            child.gid,
            child.size,
            dir::escape(&e.name)
        )?;
    }
    writeln!(
        w,
        "summary entries {} blocks {} tails {} empty_slots {} wide_rec_len {} inline {}",
        d.entries.len(),
        d.blocks,
        d.tails,
        d.empty_slots,
        d.wide_rec_len,
        if d.inline { 1 } else { 0 }
    )?;
    Ok(())
}

// Extended attributes, in the order they are stored: the ones inside
// the inode first, then the ones in the attribute block. The value is
// printed as hex because an attribute value is bytes, and the only
// comparison worth making is byte for byte.
pub fn ea(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let ino = inode::read(img, sb, num)?;
    let attrs = xattr::read(img, sb, &ino)?;
    writeln!(w, "inode {}", num)?;
    for a in &attrs {
        let hex: String = a.value.iter().map(|b| format!("{:02x}", b)).collect();
        writeln!(
            w,
            "attr {} {} {} {} {}",
            a.source,
            a.full_name(),
            a.value.len(),
            if a.value_inum != 0 { "ea_inode" } else { "-" },
            hex
        )?;
    }
    Ok(())
}

// The contents of one file, written as bytes. This is the end to end
// read: the block map or the extent tree, the holes, the inline data
// and the size all have to be right together, and debugfs dump
// produces the same bytes from an independent implementation.
pub fn cat(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let ino = inode::read(img, sb, num)?;
    if ino.has_inline_data() {
        let attrs = xattr::read(img, sb, &ino)?;
        let mut buf = ino.block.to_vec();
        if let Some(tail) = xattr::inline_tail(&attrs) {
            buf.extend_from_slice(&tail.value);
        }
        buf.truncate(ino.size as usize);
        w.write_all(&buf)?;
        return Ok(());
    }
    if ino.is_fast_symlink(sb.block_size) {
        let n = (ino.size as usize).min(60);
        w.write_all(&ino.block[..n])?;
        return Ok(());
    }
    let blocks = map::data_blocks(img, sb, &ino)?;
    let bsize = sb.block_size as u64;
    let mut left = ino.size;
    let mut logical = 0u64;
    let zeros = vec![0u8; bsize as usize];
    let mut iter = blocks.into_iter().peekable();
    while left > 0 {
        let want = left.min(bsize) as usize;
        // A hole reads as zeroes. The map holds only the blocks that
        // exist, so anything it skips is a hole and not an error.
        while let Some(&(lo, _)) = iter.peek() {
            if lo < logical {
                iter.next();
            } else {
                break;
            }
        }
        match iter.peek() {
            Some(&(lo, phys)) if lo == logical => {
                let b = img.block(phys)?;
                w.write_all(&b[..want])?;
                iter.next();
            }
            _ => w.write_all(&zeros[..want])?,
        }
        left -= want as u64;
        logical += 1;
    }
    Ok(())
}

// The hash index of one directory, with the hash of every name
// recomputed from the name itself rather than read from the tree.
// debugfs htree_dump prints both, so each name is one comparison
// against somebody else's implementation of the hash.
pub fn htree(w: &mut impl Write, img: &Img, sb: &Sb, num: u32) -> Result<()> {
    let ino = inode::read(img, sb, num)?;
    if !ino.is_dir() || !dx::is_indexed(&ino) {
        return Ok(());
    }
    let (root, visits) = dx::read(img, sb, &ino)?;
    writeln!(w, "htree {}", num)?;
    writeln!(
        w,
        "root reserved_zero {} hash_version {} info_length {} indirect_levels {} flags {}",
        root.reserved_zero, root.hash_version, root.info_length, root.indirect_levels,
        root.flags
    )?;
    writeln!(
        w,
        "hash effective_version {}",
        dx::effective_version(sb, &root)
    )?;
    for v in &visits {
        match v {
            dx::Visit::Node(n) => {
                writeln!(
                    w,
                    "node count {} limit {} checksum {}",
                    n.count,
                    n.limit,
                    match n.checksum {
                        Some(c) => format!("0x{:08x}", c),
                        None => "-".to_string(),
                    }
                )?;
                for (i, (h, b)) in n.entries.iter().enumerate() {
                    writeln!(w, "nodeentry {} 0x{:08x} {}", i, h, b)?;
                }
            }
            dx::Visit::Leaf(l) => {
                writeln!(w, "leaf {} {}", l.logical, l.physical)?;
                for (ino_num, name, rec_len) in &l.names {
                    let (h, minor) = dx::name_hash(sb, &root, name);
                    writeln!(
                        w,
                        "name {} 0x{:08x} 0x{:08x} {} {}",
                        ino_num,
                        h,
                        minor,
                        rec_len,
                        dir::escape(name)
                    )?;
                }
            }
        }
    }
    Ok(())
}

// The journal superblock, one key and one value per line. dumpe2fs
// prints the same handful of fields at the end of its output, which is
// what the comparison reads. An image with no journal prints one line
// saying so rather than nothing, because nothing is also what a broken
// reader prints.
pub fn journal(w: &mut impl Write, img: &Img, sb: &Sb) -> Result<()> {
    let j = match journal::read(img, sb)? {
        None => {
            writeln!(w, "journal none")?;
            return Ok(());
        }
        Some(j) => j,
    };
    writeln!(w, "journal inode {}", j.inum)?;
    writeln!(w, "block_type {}", j.block_type)?;
    writeln!(w, "blocksize {}", j.blocksize)?;
    writeln!(w, "blocks {}", j.maxlen)?;
    writeln!(w, "size_k {}", (j.maxlen as u64 * j.blocksize as u64) / 1024)?;
    writeln!(w, "first {}", j.first)?;
    writeln!(w, "sequence {}", j.sequence)?;
    writeln!(w, "start {}", j.start)?;
    writeln!(w, "errno {}", j.errno)?;
    writeln!(w, "max_transaction {}", j.max_transaction)?;
    writeln!(w, "max_trans_data {}", j.max_trans_data)?;
    writeln!(w, "fast_commit_blocks {}", j.num_fc_blks)?;
    writeln!(w, "nr_users {}", j.nr_users)?;
    writeln!(w, "uuid {}", crate::sb::uuid_string(&j.uuid))?;
    writeln!(w, "checksum_type {}", j.checksum_type)?;
    writeln!(w, "head {}", j.head)?;
    let mut f = journal::names(j.feature_compat, journal::COMPAT_FLAGS);
    f.extend(journal::names(j.feature_incompat, journal::INCOMPAT_FLAGS));
    writeln!(
        w,
        "features {}",
        if f.is_empty() {
            "(none)".to_string()
        } else {
            f.join(" ")
        }
    )?;
    writeln!(w, "unknown_compat 0x{:08x}", j.feature_compat & !0x1)?;
    writeln!(
        w,
        "unknown_incompat 0x{:08x}",
        j.feature_incompat & !0x3f
    )?;
    writeln!(w, "ro_compat 0x{:08x}", j.feature_ro_compat)?;
    Ok(())
}
