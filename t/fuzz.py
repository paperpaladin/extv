#!/usr/bin/env python3
"""Single-bit damage in the metadata, over and over, against e2fsck.

The damaged-image cases next door are faults placed by hand, which
means they are faults somebody thought of. This flips one bit at a
time at a random offset inside the metadata region and asks two
questions of every result:

  does extv stay inside its own contract: exit 0, 1 or 2, no panic,
  no hang;

  and when extv says the image is clean, does e2fsck agree. That is
  the hard invariant, asked of damage nobody designed.

Deterministic on purpose. The seed is fixed, so the same offsets and
the same bits are hit on every run and a failure can be reproduced by
running it again. A digest of the offsets is printed, because a
seeded check that ignores its seed passes while generating something
else entirely, which METHOD records happening in derx.

A miss, where extv says clean and e2fsck does not, is counted and
named rather than tolerated silently. The count has to be zero. Three
of them were real gaps when this was first run: i_blocks never
compared against the blocks an inode owns, the inode checksum never
verified by `check`, and i_blocks compared as distinct blocks when it
counts references.
"""
import hashlib
import os
import random
import shutil
import subprocess
import sys

SCRATCH, NAME, BASE = sys.argv[1], sys.argv[2], sys.argv[3]
ROUNDS = int(sys.argv[4]) if len(sys.argv) > 4 else 80
# Fixed, and different per base, so the two layouts are not hit in the
# same places. Passing it in rather than deriving it from the clock is
# the whole point: a failure here has to be reproducible by running
# the suite again.
SEED = int(sys.argv[5]) if len(sys.argv) > 5 else 20261020
EXTV = os.environ.get("EXTV", "./target/release/extv")

# The first two megabytes of a 1024-byte-block image hold the
# superblock, the descriptors, the reserved growth blocks, the
# bitmaps and the start of the inode table. Past that is file data,
# where a flipped bit is not a filesystem fault at all and e2fsck
# rightly says nothing.
REGION = (1024, 2 * 1024 * 1024)


def e2fsck_sees(img):
    p = subprocess.run(["e2fsck", "-fn", img], stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT)
    t = p.stdout.decode("latin-1")
    return (p.returncode != 0 or "Fix? no" in t or "trying backup" in t), t


def main():
    img = os.path.join(SCRATCH, "fuzz-%s.img" % NAME)
    rnd = random.Random(SEED)
    digest = hashlib.sha256()
    verdicts = {0: 0, 1: 0, 2: 0}
    misses = []
    broke = []
    for _ in range(ROUNDS):
        shutil.copyfile(BASE, img)
        off = rnd.randrange(*REGION)
        bit = rnd.randrange(8)
        digest.update(("%d:%d\n" % (off, bit)).encode())
        with open(img, "r+b") as f:
            f.seek(off)
            b = f.read(1)[0]
            f.seek(off)
            f.write(bytes([b ^ (1 << bit)]))
        try:
            p = subprocess.run([EXTV, "check", img], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, timeout=120)
            rc = p.returncode
        except subprocess.TimeoutExpired:
            broke.append("offset %d bit %d: extv did not finish in 120 "
                         "seconds" % (off, bit))
            continue
        if rc not in verdicts:
            broke.append("offset %d bit %d: extv exited %d, which is not "
                         "clean, inconsistent or malformed: %s"
                         % (off, bit, rc, p.stderr.decode().strip()[:120]))
            continue
        verdicts[rc] += 1
        if rc == 0:
            seen, text = e2fsck_sees(img)
            if seen:
                first = next((l for l in text.splitlines()
                              if l and not l.startswith(("Pass", "e2fsck", "/"))),
                             "")
                misses.append("offset %d bit %d: extv says clean, e2fsck says "
                              "%s" % (off, bit, first.strip()))

    for b in broke[:5]:
        print("FAIL %s fuzz %s" % (NAME, b))
    for m in misses[:5]:
        print("FAIL %s fuzz %s" % (NAME, m))
    if not broke and not misses:
        print("ok   %s fuzz (%d single-bit faults: %d clean, %d inconsistent, "
              "%d refused, 0 missed)"
              % (NAME, ROUNDS, verdicts[0], verdicts[1], verdicts[2]))
    # The digest is over the offsets and bits actually generated, not
    # over the report about them, so an unwired seed changes it.
    print("ok   %s fuzz faults are the same every run (%s)"
          % (NAME, digest.hexdigest()[:16]))
    print("COUNT fuzz_faults %d" % ROUNDS)
    print("COUNT fuzz_refused %d" % verdicts[2])
    print("COUNT fuzz_inconsistent %d" % verdicts[1])
    print("COUNT fuzz_unnoticed %d" % verdicts[0])
    print("COUNT fuzz_missed %d" % len(misses))
    print("DONE")


main()
