#!/usr/bin/env python3
"""Compare `extv bitmaps` against the free lists dumpe2fs prints.

dumpe2fs ends each group's section with "Free blocks:" and "Free
inodes:", which are the on-disk bitmaps decoded into runs. That makes
this a comparison of every bit of every bitmap in the image rather
than of a free count, and a free count is exactly the summary that
agrees while individual bits are wrong.

The run boundaries are compared as well as the membership, because the
two are the same information and a disagreement on either is a
disagreement.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]

HEAD = re.compile(r"^Group (\d+): ")
FREE = re.compile(r"^  Free (blocks|clusters|inodes): *(.*)$")


SINGLES = [0]
RANGES = [0]


def runs(text, count=False):
    """"1-3, 7, 9-11" -> [(1,3),(7,7),(9,11)]"""
    out = []
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            a, b = part.split("-", 1)
            out.append((int(a), int(b)))
            if count:
                RANGES[0] += 1
        else:
            out.append((int(part), int(part)))
            if count:
                SINGLES[0] += 1
    return out


def main():
    p = subprocess.run([EXTV, "bitmaps", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s bitmaps: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    ours = {}
    cur = None
    summary = {}
    for line in p.stdout.decode().splitlines():
        f = line.split()
        if f[0] == "group":
            cur = int(f[1])
            ours[cur] = {}
        elif f[0] == "summary":
            summary = dict(zip(f[1::2], (int(x) for x in f[2::2])))
        else:
            ours[cur][f[0]] = runs(" ".join(f[1:]).replace(" ", ","))

    d = subprocess.run(["dumpe2fs", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    want = {}
    cur = None
    for line in d.stdout.decode("latin-1").splitlines():
        m = HEAD.match(line)
        if m:
            cur = int(m.group(1))
            want[cur] = {}
            continue
        m = FREE.match(line)
        if m and cur is not None:
            key = "freeinodes" if m.group(1) == "inodes" else "freeblocks"
            want[cur][key] = runs(m.group(2), count=True)

    bad = []
    nblocks = 0
    ninodes = 0
    ngroups = 0
    nruns = 0
    for g in sorted(want):
        if g not in ours:
            bad.append("group %d: dumpe2fs has it, extv does not" % g)
            continue
        for key in ("freeblocks", "freeinodes"):
            w = want[g].get(key, [])
            o = ours[g].get(key, [])
            if w != o:
                # Name the first run that differs rather than printing
                # a thousand numbers.
                for a, b in zip(w, o):
                    if a != b:
                        bad.append("group %d %s: dumpe2fs=%s extv=%s"
                                   % (g, key, a, b))
                        break
                else:
                    bad.append("group %d %s: dumpe2fs has %d runs, extv %d"
                               % (g, key, len(w), len(o)))
                continue
            nruns += len(w)
            n = sum(b - a + 1 for a, b in w)
            if key == "freeblocks":
                nblocks += n
            else:
                ninodes += n
        ngroups += 1
    for g in sorted(ours):
        if g not in want:
            bad.append("group %d: extv has it, dumpe2fs does not" % g)

    for b in bad[:6]:
        print("FAIL %s bitmaps %s" % (NAME, b))
    if not bad:
        print("ok   %s bitmaps (%d groups, %d free runs against dumpe2fs)"
              % (NAME, ngroups, nruns))
    print("COUNT bitmap_groups %d" % ngroups)
    print("COUNT bitmap_free_runs %d" % nruns)
    print("COUNT bitmap_blocks_in_use %d" % summary.get("blocks_in_use", 0))
    # Both spellings dumpe2fs uses for a run, and the two kinds of
    # uninitialised group, counted so that the suite can insist the
    # corpus reaches each of them.
    print("COUNT bitmap_single_runs %d" % SINGLES[0])
    print("COUNT bitmap_range_runs %d" % RANGES[0])
    print("COUNT bitmap_block_uninit %d" % summary.get("block_uninit", 0))
    print("COUNT bitmap_inode_uninit %d" % summary.get("inode_uninit", 0))
    print("DONE")


main()
