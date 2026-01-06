#!/usr/bin/env python3
"""Compare `extv ls` against `debugfs ls -l` for every directory.

Seven fields per entry: the inode number and the file type come from
the directory entry itself, the mode, owner, group and size from the
inode it names, and the name is compared as bytes rather than as text.
The date column debugfs prints is not compared here: it is the inode's
mtime, which cmp_inodes.py already checks against the raw field, and
comparing a formatted date would be comparing two calls to ctime.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])

LINE = re.compile(
    r"^\s*(\d+)\s+(\d+)\s+\((\d+)\)\s+(\d+)\s+(\d+)\s+(\d+)\s+"
    r"\S+\s+\S+\s(.*)$")


def unescape(s):
    """debugfs prints a byte outside printable ASCII as \\xNN."""
    out = bytearray()
    i = 0
    while i < len(s):
        if s[i] == "\\" and i + 3 < len(s) + 1 and s[i + 1] == "x":
            out.append(int(s[i + 2:i + 4], 16))
            i += 4
        else:
            out.append(ord(s[i]))
            i += 1
    return bytes(out)


def main():
    cmds = "".join("ls -l <%d>\n" % n for n in range(1, LAST + 1))
    p = subprocess.run(["debugfs", "-f", "/dev/stdin", IMG],
                       input=cmds.encode(), stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    want = {}
    cur = None
    for line in p.stdout.decode("utf-8", "surrogateescape").splitlines():
        m = re.match(r"^debugfs:\s+ls -l <(\d+)>", line)
        if m:
            cur = int(m.group(1))
            continue
        if cur is None:
            continue
        m = LINE.match(line)
        if m:
            g = m.groups()
            want.setdefault(cur, []).append(
                (int(g[0]), g[1], int(g[2]), int(g[3]), int(g[4]), int(g[5]),
                 unescape(g[6])))

    p = subprocess.run([EXTV, "ls", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s ls: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    got = {}
    summaries = {}
    cur = None
    for line in p.stdout.decode("utf-8", "surrogateescape").splitlines():
        # Only the entry lines may hold a name with a space in it, so
        # only they are split with a limit. The first version split
        # every line that way and silently truncated the summary into
        # the wrong keys, which turned a missing feature into four
        # failures that looked like a decode bug.
        f = line.split(" ", 7) if line.startswith("entry ") else line.split()
        if f[0] == "dir":
            cur = int(f[1])
            got[cur] = []
        elif f[0] == "summary":
            summaries[cur] = dict(zip(f[1::2], (int(x) for x in f[2::2])))
        elif f[0] == "entry":
            got[cur].append((int(f[1]), f[2], int(f[3]), int(f[4]), int(f[5]),
                             int(f[6]), unescape(f[7])))

    bad = []
    nent = 0
    ndir = 0
    for n in sorted(want):
        w, g = want[n], got.get(n)
        if g is None:
            bad.append("inode %d: debugfs lists it as a directory, extv does not"
                       % n)
            continue
        if w != g:
            # Report the first entry that differs rather than the whole
            # listing, which can be five hundred lines.
            for a, b in zip(w, g):
                if a != b:
                    bad.append("inode %d entry: debugfs=%r extv=%r" % (n, a, b))
                    break
            else:
                bad.append("inode %d: debugfs has %d entries, extv has %d"
                           % (n, len(w), len(g)))
            continue
        ndir += 1
        nent += len(w)
    for n in sorted(got):
        if n not in want:
            bad.append("inode %d: extv lists it as a directory, debugfs does not"
                       % n)

    for b in bad[:8]:
        print("FAIL %s ls %s" % (NAME, b))
    if not bad:
        print("ok   %s ls (%d directories, %d entries against debugfs)"
              % (NAME, ndir, nent))
    print("COUNT dir_entries %d" % nent)
    print("COUNT dirs %d" % ndir)
    print("COUNT dirent_tails %d"
          % sum(v.get("tails", 0) for v in summaries.values()))
    print("COUNT dirs_inline %d"
          % sum(v.get("inline", 0) for v in summaries.values()))
    print("DONE")


main()
