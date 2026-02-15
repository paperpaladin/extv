#!/usr/bin/env python3
"""Read whole files out of the image and compare them with debugfs dump.

This is the end to end check: the block map or the extent tree, the
holes, the inline data and the size all have to be right together, and
a single wrong byte anywhere in the chain shows up here even when
every structural comparison agrees.

Not every file: the corpus has five hundred two-byte files in one
directory that are all the same shape, and two sparse files whose
logical size is 80 MB. What is compared is every regular file in the
root directory under 2 MB, plus one from the deep path and one from
the big directory.
"""
import os
import shutil
import subprocess
import sys
import tempfile

EXTV, IMG, NAME = sys.argv[1], sys.argv[2], sys.argv[3]
LIMIT = 2 * 1024 * 1024


def extv(*args):
    p = subprocess.run([EXTV] + list(args), stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
    return p.returncode, p.stdout, p.stderr


def listing(path):
    rc, out, err = extv("ls", IMG, path)
    if rc != 0:
        return []
    files = []
    for line in out.decode("utf-8", "surrogateescape").splitlines():
        f = line.split(" ", 7)
        if f[0] == "entry" and f[2].startswith("100"):
            files.append((int(f[1]), int(f[6]), f[7]))
    return files


def main():
    want = [(ino, size) for ino, size, _ in listing("/") if size <= LIMIT]
    for extra in ("/nested/a/b/c", "/manydir"):
        got = listing(extra)
        want += [(ino, size) for ino, size, _ in got[:2] if size <= LIMIT]
    work = tempfile.mkdtemp(prefix="extv.cat.")
    try:
        cmds = "".join("dump <%d> %s/%d\n" % (ino, work, ino) for ino, _ in want)
        subprocess.run(["debugfs", "-f", "/dev/stdin", IMG], input=cmds.encode(),
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        bad = []
        nfiles = 0
        nbytes = 0
        padded = 0
        for ino, size in want:
            theirs_path = os.path.join(work, str(ino))
            if not os.path.exists(theirs_path):
                bad.append("inode %d: debugfs dumped nothing" % ino)
                continue
            with open(theirs_path, "rb") as f:
                theirs = f.read()
            rc, ours, err = extv("cat", IMG, str(ino))
            if rc != 0:
                bad.append("inode %d: extv cat exited %d: %s"
                           % (ino, rc, err.decode().strip()))
                continue
            if theirs == ours:
                nfiles += 1
                nbytes += len(ours)
                continue
            # debugfs writes the whole sixty-byte inline area for a file
            # shorter than that, rather than i_size bytes. Its own ls
            # prints the real size, so this is debugfs disagreeing with
            # itself and extv agreeing with the inode. Checked as a
            # property rather than waved through: their copy must be
            # ours followed by zeroes.
            if (len(theirs) == 60 and len(ours) == size and size < 60
                    and theirs[:size] == ours and theirs[size:] == b"\0" * (60 - size)):
                padded += 1
                nfiles += 1
                nbytes += len(ours)
                continue
            bad.append("inode %d: %d bytes from debugfs, %d from extv, "
                       "first difference at %d"
                       % (ino, len(theirs), len(ours),
                          next((i for i in range(min(len(theirs), len(ours)))
                                if theirs[i] != ours[i]), min(len(theirs), len(ours)))))
        for b in bad[:6]:
            print("FAIL %s cat %s" % (NAME, b))
        if not bad:
            print("ok   %s cat (%d files, %d bytes, %d short inline files where "
                  "debugfs pads to 60)" % (NAME, nfiles, nbytes, padded))
        print("COUNT cat_files %d" % nfiles)
        print("COUNT cat_bytes %d" % nbytes)
        print("COUNT cat_inline_padded %d" % padded)
        print("DONE")
    finally:
        shutil.rmtree(work, ignore_errors=True)


main()
