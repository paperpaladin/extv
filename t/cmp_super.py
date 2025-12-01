#!/usr/bin/env python3
"""Compare `extv super` against `dumpe2fs -h`, field by field.

Prints a protocol the shell driver understands:

  ok   TEXT          one assertion passed
  FAIL TEXT          one assertion failed
  COUNT key n        add n to the named running total
  DONE               the script reached its end

DONE matters. A helper that dies halfway through prints neither ok nor
FAIL for the rest of its work, and without it the driver would read the
silence as nothing to report.

The comparison is on extracted values, never on text: the two programs
share no output format, and diffing them would measure formatting.
"""
import re
import subprocess
import sys
import time

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]


def run(cmd):
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p.returncode, p.stdout.decode("utf-8", "replace")


def ctime_epoch(s):
    """dumpe2fs prints times with ctime(3), in the local zone."""
    if s in ("n/a", "<none>"):
        return 0
    return int(time.mktime(time.strptime(" ".join(s.split()), "%a %b %d %H:%M:%S %Y")))


def first_word(s):
    return s.split()[0] if s.split() else ""


def num(s):
    return int(first_word(s))


def hexnum(s):
    return int(first_word(s), 16)


def lifetime_kb(s):
    return (int(s.split()[0]), s.split()[1])


def ours_lifetime(kb):
    # dumpe2fs switches unit at 8192 kB and rounds to nearest rather than
    # truncating, which is worth copying exactly: the first version here
    # shifted right by ten and reported 11 MB against dumpe2fs's 12, a
    # disagreement about printf and not about the superblock.
    kb = int(kb)
    if kb < 1 << 13:
        return (kb, "kB")
    if kb < 1 << 23:
        return ((kb + (1 << 9)) >> 10, "MB")
    if kb < 1 << 33:
        return ((kb + (1 << 19)) >> 20, "GB")
    return ((kb + (1 << 29)) >> 30, "TB")


def nameset(s):
    return " ".join(sorted(s.split()))


# dumpe2fs field -> (extv key, how to compare). The converter takes the
# dumpe2fs string and the whole extv dictionary and returns a pair to
# compare, so a field whose extv spelling needs assembling can do it here.
FIELDS = {
    "Filesystem volume name": ("volume_name", lambda s, e: (
        "" if s == "<none>" else s, e["volume_name"])),
    "Last mounted on": ("last_mounted", lambda s, e: (
        "" if s == "<not available>" else s, e["last_mounted"])),
    "Filesystem UUID": ("uuid", lambda s, e: (s, e["uuid"])),
    "Filesystem magic number": ("magic", lambda s, e: (
        hexnum(s), hexnum(e["magic"]))),
    "Filesystem revision #": ("rev_level", lambda s, e: (
        num(s), int(e["rev_level"]))),
    "Filesystem features": ("features", lambda s, e: (
        nameset(s), nameset(e["features"]))),
    "Filesystem flags": ("flag_names", lambda s, e: (
        nameset(s), nameset(e["flag_names"]))),
    "Default mount options": ("mount_opts", lambda s, e: (
        nameset(s if s != "(none)" else ""), nameset(e["mount_opts"]))),
    "Filesystem state": ("state_name", lambda s, e: (s, e["state_name"])),
    "Errors behavior": ("errors_name", lambda s, e: (s, e["errors_name"])),
    "Filesystem OS type": ("os_name", lambda s, e: (s, e["os_name"])),
    "Inode count": ("inode_count", lambda s, e: (num(s), int(e["inode_count"]))),
    "Block count": ("block_count", lambda s, e: (num(s), int(e["block_count"]))),
    "Reserved block count": ("r_blocks_count", lambda s, e: (
        num(s), int(e["r_blocks_count"]))),
    "Overhead clusters": ("overhead_clusters", lambda s, e: (
        num(s), int(e["overhead_clusters"]))),
    "Free blocks": ("free_blocks", lambda s, e: (num(s), int(e["free_blocks"]))),
    "Free inodes": ("free_inodes", lambda s, e: (num(s), int(e["free_inodes"]))),
    "First block": ("first_data_block", lambda s, e: (
        num(s), int(e["first_data_block"]))),
    "Block size": ("block_size", lambda s, e: (num(s), int(e["block_size"]))),
    "Fragment size": ("cluster_size", lambda s, e: (num(s), int(e["cluster_size"]))),
    "Cluster size": ("cluster_size", lambda s, e: (num(s), int(e["cluster_size"]))),
    "Group descriptor size": ("desc_size", lambda s, e: (num(s), int(e["desc_size"]))),
    "Reserved GDT blocks": ("reserved_gdt_blocks", lambda s, e: (
        num(s), int(e["reserved_gdt_blocks"]))),
    "Blocks per group": ("blocks_per_group", lambda s, e: (
        num(s), int(e["blocks_per_group"]))),
    "Fragments per group": ("clusters_per_group", lambda s, e: (
        num(s), int(e["clusters_per_group"]))),
    "Clusters per group": ("clusters_per_group", lambda s, e: (
        num(s), int(e["clusters_per_group"]))),
    "Inodes per group": ("inodes_per_group", lambda s, e: (
        num(s), int(e["inodes_per_group"]))),
    "Inode blocks per group": ("inode_blocks_per_group", lambda s, e: (
        num(s), int(e["inode_blocks_per_group"]))),
    "Flex block group size": ("flex_bg_size", lambda s, e: (
        num(s), int(e["flex_bg_size"]))),
    "Filesystem created": ("mkfs_time", lambda s, e: (
        ctime_epoch(s), int(e["mkfs_time"]))),
    "Last mount time": ("mtime", lambda s, e: (ctime_epoch(s), int(e["mtime"]))),
    "Last write time": ("wtime", lambda s, e: (ctime_epoch(s), int(e["wtime"]))),
    "Mount count": ("mnt_count", lambda s, e: (num(s), int(e["mnt_count"]))),
    "Maximum mount count": ("max_mnt_count", lambda s, e: (
        int(first_word(s)), int(e["max_mnt_count"]))),
    "Last checked": ("lastcheck", lambda s, e: (ctime_epoch(s), int(e["lastcheck"]))),
    "Check interval": ("checkinterval", lambda s, e: (
        num(s), int(e["checkinterval"]))),
    "Lifetime writes": ("kbytes_written", lambda s, e: (
        lifetime_kb(s), ours_lifetime(e["kbytes_written"]))),
    "Reserved blocks uid": ("def_resuid", lambda s, e: (num(s), int(e["def_resuid"]))),
    "Reserved blocks gid": ("def_resgid", lambda s, e: (num(s), int(e["def_resgid"]))),
    "First inode": ("first_ino", lambda s, e: (num(s), int(e["first_ino"]))),
    "Inode size": ("inode_size", lambda s, e: (num(s), int(e["inode_size"]))),
    "Required extra isize": ("min_extra_isize", lambda s, e: (
        num(s), int(e["min_extra_isize"]))),
    "Desired extra isize": ("want_extra_isize", lambda s, e: (
        num(s), int(e["want_extra_isize"]))),
    "Journal inode": ("journal_inum", lambda s, e: (num(s), int(e["journal_inum"]))),
    "First orphan inode": ("last_orphan", lambda s, e: (num(s), int(e["last_orphan"]))),
    "Orphan file inode": ("orphan_file_inum", lambda s, e: (
        num(s), int(e["orphan_file_inum"]))),
    "Default directory hash": ("hash_version", lambda s, e: (s, e["hash_version"])),
    "Directory Hash Seed": ("hash_seed", lambda s, e: (s, e["hash_seed"])),
    "Checksum type": ("checksum_type", lambda s, e: (
        {"crc32c": 1}.get(s, -1), int(e["checksum_type"]))),
    "Checksum seed": ("checksum_seed", lambda s, e: (
        hexnum(s), hexnum(e["checksum_seed"]))),
    "Checksum": ("checksum", lambda s, e: (hexnum(s), hexnum(e["checksum"]))),
    "Lost found inode": ("lpf_ino", lambda s, e: (num(s), int(e["lpf_ino"]))),
    "User quota inode": ("usr_quota_inum", lambda s, e: (
        num(s), int(e["usr_quota_inum"]))),
    "Group quota inode": ("grp_quota_inum", lambda s, e: (
        num(s), int(e["grp_quota_inum"]))),
    "Project quota inode": ("prj_quota_inum", lambda s, e: (
        num(s), int(e["prj_quota_inum"]))),
}

# Fields dumpe2fs prints that nothing here compares, with the reason. The
# set is asserted, not ignored: a field that appears in a new e2fsprogs
# and is quietly dropped would never be noticed otherwise.
UNCHECKED = {
    "Journal backup": "a copy of the journal inode's block map, not a field",
    "Journal features": "journal superblock, compared by the journal checks",
    "Total journal size": "journal superblock",
    "Total journal blocks": "journal superblock",
    "Max transaction length": "journal superblock",
    "Fast commit length": "journal superblock",
    "Journal sequence": "journal superblock",
    "Journal start": "journal superblock",
    "Journal errno": "journal superblock",
    "Journal size": "derived from the journal inode's size",
    "Overhead blocks": "older spelling of Overhead clusters",
    "FS Error count": "error history, not read",
    "First error time": "error history, not read",
    "First error function": "error history, not read",
    "First error line #": "error history, not read",
    "First error inode #": "error history, not read",
    "First error block #": "error history, not read",
    "First error err": "error history, not read",
    "Last error time": "error history, not read",
    "Last error function": "error history, not read",
    "Last error line #": "error history, not read",
    "Last error inode #": "error history, not read",
    "Last error block #": "error history, not read",
    "Last error err": "error history, not read",
    "Default mount opts": "older spelling",
    "Reserved blocks": "not printed by this version",
    "Filesystem created on": "older spelling",
}


def main():
    rc, out = run(["dumpe2fs", "-h", IMG])
    if rc != 0:
        print("FAIL %s super: dumpe2fs -h failed (exit %d)" % (NAME, rc))
        print("DONE")
        return
    want = {}
    for line in out.splitlines():
        m = re.match(r"^([A-Za-z][^:]*):\s*(.*?)\s*$", line)
        if m:
            want[m.group(1)] = m.group(2)

    rc, out = run([EXTV, "super", IMG])
    if rc != 0:
        print("FAIL %s super: extv exited %d" % (NAME, rc))
        print("DONE")
        return
    got = {}
    for line in out.splitlines():
        k, _, v = line.partition(" ")
        got[k] = v

    checked = 0
    bad = []
    unknown = []
    for key, value in want.items():
        if key in FIELDS:
            ekey, conv = FIELDS[key]
            if ekey not in got:
                bad.append("%s: extv printed no %s" % (key, ekey))
                continue
            try:
                a, b = conv(value, got)
            except Exception as exc:  # a parse failure is a failure
                bad.append("%s: cannot compare %r (%s)" % (key, value, exc))
                continue
            checked += 1
            if a != b:
                bad.append("%s: dumpe2fs=%r extv=%r" % (key, a, b))
        elif key not in UNCHECKED:
            unknown.append(key)

    for b in bad:
        print("FAIL %s super %s" % (NAME, b))
    if not bad:
        print("ok   %s super (%d fields against dumpe2fs)" % (NAME, checked))
    if unknown:
        print("FAIL %s super: dumpe2fs printed fields nothing compares: %s"
              % (NAME, ", ".join(unknown)))
    else:
        print("ok   %s super: every dumpe2fs field is compared or listed as unchecked"
              % NAME)
    print("COUNT super_fields %d" % checked)
    print("DONE")


main()
