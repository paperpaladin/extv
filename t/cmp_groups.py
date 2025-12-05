#!/usr/bin/env python3
"""Compare `extv groups` against the per-group sections of dumpe2fs.

One image has many groups and each group has around nine fields that
dumpe2fs prints, so this is the broadest comparison in the suite: a
thousand values from one 256 MB image, not one.

Only the fields dumpe2fs actually prints are compared, and which those
are depends on the features: without metadata_csum or uninit_bg there is
no group checksum, no bitmap checksums and no unused inode count, so
those images contribute fewer comparisons rather than a false agreement
against a missing value.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]

G_HEAD = re.compile(
    r"^Group (\d+): \(Blocks (\d+)-(\d+)\)(?: csum (0x[0-9a-fA-F]+))?"
    r"(?: \[([^\]]*)\])?")
G_SUPER = re.compile(r"^  (Primary|Backup) superblock at (\d+)")
G_BBITMAP = re.compile(r"^  Block bitmap at (\d+)[^,]*(?:, csum (0x[0-9a-fA-F]+))?")
G_IBITMAP = re.compile(r"^  Inode bitmap at (\d+)[^,]*(?:, csum (0x[0-9a-fA-F]+))?")
G_ITABLE = re.compile(r"^  Inode table at (\d+)")
G_COUNTS = re.compile(
    r"^  (\d+) free (?:blocks|clusters), (\d+) free inodes, (\d+) directories"
    r"(?:, (\d+) unused inodes)?")


def parse_dumpe2fs(text):
    groups = {}
    cur = None
    for line in text.splitlines():
        m = G_HEAD.match(line)
        if m:
            cur = {
                "first_block": int(m.group(2)),
                "last_block": int(m.group(3)),
                "has_super": 0,
            }
            if m.group(4):
                cur["checksum"] = int(m.group(4), 16)
            if m.group(5) is not None:
                cur["flags"] = set(x.strip() for x in m.group(5).split(",") if x.strip())
            else:
                cur["flags"] = set()
            groups[int(m.group(1))] = cur
            continue
        if cur is None:
            continue
        m = G_SUPER.match(line)
        if m:
            cur["has_super"] = 1
            continue
        m = G_BBITMAP.match(line)
        if m:
            cur["block_bitmap"] = int(m.group(1))
            if m.group(2):
                cur["block_bitmap_csum"] = int(m.group(2), 16)
            continue
        m = G_IBITMAP.match(line)
        if m:
            cur["inode_bitmap"] = int(m.group(1))
            if m.group(2):
                cur["inode_bitmap_csum"] = int(m.group(2), 16)
            continue
        m = G_ITABLE.match(line)
        if m:
            cur["inode_table"] = int(m.group(1))
            continue
        m = G_COUNTS.match(line)
        if m:
            cur["free_blocks"] = int(m.group(1))
            cur["free_inodes"] = int(m.group(2))
            cur["used_dirs"] = int(m.group(3))
            if m.group(4) is not None:
                cur["unused_inodes"] = int(m.group(4))
            continue
    return groups


def main():
    p = subprocess.run(["dumpe2fs", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    if p.returncode != 0:
        print("FAIL %s groups: dumpe2fs failed (exit %d)" % (NAME, p.returncode))
        print("DONE")
        return
    want = parse_dumpe2fs(p.stdout.decode("utf-8", "replace"))

    p = subprocess.run([EXTV, "groups", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s groups: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    got = {}
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        parts = line.split(" ", 3)
        if len(parts) < 4 or parts[0] != "group":
            print("FAIL %s groups: cannot parse extv line %r" % (NAME, line))
            continue
        got.setdefault(int(parts[1]), {})[parts[2]] = parts[3]

    bad = []
    checked = 0
    if sorted(want) != sorted(got):
        print("FAIL %s groups: dumpe2fs lists %d groups, extv lists %d"
              % (NAME, len(want), len(got)))
        print("DONE")
        return
    for g in sorted(want):
        ours = got[g]
        for key, value in sorted(want[g].items()):
            if key == "flags":
                mine = set(x for x in ours.get("flags", "").split(",") if x)
            elif key in ("checksum", "block_bitmap_csum", "inode_bitmap_csum"):
                mine = int(ours.get(key, "0x0"), 16)
            else:
                mine = int(ours.get(key, "-1"))
            checked += 1
            if mine != value:
                bad.append("group %d %s: dumpe2fs=%r extv=%r" % (g, key, value, mine))

    for b in bad[:12]:
        print("FAIL %s groups %s" % (NAME, b))
    if len(bad) > 12:
        print("FAIL %s groups: and %d more" % (NAME, len(bad) - 12))
    if not bad:
        print("ok   %s groups (%d groups, %d fields against dumpe2fs)"
              % (NAME, len(want), checked))
    print("COUNT group_fields %d" % checked)
    print("COUNT groups %d" % len(want))
    print("DONE")


main()
