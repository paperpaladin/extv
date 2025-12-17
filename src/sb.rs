// The feature tables below are written out in full on purpose: they are
// the format, not a list of what extv happens to use today, and a partial
// table is how a reader silently drops a feature it should have refused.
#![allow(dead_code)]
use crate::err::{bad, Result};
use crate::img::{Img, V};

pub const MAGIC: u16 = 0xEF53;
pub const SUPER_OFFSET: u64 = 1024;
pub const SUPER_SIZE: usize = 1024;

// Feature bits, with the names e2fsprogs prints. The names are not ours to
// choose: the suite compares the set of names dumpe2fs prints against the
// set extv prints, so a spelling that differs is a failure, which is the
// point of writing them out rather than printing hex.
pub const COMPAT: &[(u32, &str)] = &[
    (0x0001, "dir_prealloc"),
    (0x0002, "imagic_inodes"),
    (0x0004, "has_journal"),
    (0x0008, "ext_attr"),
    (0x0010, "resize_inode"),
    (0x0020, "dir_index"),
    (0x0040, "lazy_bg"),
    (0x0100, "snapshot_bitmap"),
    (0x0200, "sparse_super2"),
    (0x0400, "fast_commit"),
    (0x0800, "stable_inodes"),
    (0x1000, "orphan_file"),
];

pub const INCOMPAT: &[(u32, &str)] = &[
    (0x0001, "compression"),
    (0x0002, "filetype"),
    (0x0004, "needs_recovery"),
    (0x0008, "journal_dev"),
    (0x0010, "meta_bg"),
    (0x0040, "extent"),
    (0x0080, "64bit"),
    (0x0100, "mmp"),
    (0x0200, "flex_bg"),
    (0x0400, "ea_inode"),
    (0x1000, "dirdata"),
    (0x2000, "metadata_csum_seed"),
    (0x4000, "large_dir"),
    (0x8000, "inline_data"),
    (0x10000, "encrypt"),
    (0x20000, "casefold"),
];

pub const RO_COMPAT: &[(u32, &str)] = &[
    (0x0001, "sparse_super"),
    (0x0002, "large_file"),
    (0x0004, "btree_dir"),
    (0x0008, "huge_file"),
    (0x0010, "uninit_bg"),
    (0x0020, "dir_nlink"),
    (0x0040, "extra_isize"),
    (0x0080, "snapshot"),
    (0x0100, "quota"),
    (0x0200, "bigalloc"),
    (0x0400, "metadata_csum"),
    (0x0800, "replica"),
    (0x1000, "readonly"),
    (0x2000, "project"),
    (0x4000, "shared_blocks"),
    (0x8000, "verity"),
    (0x10000, "orphan_present"),
];

pub const INCOMPAT_64BIT: u32 = 0x0080;
pub const INCOMPAT_EXTENT: u32 = 0x0040;
pub const INCOMPAT_FILETYPE: u32 = 0x0002;
pub const INCOMPAT_META_BG: u32 = 0x0010;
pub const INCOMPAT_INLINE_DATA: u32 = 0x8000;
pub const INCOMPAT_CSUM_SEED: u32 = 0x2000;
pub const INCOMPAT_RECOVER: u32 = 0x0004;
pub const COMPAT_DIR_INDEX: u32 = 0x0020;
pub const COMPAT_HAS_JOURNAL: u32 = 0x0004;
pub const COMPAT_ORPHAN_FILE: u32 = 0x1000;
pub const RO_SPARSE_SUPER: u32 = 0x0001;
pub const RO_METADATA_CSUM: u32 = 0x0400;
pub const RO_GDT_CSUM: u32 = 0x0010;
pub const RO_BIGALLOC: u32 = 0x0200;
pub const RO_HUGE_FILE: u32 = 0x0008;
pub const RO_LARGE_FILE: u32 = 0x0002;

pub struct Sb {
    pub raw: Vec<u8>,
    pub inodes_count: u32,
    pub blocks_count: u64,
    pub r_blocks_count: u64,
    pub free_blocks: u64,
    pub free_inodes: u32,
    pub first_data_block: u32,
    pub block_size: u32,
    pub cluster_size: u32,
    pub blocks_per_group: u32,
    pub clusters_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: i16,
    pub state: u16,
    pub errors: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub def_resuid: u16,
    pub def_resgid: u16,
    pub first_ino: u32,
    pub inode_size: u16,
    pub block_group_nr: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    pub volume_name: String,
    pub last_mounted: String,
    pub reserved_gdt_blocks: u16,
    pub journal_inum: u32,
    pub last_orphan: u32,
    pub hash_seed: [u32; 4],
    pub def_hash_version: u8,
    pub desc_size: u16,
    pub default_mount_opts: u32,
    pub first_meta_bg: u32,
    pub mkfs_time: u32,
    pub min_extra_isize: u16,
    pub want_extra_isize: u16,
    pub flags: u32,
    pub log_groups_per_flex: u8,
    pub checksum_type: u8,
    pub kbytes_written: u64,
    pub overhead_clusters: u32,
    pub lpf_ino: u32,
    pub prj_quota_inum: u32,
    pub usr_quota_inum: u32,
    pub grp_quota_inum: u32,
    pub orphan_file_inum: u32,
    pub checksum_seed: u32,
    pub checksum: u32,
    pub group_count: u64,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

impl Sb {
    pub fn read(img: &Img) -> Result<Sb> {
        if img.len < SUPER_OFFSET + SUPER_SIZE as u64 {
            return bad(format!(
                "file is {} bytes, too short to hold a superblock at offset 1024",
                img.len
            ));
        }
        let raw = img.read_at(SUPER_OFFSET, SUPER_SIZE)?;
        let v = V::new(&raw, "superblock");

        let magic = v.u16(56)?;
        if magic != MAGIC {
            return bad(format!(
                "superblock magic is 0x{:04X}, not 0x{:04X}",
                magic, MAGIC
            ));
        }
        let log_block_size = v.u32(24)?;
        // The kernel's own limit. 2^16 is the largest block size ext4
        // defines, and anything above it is a shift that would overflow.
        if log_block_size > 6 {
            return bad(format!(
                "s_log_block_size is {}, which is a block size of 2^{} bytes",
                log_block_size,
                10 + log_block_size
            ));
        }
        let block_size = 1024u32 << log_block_size;
        let log_cluster_size = v.u32(28)?;
        if log_cluster_size > 20 {
            return bad(format!("s_log_cluster_size is {}", log_cluster_size));
        }
        let cluster_size = 1024u32 << log_cluster_size;

        let rev_level = v.u32(76)?;
        // A revision 0 filesystem has none of the dynamic fields: the inode
        // size is fixed at 128 and the first non-reserved inode at 11.
        let (first_ino, inode_size, feature_compat, feature_incompat, feature_ro_compat) =
            if rev_level == 0 {
                (11u32, 128u16, 0u32, 0u32, 0u32)
            } else {
                (v.u32(84)?, v.u16(88)?, v.u32(92)?, v.u32(96)?, v.u32(100)?)
            };
        if inode_size < 128 || !inode_size.is_power_of_two() || inode_size as u32 > block_size {
            return bad(format!("inode size {} is impossible", inode_size));
        }

        let wide = feature_incompat & INCOMPAT_64BIT != 0;
        let blocks_count = v.u32_pair(4, 336, wide)?;
        let inodes_per_group = v.u32(40)?;
        let blocks_per_group = v.u32(32)?;
        if inodes_per_group == 0 || blocks_per_group == 0 {
            return bad("inodes per group or blocks per group is zero");
        }
        let first_data_block = v.u32(20)?;
        if first_data_block as u64 >= blocks_count {
            return bad(format!(
                "first data block {} is past the end of a {}-block filesystem",
                first_data_block, blocks_count
            ));
        }
        let clusters_per_group = v.u32(36)?;
        if clusters_per_group == 0 {
            return bad("clusters per group is zero");
        }
        // Groups are counted in clusters when bigalloc is on, which is the
        // one place the two units are not interchangeable.
        let per_group = if feature_ro_compat & RO_BIGALLOC != 0 {
            clusters_per_group as u64 * (cluster_size / block_size) as u64
        } else {
            blocks_per_group as u64
        };
        let group_count =
            (blocks_count - first_data_block as u64 + per_group - 1) / per_group;

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(v.bytes(104, 16)?);
        let mut hash_seed = [0u32; 4];
        for (i, h) in hash_seed.iter_mut().enumerate() {
            *h = v.u32(236 + 4 * i)?;
        }

        let desc_size = if wide {
            let d = v.u16(254)?;
            if d < 32 || !d.is_power_of_two() || d as u32 > block_size {
                return bad(format!("group descriptor size {} is impossible", d));
            }
            d
        } else {
            32
        };

        Ok(Sb {
            inodes_count: v.u32(0)?,
            blocks_count,
            r_blocks_count: v.u32_pair(8, 340, wide)?,
            free_blocks: v.u32_pair(12, 344, wide)?,
            free_inodes: v.u32(16)?,
            first_data_block,
            block_size,
            cluster_size,
            blocks_per_group,
            clusters_per_group,
            inodes_per_group,
            mtime: v.u32(44)?,
            wtime: v.u32(48)?,
            mnt_count: v.u16(52)?,
            max_mnt_count: v.u16(54)? as i16,
            state: v.u16(58)?,
            errors: v.u16(60)?,
            lastcheck: v.u32(64)?,
            checkinterval: v.u32(68)?,
            creator_os: v.u32(72)?,
            rev_level,
            def_resuid: v.u16(80)?,
            def_resgid: v.u16(82)?,
            first_ino,
            inode_size,
            block_group_nr: v.u16(90)?,
            feature_compat,
            feature_incompat,
            feature_ro_compat,
            uuid,
            volume_name: cstr(v.bytes(120, 16)?),
            last_mounted: cstr(v.bytes(136, 64)?),
            reserved_gdt_blocks: v.u16(206)?,
            journal_inum: v.u32(224)?,
            last_orphan: v.u32(232)?,
            hash_seed,
            def_hash_version: v.u8(252)?,
            desc_size,
            default_mount_opts: v.u32(256)?,
            first_meta_bg: v.u32(260)?,
            mkfs_time: v.u32(264)?,
            min_extra_isize: v.u16(348)?,
            want_extra_isize: v.u16(350)?,
            flags: v.u32(352)?,
            log_groups_per_flex: v.u8(372)?,
            checksum_type: v.u8(373)?,
            kbytes_written: v.u64(376)?,
            overhead_clusters: v.u32(584)?,
            lpf_ino: v.u32(616)?,
            prj_quota_inum: v.u32(620)?,
            usr_quota_inum: v.u32(576)?,
            grp_quota_inum: v.u32(580)?,
            orphan_file_inum: v.u32(640)?,
            checksum_seed: v.u32(624)?,
            checksum: v.u32(1020)?,
            group_count,
            raw,
        })
    }

    pub fn has_incompat(&self, bit: u32) -> bool {
        self.feature_incompat & bit != 0
    }
    pub fn has_compat(&self, bit: u32) -> bool {
        self.feature_compat & bit != 0
    }
    pub fn has_ro(&self, bit: u32) -> bool {
        self.feature_ro_compat & bit != 0
    }
    // Group descriptors carry a checksum, and therefore meaningful
    // bg_flags, only under one of these two features. e2fsprogs tests
    // exactly this before printing either, and the corpus proves why: a
    // plain ext2 image built by mke2fs has bg_flags 4 on disk in every
    // group, which would read as "the inode table is zeroed" from a
    // reader that trusted the field. dumpe2fs prints nothing for it.
    pub fn has_group_desc_csum(&self) -> bool {
        self.has_ro(RO_GDT_CSUM) || self.has_ro(RO_METADATA_CSUM)
    }
    pub fn is_64bit(&self) -> bool {
        self.has_incompat(INCOMPAT_64BIT)
    }
    pub fn inode_blocks_per_group(&self) -> u32 {
        // Rounded up: the last inode table block is partly unused when the
        // inodes do not divide evenly into blocks.
        let per_block = self.block_size / self.inode_size as u32;
        (self.inodes_per_group + per_block - 1) / per_block
    }
    pub fn flex_bg_size(&self) -> u32 {
        1u32 << self.log_groups_per_flex
    }

    pub fn feature_names(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (bit, name) in COMPAT {
            if self.feature_compat & bit != 0 {
                out.push(*name);
            }
        }
        for (bit, name) in INCOMPAT {
            if self.feature_incompat & bit != 0 {
                out.push(*name);
            }
        }
        for (bit, name) in RO_COMPAT {
            if self.feature_ro_compat & bit != 0 {
                out.push(*name);
            }
        }
        out
    }

    pub fn uuid_string(&self) -> String {
        let u = &self.uuid;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7], u[8], u[9], u[10], u[11], u[12],
            u[13], u[14], u[15]
        )
    }

    pub fn hash_seed_string(&self) -> String {
        let b: Vec<u8> = self
            .hash_seed
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12],
            b[13], b[14], b[15]
        )
    }

    pub fn hash_version_name(&self) -> &'static str {
        match self.def_hash_version {
            0 => "legacy",
            1 => "half_md4",
            2 => "tea",
            3 => "legacy_unsigned",
            4 => "half_md4_unsigned",
            5 => "tea_unsigned",
            6 => "siphash",
            _ => "unknown",
        }
    }
}

// Names for the enumerated fields, matching what e2fsprogs prints. Same
// argument as the feature tables: if the suite compares names it compares
// the decode, and if it compares numbers it compares nothing we did not
// already read straight out of the image.
pub const MOUNT_OPTS: &[(u32, &str)] = &[
    (0x0001, "debug"),
    (0x0002, "bsdgroups"),
    (0x0004, "user_xattr"),
    (0x0008, "acl"),
    (0x0010, "uid16"),
    (0x0100, "nobarrier"),
    (0x0200, "block_validity"),
    (0x0400, "discard"),
    (0x0800, "nodelalloc"),
];

pub const FS_FLAGS: &[(u32, &str)] = &[
    (0x0001, "signed_directory_hash"),
    (0x0002, "unsigned_directory_hash"),
    (0x0004, "test_filesystem"),
];

impl Sb {
    pub fn state_name(&self) -> String {
        // e2fsprogs prints "clean" for the valid bit alone and appends a
        // comma-separated list of the other two.
        let mut parts: Vec<&str> = Vec::new();
        if self.state & 0x0001 != 0 {
            parts.push("clean");
        } else {
            parts.push("not clean");
        }
        if self.state & 0x0002 != 0 {
            parts.push("with errors");
        }
        if self.state & 0x0004 != 0 {
            parts.push("orphans being recovered");
        }
        parts.join(",")
    }

    pub fn errors_name(&self) -> &'static str {
        match self.errors {
            1 => "Continue",
            2 => "Remount read-only",
            3 => "Panic",
            _ => "Unknown",
        }
    }

    pub fn os_name(&self) -> &'static str {
        match self.creator_os {
            0 => "Linux",
            1 => "Hurd",
            2 => "Masix",
            3 => "FreeBSD",
            4 => "Lites",
            _ => "Unknown",
        }
    }

    pub fn mount_opt_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for (bit, name) in MOUNT_OPTS {
            if self.default_mount_opts & bit != 0 {
                out.push(name);
            }
        }
        // The journal mode is a two-bit field, not three flags.
        match (self.default_mount_opts >> 5) & 3 {
            1 => out.push("journal_data"),
            2 => out.push("journal_data_ordered"),
            3 => out.push("journal_data_writeback"),
            _ => {}
        }
        out
    }

    pub fn flag_names(&self) -> Vec<&'static str> {
        FS_FLAGS
            .iter()
            .filter(|(bit, _)| self.flags & bit != 0)
            .map(|(_, n)| *n)
            .collect()
    }
}
