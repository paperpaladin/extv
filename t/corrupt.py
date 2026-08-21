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
# Cases where extv objects and no outside authority does. Each one is
# a deliberate difference with a reason written beside it, and the
# count is printed so that the number cannot grow unnoticed.
silent = 0
# Paths a damaged image reaches that no clean one does, counted so the
# suite can insist they were reached at all.
tags = {}


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
    # e2fsck prints "Fix? no" for every problem it finds under -n, and
    # for some of them it still exits 0. Changing the superblock's
    # free inode count to 123456 gets "Free inodes count wrong
    # (123456, counted=15826). Fix? no" and an exit status of zero, so
    # the exit status alone called the damaged image clean.
    if "Fix? no" in text:
        return "e2fsck reported a problem and exited %d" % p.returncode
    d = subprocess.run(["dumpe2fs", "-h", img], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    if d.returncode != 0:
        return "dumpe2fs exit %d" % d.returncode
    return None


def case(label, base, breakit, cmd, want, e2fsck_sees=True, why="", msg=None,
         tag=None):
    """Run one damaged-image case. cmd is the extv argv after the image."""
    global refusals, inconsistent, silent
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
        # For an inconsistency the reason is on stdout rather than on
        # stderr, and it is checked for the same reason: an image can
        # be inconsistent for more than one thing at a time.
        if msg is not None and msg not in out:
            failed.append("%s: inconsistent for the wrong reason: wanted %r, "
                          "said %r" % (label, msg, out[:200]))
            return
        inconsistent += 1
    else:
        raise SystemExit("corrupt.py: unknown expectation %r" % want)

    if e2fsck_sees and saw is None:
        failed.append("%s: extv says %s and no oracle sees anything wrong"
                      % (label, want))
        return
    if not e2fsck_sees:
        silent += 1
    if not e2fsck_sees and saw is not None:
        failed.append("%s: marked as damage no oracle sees, and %s"
                      % (label, saw))
        return
    passed.append("%s (%s, %s)" % (label, want, saw if saw else why))
    if tag:
        tags[tag] = tags.get(tag, 0) + 1


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


# A directory block is a chain of entries, each saying how long it is.
# The classic hang is a rec_len of zero, where the walk never
# advances; the classic over-read is a name that claims to be longer
# than the entry holding it.
def directory_cases(base):
    rootblk = bmap(base, 2) * blocksize(base)
    bs = blocksize(base)
    # Entry 0 is "." with rec_len 12, entry 1 is ".." and runs to the
    # end of the block, so offset 12 is the one to damage.
    dd = rootblk + 12

    case("directory entry with rec_len 0", base,
         lambda img: poke(img, dd + 4, u16le(0)), ["ls", "2"], "refuse",
         msg="which cannot hold a header")
    case("directory entry running past the end of its block", base,
         lambda img: poke(img, dd + 4, u16le(bs)), ["ls", "2"], "refuse",
         msg="runs past the end of the block")
    case("directory entry whose length is not a multiple of 4", base,
         lambda img: poke(img, dd + 4, u16le(13)), ["ls", "2"], "refuse",
         msg="not a multiple of 4")
    case("directory entry whose name overruns it", base,
         lambda img: poke(img, dd + 6, bytes([255])), ["ls", "2"], "refuse",
         msg="in a 12-byte entry" if False else "-byte name")
    case("directory entry naming an inode that does not exist", base,
         lambda img: poke(img, dd, u32le(0x7fffffff)), ["ls", "2"], "refuse",
         msg="past the last inode")


# The orphan list is a chain through i_dtime, so an image can point an
# inode at itself and a reader that follows pointers never stops.
def orphan_cases(base):
    live = ino_of(base, "/small")
    other = ino_of(base, "/empty")

    # These go in through debugfs rather than by poking bytes, so that
    # the superblock and inode checksums stay right and the only thing
    # wrong with the image is the thing under test. Poking the bytes
    # directly broke the superblock checksum, e2fsck refused to open
    # the image at all, and the case would have passed for a reason
    # that had nothing to do with the orphan list.
    def dbg(img, *cmds):
        for c in cmds:
            subprocess.run(["debugfs", "-w", "-R", c, img],
                           stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)

    def loop(img):
        dbg(img, "ssv last_orphan %d" % live,
            "sif <%d> dtime %d" % (live, other),
            "sif <%d> dtime %d" % (other, live))
    # e2fsck does see this one, though not as a loop: an inode with a
    # dtime set and links pointing at it is its own complaint.
    case("orphan list that loops", base, loop, ["orphans"], "refuse",
         msg="it is a loop")

    case("orphan list naming an inode past the end", base,
         lambda img: dbg(img, "ssv last_orphan 2147483647"), ["orphans"],
         "refuse", e2fsck_sees=False, msg="past the last inode",
         why="e2fsck leaves the orphan list alone on a cleanly unmounted image")


# Damage that leaves the image readable and wrong. These are the ones
# that have to come out as exit 1 rather than as a refusal, because
# extv can read every structure involved and the numbers do not agree.
def inconsistent_cases(base):
    bs = blocksize(base)

    def flip_block_bitmap(img):
        out = sh("dumpe2fs", img)
        m = re.search(r"^Group 0:.*?^  Block bitmap at (\d+)", out,
                      re.M | re.S)
        b = int(m.group(1))
        old = peek(img, b * bs + 100, 1)
        poke(img, b * bs + 100, bytes([old[0] ^ 0xff]))
    case("a byte of the block bitmap inverted", base, flip_block_bitmap,
         ["check"], "inconsistent")

    def wrong_free_count(img):
        first = 1 if bs == 1024 else 0
        # bg_free_blocks_count_lo is 12 bytes into the first
        # descriptor, which starts at the beginning of the table.
        poke(img, (first + 1) * bs + 12, u16le(7))
    case("a group descriptor's free block count changed", base,
         wrong_free_count, ["check"], "inconsistent")

    def wrong_links(img):
        n = ino_of(img, "/small")
        poke(img, inode_offset(img, n) + 26, u16le(7))
    case("an inode's link count changed", base, wrong_links, ["check"],
         "inconsistent")

    def sb_free_inodes(img):
        poke(img, 1024 + 16, u32le(123456))
    case("the superblock's free inode count changed", base, sb_free_inodes,
         ["check"], "inconsistent")

    def sb_free_blocks(img):
        poke(img, 1024 + 12, u32le(99))
    case("the superblock's free block count changed", base, sb_free_blocks,
         ["check"], "inconsistent")

    # Two inodes pointing at the same block. e2fsck runs an extra
    # pass for this and calls it multiply-claimed; extv names both
    # inodes. The path that reports it was dead code until this case
    # existed, because nothing in a corpus built by mke2fs shares a
    # block.
    def share_a_block(img):
        a = ino_of(img, "/b1023")
        b = int(sh("debugfs", "-R", "bmap /small 0", img).strip().split()[-1])
        subprocess.run(["debugfs", "-w", "-R", "sif <%d> block[0] %d" % (a, b),
                        img], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    if not has_extents(base):
        case("one block claimed by two inodes", base, share_a_block,
             ["check"], "inconsistent", msg="claimed by inode", tag="shared")

    def clear_inode_bit(img):
        out = sh("dumpe2fs", img)
        m = re.search(r"^Group 0:.*?^  Inode bitmap at (\d+)", out,
                      re.M | re.S)
        b = int(m.group(1)) * bs
        n = ino_of(img, "/small") - 1
        old = peek(img, b + n // 8, 1)[0]
        poke(img, b + n // 8, bytes([old & ~(1 << (n % 8))]))
    case("an in-use inode's bit cleared in the inode bitmap", base,
         clear_inode_bit, ["check"], "inconsistent")


def main():
    for base in BASES:
        superblock_cases(base)
        extent_cases(base)
        directory_cases(base)
        orphan_cases(base)
        inconsistent_cases(base)
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
    print("COUNT corrupt_oracle_silent %d" % silent)
    for k in sorted(tags):
        print("COUNT corrupt_%s %d" % (k, tags[k]))
    print("DONE")


EXTV = os.environ.get("EXTV", "./target/release/extv")
main()
