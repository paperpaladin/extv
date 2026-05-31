#!/usr/bin/env python3
"""Damaged images, and the rule that extv has to refuse rather than guess.

Each case takes a clean image, breaks one thing in it on purpose, and
says what extv must do. Two outcomes count as right and they are
counted apart from each other:

  refuse        exit 2, one line on stderr naming what is wrong, and
                nothing on stdout. A refusal is the answer when the
                image says something impossible.
  inconsistent  exit 1 from `extv check`. The image can be read and
                what it says does not add up.

Anything else is a wrong answer: a clean verdict on a broken image, a
crash, a hang, or a refusal with facts already printed.

e2fsck is the outside authority on every case, and its verdict is
recorded beside ours. METHOD's point about refusals is that a program
can refuse valid input and look rigorous doing it, so a case where
e2fsck is happy and extv refuses is marked in the table with the
reason, rather than passing quietly.
"""
import os
import re
import shutil
import subprocess
import sys

SCRATCH, NAME = sys.argv[1], sys.argv[2]
BASES = sys.argv[3:]

passed = []
failed = []
refusals = 0
inconsistent = 0


def sh(*args):
    return subprocess.run(args, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL).stdout.decode("latin-1")


def blocksize(img):
    m = re.search(r"^Block size:\s+(\d+)", sh("dumpe2fs", "-h", img), re.M)
    return int(m.group(1))


def inode_offset(img, num):
    """Byte offset of one inode, from debugfs imap."""
    out = sh("debugfs", "-R", "imap <%d>" % num, img)
    m = re.search(r"located at block (\d+), offset (0x[0-9a-f]+)", out)
    if not m:
        raise SystemExit("corrupt.py: cannot locate inode %d in %s" % (num, img))
    return int(m.group(1)) * blocksize(img) + int(m.group(2), 16)


def ino_of(img, path):
    out = sh("debugfs", "-R", "stat %s" % path, img)
    m = re.search(r"^Inode: (\d+)", out, re.M)
    if not m:
        raise SystemExit("corrupt.py: no inode for %s in %s" % (path, img))
    return int(m.group(1))


def has_extents(img):
    return "extent" in sh("dumpe2fs", "-h", img)


def u16le(n):
    return bytes([n & 0xff, (n >> 8) & 0xff])


def u32le(n):
    return bytes([n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff, (n >> 24) & 0xff])


def rd32(img, off):
    b = peek(img, off, 4)
    return b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24)


def bmap(img, num, logical=0):
    out = sh("debugfs", "-R", "bmap <%d> %d" % (num, logical), img).strip()
    return int(out.split()[-1])


def poke(img, off, data):
    with open(img, "r+b") as f:
        f.seek(off)
        f.write(data)


def peek(img, off, n):
    with open(img, "rb") as f:
        f.seek(off)
        return f.read(n)


def oracle_saw(img):
    """Did an outside authority notice the damage, and which one?

    Exit status alone is not enough. e2fsck recovers from a broken
    primary superblock or a broken descriptor table by reading the
    backups, checks the filesystem through those, finds nothing else
    wrong and exits 0. The image is still damaged and e2fsck says so
    in words: "Superblock invalid, trying backup blocks" or "Group
    descriptors look bad... trying backup blocks". extv has no backup
    fallback, so it refuses, and the fallback message is the authority
    that says the refusal is not extv rejecting a healthy image.

    dumpe2fs is the third voice: it fails outright on a superblock it
    cannot parse.
    """
    p = subprocess.run(["e2fsck", "-fn", img], stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT)
    text = p.stdout.decode("latin-1")
    if p.returncode != 0:
        return "e2fsck exit %d" % p.returncode
    if "trying backup blocks" in text:
        return "e2fsck fell back to the backups"
    d = subprocess.run(["dumpe2fs", "-h", img], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    if d.returncode != 0:
        return "dumpe2fs exit %d" % d.returncode
    return None


def case(label, base, breakit, cmd, want, e2fsck_sees=True, why="", msg=None):
    """Run one damaged-image case. cmd is the extv argv after the image."""
    global refusals, inconsistent
    # The same damage behaves differently on different layouts, so the
    # base is part of the case's name.
    label = "%s [%s]" % (label, os.path.basename(base).replace(".img", ""))
    img = os.path.join(SCRATCH, "corrupt.img")
    shutil.copyfile(base, img)
    try:
        breakit(img)
    except SystemExit:
        raise
    except Exception as e:
        failed.append("%s: could not break the image: %s" % (label, e))
        return
    # extv takes the image as its second argument, before any inode
    # or path. The first version of this appended the image after the
    # argument and every case reported "no such file" as a refusal
    # with the wrong exit code.
    p = subprocess.run([EXTV, cmd[0], img] + cmd[1:], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, timeout=60)
    out = p.stdout.decode("latin-1")
    err = p.stderr.decode("latin-1").strip()
    saw = oracle_saw(img)

    if want == "refuse":
        if p.returncode != 2:
            failed.append("%s: extv exited %d, wanted 2 (refusal)%s"
                          % (label, p.returncode,
                             "; said " + repr(err[:120]) if err else ""))
            return
        if out != "":
            failed.append("%s: refused and still printed %d bytes of facts"
                          % (label, len(out)))
            return
        if not err.startswith("extv: malformed image:"):
            failed.append("%s: refused without saying the image is malformed: %r"
                          % (label, err[:120]))
            return
        # The reason matters, not just the refusal. Deleting the rule
        # that an extent node's depth must fall by one left the case
        # passing, because the child was then read as an interior
        # node and the grandchild's magic failed instead. The refusal
        # was real and it was not the one being tested.
        if msg is not None and msg not in err:
            failed.append("%s: refused for the wrong reason: wanted %r, said %r"
                          % (label, msg, err[:160]))
            return
        refusals += 1
    elif want == "inconsistent":
        if p.returncode != 1:
            failed.append("%s: extv exited %d, wanted 1 (inconsistent)%s"
                          % (label, p.returncode,
                             "; said " + repr(err[:120]) if err else ""))
            return
        if "verdict inconsistent" not in out:
            failed.append("%s: exited 1 without an inconsistent verdict" % label)
            return
        inconsistent += 1
    else:
        raise SystemExit("corrupt.py: unknown expectation %r" % want)

    if e2fsck_sees and saw is None:
        failed.append("%s: extv says %s and no oracle sees anything wrong"
                      % (label, want))
        return
    if not e2fsck_sees and saw is not None:
        failed.append("%s: marked as damage no oracle sees, and %s"
                      % (label, saw))
        return
    passed.append("%s (%s, %s)" % (label, want, saw if saw else why))


# ---------------------------------------------------------------- cases

def truncate_to(n):
    def f(img):
        with open(img, "r+b") as fh:
            fh.truncate(n)
    return f


def superblock_cases(base):
    # The magic number. Nothing can be located without it and guessing
    # the layout from the rest of the superblock is exactly the guess
    # this program does not make.
    case("superblock magic zeroed", base,
         lambda img: poke(img, 1024 + 56, b"\x00\x00"), ["super"], "refuse",
         msg="superblock magic")
    case("superblock magic wrong", base,
         lambda img: poke(img, 1024 + 56, b"\x54\xef"), ["super"], "refuse")
    # s_log_block_size of 0xff asks for a block of 2**(10+255) bytes.
    case("absurd block size", base,
         lambda img: poke(img, 1024 + 24, b"\xff\x00\x00\x00"), ["super"],
         "refuse")
    # Zero inodes per group is a division by zero in every reader that
    # locates an inode.
    case("inodes per group zero", base,
         lambda img: poke(img, 1024 + 40, b"\x00\x00\x00\x00"), ["stat", "2"],
         "refuse")
    case("blocks per group zero", base,
         lambda img: poke(img, 1024 + 32, b"\x00\x00\x00\x00"), ["groups"],
         "refuse")
    # A file that stops before the superblock, and one that stops in
    # the middle of the group descriptors.
    case("image truncated to 512 bytes", base, truncate_to(512), ["super"],
         "refuse")
    def cut_in_gdt(img):
        bs = blocksize(img)
        first = 1 if bs == 1024 else 0
        truncate_to((first + 1) * bs + bs // 2)(img)
    case("image stops inside the group descriptors", base, cut_in_gdt,
         ["groups"], "refuse")
    # Garbage over the whole group descriptor table.
    def wreck_gdt(img):
        bs = blocksize(img)
        first = 1 if bs == 1024 else 0
        poke(img, (first + 1) * bs, b"\xff" * bs)
    case("group descriptors are 0xff", base, wreck_gdt, ["groups"], "refuse",
         msg="block bitmap at block")


# The extent tree is where a hostile image gets a reader to recurse,
# so these are the cases the recursion has to survive. i_block starts
# 40 bytes into the inode and holds a 12-byte header followed by four
# 12-byte entries.
EXT_HEADER = 40
EXT_MAGIC_OFF = 0
EXT_ENTRIES_OFF = 2
EXT_MAX_OFF = 4
EXT_DEPTH_OFF = 6


def extent_cases(base):
    if not has_extents(base):
        return
    flat = ino_of(base, "/b4096")
    deep = ino_of(base, "/chunks")
    root = inode_offset(base, flat) + EXT_HEADER
    droot = inode_offset(base, deep) + EXT_HEADER

    case("extent magic wrong", base,
         lambda img: poke(img, root + EXT_MAGIC_OFF, u16le(0x1234)),
         ["extents", str(flat)], "refuse", msg="extent node magic")
    case("extent depth of 9", base,
         lambda img: poke(img, root + EXT_DEPTH_OFF, u16le(9)),
         ["extents", str(flat)], "refuse", msg="the format allows 5")
    case("extent entry count above the maximum", base,
         lambda img: poke(img, root + EXT_ENTRIES_OFF, u16le(400)),
         ["extents", str(flat)], "refuse")
    case("extent node claims room for 9999 entries", base,
         lambda img: poke(img, root + EXT_MAX_OFF, u16le(9999)),
         ["extents", str(flat)], "refuse")

    # Depth that does not fall. The root of /chunks has depth 1 and
    # its first index points at a node that must have depth 0.
    def child_keeps_depth(img):
        child = rd32(img, droot + 12 + 4)
        poke(img, child * blocksize(img) + EXT_DEPTH_OFF, u16le(1))
    case("interior node whose child has the same depth", base,
         child_keeps_depth, ["extents", str(deep)], "refuse",
         msg="under a node of depth")

    # An index pointing at the block it lives in. With the depth rule
    # in place this is caught by the depth, which is the point: the
    # recursion cannot be made to run away without first making the
    # depth wrong.
    def self_pointing(img):
        child = rd32(img, droot + 12 + 4)
        poke(img, child * blocksize(img) + 12 + 4, u32le(child))
        poke(img, child * blocksize(img) + EXT_DEPTH_OFF, u16le(1))
    case("interior node that points at itself", base, self_pointing,
         ["extents", str(deep)], "refuse", msg="under a node of depth")

    # A leaf extent whose physical start is past the end of the
    # filesystem. Reading it would read somebody else's file, or
    # nothing at all.
    def off_the_end(img):
        poke(img, root + 12 + 8, u32le(0xfffffff0))
    case("leaf extent past the end of the filesystem", base, off_the_end,
         ["blocks", str(flat)], "refuse")


def main():
    for base in BASES:
        superblock_cases(base)
        extent_cases(base)
    for p in passed:
        print("ok   %s corrupt %s" % (NAME, p))
    for f in failed:
        print("FAIL %s corrupt %s" % (NAME, f))
    print("COUNT corrupt_refusals %d" % refusals)
    print("COUNT corrupt_inconsistent %d" % inconsistent)
    print("COUNT corrupt_cases %d" % (len(passed) + len(failed)))
    # Wrong answers are counted apart from refusals on purpose. A
    # refusal is a result; a wrong answer is a defect, and a run that
    # reported them together would let one hide inside the other.
    print("COUNT corrupt_wrong_answers %d" % len(failed))
    print("DONE")


EXTV = os.environ.get("EXTV", "./target/release/extv")
main()
