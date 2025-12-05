use crate::err::Result;
use crate::group;
use crate::img::Img;
use crate::sb::Sb;
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
