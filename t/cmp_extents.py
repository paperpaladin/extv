#!/usr/bin/env python3
"""Compare `extv extents` against `debugfs dump_extents`.

dump_extents prints the tree rather than the file: one row per entry in
every node, with the level, the entry's position in its node, the
logical range, the physical range, the length and the uninitialised
flag. Those are seven separate facts per row and three of them are not
stored anywhere on disk, they are computed from the neighbours, so this
is the check that the tree is being walked and not just read.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])

ROW = re.compile(
    r"^\s*(\d+)/\s*(\d+)\s+(\d+)/\s*(\d+)\s+(\d+)\s*-\s*(\d+)\s+(\d+)"
    r"(?:\s*-\s*(\d+))?\s+(\d+)\s*(\S*)\s*$")


def main():
    cmds = "".join("dump_extents <%d>\n" % n for n in range(1, LAST + 1))
    p = subprocess.run(["debugfs", "-f", "/dev/stdin", IMG],
                       input=cmds.encode(), stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    want = {}
    cur = None
    for line in p.stdout.decode("utf-8", "replace").splitlines():
        m = re.match(r"^debugfs:\s+dump_extents <(\d+)>", line)
        if m:
            cur = want.setdefault(int(m.group(1)), [])
            continue
        if cur is None or line.startswith("Level"):
            continue
        m = ROW.match(line)
        if m:
            g = m.groups()
            cur.append((int(g[0]), int(g[1]), int(g[2]), int(g[3]), int(g[4]),
                        int(g[5]), int(g[6]), int(g[7]) if g[7] else int(g[6]),
                        int(g[8]), g[9] or "-"))

    p = subprocess.run([EXTV, "extents", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s extents: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    got = {}
    cur = None
    for line in p.stdout.decode().splitlines():
        f = line.split()
        if f[0] == "inode":
            cur = got.setdefault(int(f[1]), [])
        else:
            cur.append((int(f[1]), int(f[2]), int(f[3]), int(f[4]), int(f[5]),
                        int(f[6]), int(f[7]), int(f[8]), int(f[9]), f[10]))

    bad = []
    rows = 0
    uninit = 0
    deep = 0
    for n in sorted(want):
        w, g = want[n], got.get(n, [])
        if w != g:
            bad.append("inode %d: debugfs=%r extv=%r" % (n, w[:3], g[:3]))
            continue
        rows += len(w)
        uninit += sum(1 for r in w if r[9] == "Uninit")
        deep += sum(1 for r in w if r[1] > 0)
    for b in bad[:8]:
        print("FAIL %s extents %s" % (NAME, b))
    if not bad:
        print("ok   %s extents (%d rows, %d below the root, %d uninitialised)"
              % (NAME, rows, deep, uninit))
    print("COUNT extent_rows %d" % rows)
    print("COUNT extent_rows_deep %d" % deep)
    print("COUNT extent_uninit %d" % uninit)
    print("DONE")


main()
