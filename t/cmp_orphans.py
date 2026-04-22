#!/usr/bin/env python3
"""Compare `extv orphans` against what dumpe2fs says about the list.

dumpe2fs prints two of these facts: "First orphan inode", when the
list is not empty, and "Orphan file inode" when the feature is on.
The rest of the chain and the contents of the orphan file have no
oracle outside this program, so they are reported and counted here
and the README says plainly that they are unverified.

The chain is still worth walking under the comparison, because the
head it starts from is a number dumpe2fs agrees with and the walk
either terminates at a real inode or it does not.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]


def main():
    p = subprocess.run([EXTV, "orphans", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s orphans: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    ours = {}
    summary = {}
    for line in p.stdout.decode().splitlines():
        f = line.split()
        if f[0] == "summary":
            summary = dict(zip(f[1::2], (int(x) for x in f[2::2])))
        else:
            ours[f[0]] = f[1:]

    d = subprocess.run(["dumpe2fs", "-h", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    want = {}
    for line in d.stdout.decode("latin-1").splitlines():
        m = re.match(r"^(First orphan inode|Orphan file inode):\s+(\d+)", line)
        if m:
            want[m.group(1)] = int(m.group(2))

    bad = []
    head = int(ours["last_orphan"][0])
    if want.get("First orphan inode", 0) != head:
        bad.append("first orphan inode: dumpe2fs=%s extv=%d"
                   % (want.get("First orphan inode", 0), head))
    if want.get("Orphan file inode", 0) != int(ours["orphan_file_inum"][0]):
        bad.append("orphan file inode: dumpe2fs=%s extv=%s"
                   % (want.get("Orphan file inode", 0),
                      ours["orphan_file_inum"][0]))
    # The head has to be the first link of the chain, or the walk did
    # not start where dumpe2fs says it starts.
    chain = [int(x) for x in ours["chain"]]
    if head == 0 and chain:
        bad.append("the list is empty and extv walked %d inodes" % len(chain))
    if head != 0 and (not chain or chain[0] != head):
        bad.append("the list starts at %d and extv walked %r" % (head, chain))
    if summary.get("bad_magic", 0):
        bad.append("%d orphan file blocks do not carry the orphan magic"
                   % summary["bad_magic"])

    for b in bad:
        print("FAIL %s orphans %s" % (NAME, b))
    if not bad:
        print("ok   %s orphans (head %d, chain %d, orphan file %d blocks)"
              % (NAME, head, len(chain), summary.get("file_blocks", 0)))
    print("COUNT orphan_chain %d" % len(chain))
    print("COUNT orphan_file_blocks %d" % summary.get("file_blocks", 0))
    print("COUNT orphan_lists_empty %d" % (1 if head == 0 else 0))
    print("DONE")


main()
