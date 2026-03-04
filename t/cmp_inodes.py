#!/usr/bin/env python3
"""Compare `extv stat` against `debugfs -R stat` for a range of inodes.

Both sides are asked for every inode in the range in one process, which
is the difference between the suite taking a second and taking a
minute: 600 inodes against debugfs one invocation at a time costs more
than everything else here put together.

Unused inodes are compared too. They are all zeroes and agree trivially,
so they are counted separately from the in-use ones: a number dominated
by empty slots would say nothing about the decode.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])

HEAD = re.compile(r"^Inode: (\d+)\s+Type: (.+?)\s+Mode:\s+(\d+)\s+Flags: (0x[0-9a-f]+)")
GEN = re.compile(r"^Generation:\s+(\d+)\s+Version: (0x[0-9a-f]+)(?::([0-9a-f]+))?")
USER = re.compile(r"^User:\s+(\d+)\s+Group:\s+(\d+)\s+(?:Project:\s+(\d+)\s+)?Size: (\d+)")
ACL = re.compile(r"^File ACL: (\d+)")
LINKS = re.compile(r"^Links: (\d+)\s+Blockcount: (\d+)")
# debugfs prints dtime as 0x........:(........), in brackets, because
# ext4 has no i_dtime_extra: the seconds are the only fact there, so
# only the seconds are compared. The other four times have a real
# nanosecond and epoch-extension field and are compared as a pair.
TIME = re.compile(r"^\s*(a|c|m|cr|d)time: (0x[0-9a-f]+)(?::\(?([0-9a-f]+)\)?)?")
EXTRA = re.compile(r"^Size of extra inode fields: (\d+)")
CSUM = re.compile(r"^Inode checksum: (0x[0-9a-f]+)")


def debugfs_stats(img, last):
    cmds = "".join("stat <%d>\n" % n for n in range(1, last + 1))
    p = subprocess.run(["debugfs", "-f", "/dev/stdin", img],
                       input=cmds.encode(), stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    out = {}
    cur = None
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        m = HEAD.match(line)
        if m:
            cur = {"type": m.group(2), "mode": m.group(3), "flags": m.group(4)}
            out[int(m.group(1))] = cur
            continue
        if cur is None:
            continue
        m = GEN.match(line)
        if m:
            cur["generation"] = int(m.group(1))
            hi = m.group(2)
            lo = m.group(3)
            # A 128-byte inode has no high half and debugfs prints one
            # word; a large inode prints hi:lo.
            cur["version"] = ("0x%08x:%08x" % (int(hi, 16), int(lo, 16))
                              if lo is not None else "0x%08x:%08x" % (0, int(hi, 16)))
            continue
        m = USER.match(line)
        if m:
            cur["uid"] = int(m.group(1))
            cur["gid"] = int(m.group(2))
            if m.group(3) is not None:
                cur["projid"] = int(m.group(3))
            cur["size"] = int(m.group(4))
            continue
        m = ACL.match(line)
        if m:
            cur["file_acl"] = int(m.group(1))
            continue
        m = LINKS.match(line)
        if m:
            cur["links"] = int(m.group(1))
            cur["blockcount"] = int(m.group(2))
            continue
        m = TIME.match(line)
        if m:
            if m.group(1) == "d":
                cur["dtime"] = "0x%08x" % int(m.group(2), 16)
            else:
                extra = m.group(3) or "00000000"
                cur[m.group(1) + "time"] = "0x%08x:%s" % (int(m.group(2), 16),
                                                          extra)
            continue
        m = EXTRA.match(line)
        if m:
            cur["extra_isize"] = int(m.group(1))
            continue
        m = CSUM.match(line)
        if m:
            cur["checksum"] = int(m.group(1), 16)
            continue
    return out


def extv_stats(extv, img, last):
    p = subprocess.run([extv, "stat", img, "1-%d" % last],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        return None, p.stderr.decode().strip()
    out = {}
    cur = None
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        k, _, v = line.partition(" ")
        if k == "inode":
            cur = {}
            out[int(v)] = cur
        elif cur is not None:
            cur[k] = v
    return out, None


def main():
    want = debugfs_stats(IMG, LAST)
    got, err = extv_stats(EXTV, IMG, LAST)
    if got is None:
        print("FAIL %s stat: extv failed: %s" % (NAME, err))
        print("DONE")
        return
    bad = []
    checked = 0
    inuse = 0
    csums = 0
    for n in sorted(want):
        if n not in got:
            bad.append("inode %d: extv printed nothing" % n)
            continue
        w, g = want[n], got[n]
        if g.get("in_use") == "yes":
            inuse += 1
        for key, value in sorted(w.items()):
            mine = g.get(key)
            if key in ("uid", "gid", "projid", "size", "file_acl", "links",
                       "blockcount", "generation", "extra_isize"):
                mine = int(mine) if mine is not None else None
            elif key == "checksum":
                mine = int(mine, 16) if mine is not None else None
            elif key == "mode":
                value = value.lstrip("0") or "0"
                mine = (mine or "").lstrip("0") or "0"
            checked += 1
            if mine != value:
                bad.append("inode %d %s: debugfs=%r extv=%r" % (n, key, value, mine))
        # The recomputed inode checksum is only meaningful for an inode
        # that is in use: mke2fs leaves the unused ones zeroed, and a
        # checksum over zeroes is not zero.
        if "checksum_ok" in g and g.get("in_use") == "yes":
            csums += 1
            if g["checksum_ok"] != "yes":
                bad.append("inode %d: recomputed checksum %s against stored %s"
                           % (n, g.get("checksum_computed"), g.get("checksum")))

    for b in bad[:12]:
        print("FAIL %s stat %s" % (NAME, b))
    if len(bad) > 12:
        print("FAIL %s stat: and %d more" % (NAME, len(bad) - 12))
    if not bad:
        print("ok   %s stat (%d inodes, %d in use, %d fields against debugfs, "
              "%d checksums recomputed)" % (NAME, len(want), inuse, checked, csums))
    print("COUNT inode_fields %d" % checked)
    print("COUNT inodes %d" % len(want))
    print("COUNT inodes_in_use %d" % inuse)
    print("COUNT inode_checksums %d" % csums)
    print("COUNT inodes_deleted %d"
          % sum(1 for v in want.values()
                if v.get("dtime", "0x00000000") != "0x00000000"))
    print("DONE")


main()
