#!/usr/bin/env python3
"""Compare `extv journal` against the journal section of dumpe2fs.

dumpe2fs prints seven lines about the journal superblock at the end of
its output. Six of them are fields read straight out of that block, so
they are compared one at a time. The seventh is discussed below.

An image with no journal has to produce nothing on the dumpe2fs side
and the single line "journal none" on ours, and that is checked too:
every ext2 image in the corpus is one, and a reader that printed a
journal for a filesystem that has none would otherwise go unnoticed.
"""
import re
import subprocess
import sys

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]

# dumpe2fs line -> the extv key holding the same fact.
#
# "Max transaction length" is not s_max_transaction. Setting that field
# to 1234 by hand in a scratch image left the dumpe2fs line reading
# 4096, which is the journal's length, so what dumpe2fs prints there is
# s_maxlen a second time. The comparison follows the oracle rather than
# the field name, and s_max_transaction itself has no oracle: the
# README says so.
FIELDS = {
    "Total journal blocks": "blocks",
    "Max transaction length": "blocks",
    "Fast commit length": "fast_commit_blocks",
    "Journal start": "start",
    "Journal errno": "errno",
}


def main():
    p = subprocess.run([EXTV, "journal", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    if p.returncode != 0:
        print("FAIL %s journal: extv exited %d: %s"
              % (NAME, p.returncode, p.stderr.decode().strip()))
        print("DONE")
        return
    ours = {}
    for line in p.stdout.decode().splitlines():
        k, _, v = line.partition(" ")
        ours[k] = v

    d = subprocess.run(["dumpe2fs", IMG], stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    want = {}
    for line in d.stdout.decode("latin-1").splitlines():
        m = re.match(r"^((?:Journal|Total journal|Max transaction|Fast commit)"
                     r"[^:]*):\s*(.*?)\s*$", line)
        if m and m.group(1) not in ("Journal inode", "Journal backup",
                                    "Journal size"):
            want[m.group(1)] = m.group(2)

    bad = []
    checked = 0
    if "journal" in ours and ours["journal"] == "none":
        if want:
            bad.append("extv says there is no journal, dumpe2fs printed %s"
                       % ", ".join(sorted(want)))
        else:
            print("ok   %s journal: no journal, and dumpe2fs prints none" % NAME)
        print("COUNT journal_none 1")
        print("DONE")
        return
    if not want:
        bad.append("extv read a journal superblock, dumpe2fs printed no "
                   "journal section")

    for key, ours_key in FIELDS.items():
        if key not in want:
            # Journal errno is printed only when it is not zero.
            if key == "Journal errno" and ours.get("errno") == "0":
                continue
            bad.append("dumpe2fs did not print %s" % key)
            continue
        if want[key] != ours.get(ours_key):
            bad.append("%s: dumpe2fs=%s extv=%s"
                       % (key, want[key], ours.get(ours_key)))
        checked += 1

    # Two values dumpe2fs formats rather than prints. The size is in
    # kilobytes below 8192 and in megabytes at or above it, so the
    # oracle's string is converted back to kilobytes and the numbers
    # are compared: the fact is the size, not how dumpe2fs spells it.
    size = want.get("Total journal size", "")
    kb = None
    if size.endswith("k"):
        kb = int(size[:-1])
    elif size.endswith("M"):
        kb = int(size[:-1]) * 1024
    elif size.endswith("G"):
        kb = int(size[:-1]) * 1024 * 1024
    if kb is None or kb != int(ours.get("size_k", -1)):
        bad.append("Total journal size: dumpe2fs=%s extv=%sk"
                   % (size, ours.get("size_k")))
    checked += 1
    seq = "0x%08x" % int(ours.get("sequence", "0"))
    if want.get("Journal sequence") != seq:
        bad.append("Journal sequence: dumpe2fs=%s extv=%s"
                   % (want.get("Journal sequence"), seq))
    checked += 1

    # The feature list, as a set: dumpe2fs prints them in its own order.
    theirs = want.get("Journal features", "")
    mine = ours.get("features", "")
    if set(theirs.split()) != set(mine.split()):
        bad.append("Journal features: dumpe2fs=%r extv=%r" % (theirs, mine))
    checked += 1
    nfeat = 0 if mine == "(none)" else len(mine.split())

    # A feature bit nothing here knows about would be dropped silently
    # from the list above and the comparison would still pass, because
    # dumpe2fs would print its own name for it and the sets would differ
    # only by that one name. This makes the unknown bits visible.
    for k in ("unknown_compat", "unknown_incompat", "ro_compat"):
        if ours.get(k) != "0x00000000":
            bad.append("%s is %s: a journal feature bit nothing decodes"
                       % (k, ours.get(k)))

    for b in bad[:6]:
        print("FAIL %s journal %s" % (NAME, b))
    if not bad:
        print("ok   %s journal (%d fields against dumpe2fs, %d features)"
              % (NAME, checked, nfeat))
    print("COUNT journal_fields %d" % checked)
    print("COUNT journal_images 1")
    print("COUNT journal_features %d" % nfeat)
    print("DONE")


main()
