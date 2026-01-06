#!/usr/bin/env python3
"""Build the directory tree that every corpus image is made from.

Deterministic on purpose: no /dev/urandom, no timestamps that vary, so two
runs of the suite build byte-identical content and a disagreement is never
the corpus moving underneath the comparison.
"""
import os
import sys


def write(path, data):
    with open(path, "wb") as f:
        f.write(data)


def main():
    root = sys.argv[1]
    os.makedirs(root, exist_ok=True)
    write(os.path.join(root, "empty"), b"")
    write(os.path.join(root, "small"), b"hello extv\n")
    # One byte short of a 1024-byte block, one byte over, and a size that
    # is a whole number of 4096-byte blocks: the three ways the last block
    # of a file goes wrong.
    write(os.path.join(root, "b1023"), b"a" * 1023)
    write(os.path.join(root, "b1025"), b"b" * 1025)
    write(os.path.join(root, "b4096"), b"c" * 4096)
    # 100000 bytes is 98 blocks at 1024, 25 at 4096: past the twelve direct
    # pointers for every block size in the corpus.
    write(os.path.join(root, "hundredk"), bytes(range(256)) * 390 + b"x" * 40)
    # A megabyte, so an extent file needs more than one extent when the
    # free space is fragmented, and an indirect file needs several indirect
    # blocks.
    write(os.path.join(root, "meg"), bytes(range(256)) * 4096)

    # Holes. mke2fs -d preserves them, so a sparse source file is how the
    # deep indirect levels are reached without a multi-gigabyte image: the
    # inode below allocates four blocks and reaches the triple indirect.
    for name, off in (("hole_sind", 200 * 1024), ("hole_dind", 3 * 1024 * 1024),
                      ("hole_tind", 80 * 1024 * 1024)):
        with open(os.path.join(root, name), "wb") as f:
            f.seek(off)
            f.write(b"end\n")
    # Forty separated chunks. With extents that is forty extents, which
    # does not fit in the four an inode holds, so the tree grows an index
    # node and a depth greater than zero; with a block map it scatters
    # entries across several indirect blocks.
    with open(os.path.join(root, "chunks"), "wb") as f:
        for i in range(40):
            f.seek(i * 1024 * 1024)
            f.write(b"chunk%03d" % i * 512)

    # A hole in the middle rather than at the front.
    with open(os.path.join(root, "hole_mid"), "wb") as f:
        f.write(b"front" * 100)
        f.seek(512 * 1024)
        f.write(b"back" * 100)

    # 61 and 100 bytes. With inline_data an inode holds sixty bytes of
    # file content and puts the rest in a system.data attribute, so
    # these two are the only files in the corpus that reach the join
    # between the two halves. mke2fs will not do the same for a
    # directory: at five entries it gives up and allocates a block.
    write(os.path.join(root, "inline61"), b"q" * 61)
    write(os.path.join(root, "inline100"), b"q" * 100)

    os.makedirs(os.path.join(root, "emptydir"), exist_ok=True)
    deep = os.path.join(root, "nested", "a", "b", "c")
    os.makedirs(deep, exist_ok=True)
    write(os.path.join(deep, "deep.txt"), b"bottom\n")

    # Enough entries to need several directory blocks at every block size,
    # which is what e2fsck -fD turns into a hash tree.
    many = os.path.join(root, "manydir")
    os.makedirs(many, exist_ok=True)
    for i in range(500):
        write(os.path.join(many, "entry%04d" % i), b"%d\n" % i)
    # Names at the edges: one character, the 255-byte maximum, and names
    # whose bytes are not ASCII, because name_len is a byte count and a
    # reader that counts characters gets those wrong.
    write(os.path.join(many, "z" * 255), b"longest name allowed\n")
    write(os.path.join(root, "x"), b"one character name\n")
    # A name whose bytes are not ASCII. The source stays ASCII (METHOD's
    # rule 3) and the name is built from the bytes themselves.
    write(os.path.join(root, os.fsdecode(b"na\xc3\xafve")), b"not ascii\n")

    os.symlink("small", os.path.join(root, "sym_short"))
    # 60 bytes is the limit for a target stored inside the inode; past it
    # the target gets a block of its own, which is a different read path.
    os.symlink("t" * 200, os.path.join(root, "sym_long"))
    os.symlink("nowhere/at/all", os.path.join(root, "sym_dangling"))
    os.link(os.path.join(root, "small"), os.path.join(root, "small_hardlink"))
    print(root)


main()
