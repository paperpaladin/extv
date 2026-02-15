#!/usr/bin/env python3
"""Compare `extv htree` against `debugfs htree_dump`.

The valuable part is the hash of every name. extv computes it from the
name, the superblock's seed and the directory's hash version; debugfs
prints what its own implementation computes. So a hashed directory of
five hundred names is five hundred comparisons of a hash function
against somebody else's, and the tree structure is checked alongside
it.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME, LAST = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])

NAMELINE = re.compile(r"(\d+) 0x([0-9a-f]{8})-([0-9a-f]{8}) \((\d+)\) (\S+)")


# debugfs escapes names in its ls listing and prints them raw in
# htree_dump, so its output is decoded as latin-1: one character per
# byte, whatever the bytes are. extv always escapes, so its own output
# is ASCII.
def unescape(s):
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


def parse_debugfs(text):
    """Returns (root dict, [node...], [leaf...]) for one directory."""
    root = {}
    nodes = []
    leaves = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i].strip()
        m = re.match(r"^Reserved zero: (\d+)", line)
        if m:
            root["reserved_zero"] = int(m.group(1))
        m = re.match(r"^Hash Version: (\d+)", line)
        if m:
            root["hash_version"] = int(m.group(1))
        m = re.match(r"^Info length: (\d+)", line)
        if m:
            root["info_length"] = int(m.group(1))
        m = re.match(r"^Indirect levels: (\d+)", line)
        if m:
            root["indirect_levels"] = int(m.group(1))
        m = re.match(r"^Flags: (\d+)", line)
        if m:
            root["flags"] = int(m.group(1))
        m = re.match(r"^Number of entries \(count\): (\d+)", line)
        if m:
            count = int(m.group(1))
            limit = int(re.match(r"^Number of entries \(limit\): (\d+)",
                                 lines[i + 1].strip()).group(1))
            csum = None
            j = i + 2
            m2 = re.match(r"^Checksum: 0x([0-9a-f]+)", lines[j].strip())
            if m2:
                csum = int(m2.group(1), 16)
                j += 1
            ents = []
            for k in range(count):
                m3 = re.match(r"^Entry #(\d+): Hash 0x([0-9a-f]+), block (\d+)",
                              lines[j + k].strip())
                ents.append((int(m3.group(2), 16), int(m3.group(3))))
            nodes.append((count, limit, csum, ents))
            i = j + count
            continue
        m = re.match(r"^Reading directory block (\d+), phys (\d+)", line)
        if m:
            leaf = [int(m.group(1)), int(m.group(2)), []]
            j = i + 1
            while j < len(lines):
                # debugfs packs several names onto one line and breaks
                # at 80 columns, and it tests the width before printing
                # rather than after, so an entry wider than 80 columns
                # on its own emits an empty line first. The 255-byte
                # name in the corpus does exactly that whenever the
                # hash puts it at the start of a leaf. Stopping at the
                # first line that is not a name therefore stopped at a
                # blank one and read the leaf as empty.
                if lines[j].strip() == "":
                    j += 1
                    continue
                if not NAMELINE.search(lines[j]):
                    break
                for mm in NAMELINE.finditer(lines[j]):
                    leaf[2].append((int(mm.group(1)), int(mm.group(2), 16),
                                    int(mm.group(3), 16), int(mm.group(4)),
                                    unescape(mm.group(5))))
                j += 1
            leaves.append(tuple(leaf))
            i = j
            continue
        i += 1
    return root, nodes, leaves


def main():
    # Which directories are indexed: ask extv, then check every one of
    # them against debugfs. A directory extv thinks is not indexed and
    # debugfs dumps anyway is caught by the second loop below.
    p = subprocess.run([EXTV, "htree", IMG, "1-%d" % LAST],
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s htree: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    ours = {}
    cur = None
    for line in p.stdout.decode("utf-8", "surrogateescape").splitlines():
        f = line.split(" ")
        if f[0] == "htree":
            cur = {"root": {}, "nodes": [], "leaves": []}
            ours[int(f[1])] = cur
        elif f[0] == "hash":
            cur["effective_version"] = int(f[2])
        elif f[0] == "root":
            cur["root"] = dict(zip(f[1::2], (int(x) for x in f[2::2])))
        elif f[0] == "node":
            cur["nodes"].append([int(f[2]), int(f[4]),
                                 None if f[6] == "-" else int(f[6], 16), []])
        elif f[0] == "nodeentry":
            cur["nodes"][-1][3].append((int(f[2], 16), int(f[3])))
        elif f[0] == "leaf":
            cur["leaves"].append([int(f[1]), int(f[2]), []])
        elif f[0] == "name":
            parts = line.split(" ", 5)
            cur["leaves"][-1][2].append((int(parts[1]), int(parts[2], 16),
                                         int(parts[3], 16), int(parts[4]),
                                         unescape(parts[5])))

    bad = []
    nnames = 0
    nnodes = 0
    ndirs = 0
    deep = 0
    for ino in sorted(ours):
        p = subprocess.run(["debugfs", "-R", "htree_dump <%d>" % ino, IMG],
                           stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        root, nodes, leaves = parse_debugfs(p.stdout.decode("latin-1"))
        mine = ours[ino]
        if root != mine["root"]:
            bad.append("inode %d root: debugfs=%r extv=%r" % (ino, root, mine["root"]))
            continue
        if nodes != [tuple(n[:3]) + (n[3],) for n in mine["nodes"]]:
            bad.append("inode %d nodes: debugfs=%r extv=%r"
                       % (ino, nodes[:1], mine["nodes"][:1]))
            continue
        if leaves != [tuple(l[:2]) + (l[2],) for l in mine["leaves"]]:
            for a, b in zip(leaves, mine["leaves"]):
                if a != tuple(b[:2]) + (b[2],):
                    for x, y in zip(a[2], b[2]):
                        if x != y:
                            bad.append("inode %d name: debugfs=%r extv=%r"
                                       % (ino, x, y))
                            break
                    else:
                        bad.append("inode %d leaf %d: %d names from debugfs, %d from extv"
                                   % (ino, a[0], len(a[2]), len(b[2])))
                    break
            continue
        ndirs += 1
        nnodes += len(nodes)
        deep += root.get("indirect_levels", 0)
        nnames += sum(len(l[2]) for l in leaves)

    for b in bad[:6]:
        print("FAIL %s htree %s" % (NAME, b))
    if not bad:
        print("ok   %s htree (%d indexed directories, %d nodes, %d names hashed)"
              % (NAME, ndirs, nnodes, nnames))
    print("COUNT htree_dirs %d" % ndirs)
    print("COUNT htree_nodes %d" % nnodes)
    print("COUNT htree_names %d" % nnames)
    print("COUNT htree_levels %d" % deep)
    for ino in sorted(ours):
        print("COUNT htree_version_%d 1" % ours[ino].get("effective_version", -1))
    print("DONE")


main()
