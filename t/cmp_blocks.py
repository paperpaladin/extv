#!/usr/bin/env python3
"""Compare the block map of every inode against debugfs.

Two comparisons, both against the same walk:

  the BLOCKS or EXTENTS summary from `debugfs stat`, which carries the
  logical block numbers and names the indirect and extent tree blocks,
  against `extv map`;

  the flat list from `debugfs blocks`, which is every physical block in
  order with nothing else, against `extv blocks`.

The second is mostly implied by the first, and it is here because it is
a different code path in debugfs and because it expands the runs: a
reader that collapsed a range wrongly agrees on the summary and
disagrees on the list.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])

ENTRY = re.compile(r"\(([^)]*?)(\[u\])?\):(\d+)(?:-(\d+))?")


def debugfs(cmds):
    p = subprocess.run(["debugfs", "-f", "/dev/stdin", IMG],
                       input=cmds.encode(), stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    return p.stdout.decode("utf-8", "replace")


def parse_summary(text):
    """inode number -> list of tuples, from the stat BLOCKS/EXTENTS block.

    Also returns the set of inodes debugfs calls fast symlinks, which is
    the one place the two debugfs commands disagree with each other: a
    symlink whose target sits in the sixty bytes that would hold block
    pointers is reported by stat as a Fast link dest and by blocks as
    four block numbers made of the target's own characters.
    """
    out = {}
    fast = set()
    cur_ino = None
    collecting = False
    buf = []
    for line in text.splitlines():
        if line.startswith("Fast link dest:") and cur_ino is not None:
            fast.add(cur_ino)
            continue
        m = re.match(r"^Inode: (\d+)\s+Type:", line)
        if m:
            if cur_ino is not None:
                out[cur_ino] = parse_entries(" ".join(buf))
            cur_ino = int(m.group(1))
            collecting = False
            buf = []
            continue
        if line in ("BLOCKS:", "EXTENTS:"):
            collecting = True
            continue
        if line.startswith("TOTAL:"):
            collecting = False
            continue
        if collecting:
            buf.append(line.strip())
    if cur_ino is not None:
        out[cur_ino] = parse_entries(" ".join(buf))
    return out, fast


def parse_entries(s):
    out = []
    for m in ENTRY.finditer(s):
        what, bracket, a, b = m.group(1), m.group(2), m.group(3), m.group(4)
        bracket = "u" if bracket else "-"
        if what in ("IND", "DIND", "TIND") or what.startswith("ETB"):
            out.append(("meta", what, int(a)))
        else:
            if "-" in what:
                lo, hi = what.split("-")
            else:
                lo = hi = what
            out.append(("data", int(lo), int(hi), int(a),
                        int(b) if b is not None else int(a), bracket))
    return out


def parse_extv_map(text):
    out = {}
    summary = {}
    cur = None
    ino = None
    for line in text.splitlines():
        f = line.split()
        if f[0] == "summary":
            summary[ino] = dict(zip(f[1::2], (int(x) for x in f[2::2])))
        elif f[0] == "inode":
            cur = []
            ino = int(f[1])
            out[ino] = cur
        elif f[0] == "data":
            cur.append(("data", int(f[1]), int(f[2]), int(f[3]), int(f[4]), f[5]))
        elif f[0] == "meta":
            cur.append(("meta", f[1], int(f[2])))
    return out, summary


def parse_flat(text, numeric_only):
    """debugfs blocks prints one space-separated line per request."""
    out = {}
    cur = None
    for line in text.splitlines():
        m = re.match(r"^debugfs:\s+blocks <(\d+)>\s*(.*)$", line)
        if m:
            cur = out.setdefault(int(m.group(1)), [])
            rest = m.group(2).split()
            cur.extend(int(x) for x in rest if x.isdigit())
            continue
        if cur is not None and numeric_only:
            cur.extend(int(x) for x in line.split() if x.isdigit())
    return out


def main():
    stats = debugfs("".join("stat <%d>\n" % n for n in range(1, LAST + 1)))
    want, fast = parse_summary(stats)
    flat_want = parse_flat(debugfs("".join("blocks <%d>\n" % n
                                           for n in range(1, LAST + 1))), True)

    p = subprocess.run([EXTV, "map", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s map: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    got, summary = parse_extv_map(p.stdout.decode())

    p = subprocess.run([EXTV, "blocks", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    flat_got = {}
    cur = None
    for line in p.stdout.decode().splitlines():
        if line.startswith("inode "):
            cur = flat_got.setdefault(int(line.split()[1]), [])
        elif cur is not None and line.strip().isdigit():
            cur.append(int(line.strip()))

    bad = []
    nblocks = 0
    nentries = 0
    ninodes = 0
    nfast = 0
    # The two debugfs commands disagree on fast symlinks and stat is the
    # one that is right, so the flat list is skipped for those inodes
    # and the classification is compared instead.
    for n in sorted(set(fast) | set(k for k, v in summary.items()
                                    if v.get("fast_symlink"))):
        mine = summary.get(n, {}).get("fast_symlink", 0) == 1
        if mine != (n in fast):
            bad.append("inode %d: debugfs fast symlink=%s extv=%s"
                       % (n, n in fast, mine))
        else:
            nfast += 1
    for n in sorted(want):
        w = [e for e in want[n]]
        g = got.get(n, [])
        # debugfs marks an uninitialised extent with brackets; the
        # comparison keeps the flag on its side only, since extv reports
        # it in the summary line rather than per run.
        w_cmp, g_cmp = w, g
        if w_cmp != g_cmp:
            bad.append("inode %d map: debugfs=%r extv=%r" % (n, w_cmp[:4], g_cmp[:4]))
            continue
        if w:
            ninodes += 1
        nentries += len(w)
        if n in fast:
            continue
        fw = flat_want.get(n, [])
        fg = flat_got.get(n, [])
        if fw != fg:
            bad.append("inode %d blocks: %d from debugfs, %d from extv, first "
                       "difference at %d" % (n, len(fw), len(fg),
                                             next((i for i in range(max(len(fw), len(fg)))
                                                   if fw[i:i + 1] != fg[i:i + 1]), -1)))
            continue
        nblocks += len(fw)

    for b in bad[:10]:
        print("FAIL %s %s" % (NAME, b))
    if len(bad) > 10:
        print("FAIL %s map: and %d more" % (NAME, len(bad) - 10))
    if not bad:
        print("ok   %s map (%d inodes with blocks, %d runs, %d block numbers, "
              "%d fast symlinks)" % (NAME, ninodes, nentries, nblocks, nfast))
    print("COUNT map_runs %d" % nentries)
    print("COUNT map_block_numbers %d" % nblocks)
    print("COUNT fast_symlinks %d" % nfast)
    # What the walk actually reached on this image, summed over its
    # inodes. The driver adds these up across the corpus and refuses to
    # call a path tested when nothing reached it.
    for key in ("ind", "dind", "tind", "etb", "inline", "uninit"):
        print("COUNT path_%s %d" % (key, sum(v.get(key, 0) for v in summary.values())))
    print("DONE")


main()
