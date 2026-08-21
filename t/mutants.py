#!/usr/bin/env python3
"""Mutate one decode rule at a time and insist the suite notices.

Each entry names a file, the exact text to replace, what to replace it
with, and whether the suite is expected to go red. The expectation is
part of the table on purpose: a mutant that survives is a fact about
how weak the suite is at that point, and writing it down beside the
reason is the only way the number stays honest. If a mutant marked
"dies" survives, or one marked "survives" starts dying, this target
fails and the table is wrong.

The replacement text must appear exactly once. A mutation that no
longer matches after a refactor would otherwise be a silent no-op,
which is the same defect as a test that runs nothing.

Run it through `make mutants`. It rebuilds and runs the whole suite
once per mutant, so it takes about as long as the number of mutants
times a minute.
"""
import os
import re
import signal
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# (label, file, old, new, expectation, note)
MUTANTS = [
    ("rec_len decoded without the 65536 case", "src/dir.rs",
     """    if block_size < 65536 {
        raw as u32
    } else if raw == 0xffff || raw == 0 {
        block_size
    } else {
        ((raw & 0xfffc) as u32) | (((raw & 3) as u32) << 16)
    }""",
     "    let _ = block_size;\n    raw as u32", "dies", ""),

    ("a directory entry shorter than its header allowed", "src/dir.rs",
     "        if rec_len < 8 {", "        if false && rec_len < 8 {",
     "dies", ""),

    ("a name longer than the entry holding it allowed", "src/dir.rs",
     "        if 8 + name_len as usize > rec_len as usize {",
     "        if false && 8 + name_len as usize > rec_len as usize {",
     "dies", ""),

    ("an entry running past the end of its block allowed", "src/dir.rs",
     "        if off + rec_len as usize > bsize {",
     "        if false && off + rec_len as usize > bsize {", "dies", ""),

    ("uninitialised groups left out of the block bitmap", "src/bitmap.rs",
     """        for b in group_metadata_blocks(sb, g as u64, gd) {
            bits.set(b / ratio);
        }""",
     "        let _ = (g, gd, ratio);", "dies", ""),

    ("the cluster ratio fixed at one", "src/bitmap.rs",
     "pub fn cluster_ratio(sb: &Sb) -> u64 {\n    (sb.cluster_size / sb.block_size) as u64\n}",
     "pub fn cluster_ratio(sb: &Sb) -> u64 {\n    let _ = sb;\n    1\n}",
     "dies", ""),

    ("an INODE_UNINIT group's bitmap read off the disk", "src/bitmap.rs",
     "        if gd.flags & BG_INODE_UNINIT != 0 {\n            continue;\n        }",
     "", "dies",
     "needed an image with 0xff written over such a bitmap; it survived "
     "until that image was built"),

    ("an inode's attribute block not claimed", "src/check.rs",
     "        if ino.file_acl != 0 {\n            claim(ino.file_acl, &mut problems);\n        }",
     "", "dies", ""),

    ("directories not counted per group", "src/check.rs",
     "            dirs_in.set(num as u64 - 1);", "", "dies", ""),

    ("dot not counted towards a link count", "src/check.rs",
     '                if e.inode >= 1 && e.inode <= sb.inodes_count {',
     '                if e.inode >= 1 && e.inode <= sb.inodes_count\n                    && e.name != b"."\n                {',
     "dies", ""),

    ("the superblock's free block count not compared", "src/check.rs",
     "    if total_free_clusters * ratio != sb.free_blocks {",
     "    if false && total_free_clusters * ratio != sb.free_blocks {",
     "dies", ""),

    ("the inode bitmap not compared", "src/check.rs",
     "        if inodes_used.get(i) == ib.get(i) {",
     "        if true || inodes_used.get(i) == ib.get(i) {", "dies", ""),

    ("i_blocks never compared with the blocks an inode owns",
     "src/check.rs", "        if !ino.counts_blocks_in_clusters() {",
     "        if false && !ino.counts_blocks_in_clusters() {", "dies",
     "found by the single-bit sweep, not by a case anyone wrote"),

    ("the inode checksum not verified by check", "src/check.rs",
     "        if sb.has_ro(crate::sb::RO_METADATA_CSUM) {\n            let c = crate::csum::inode",
     "        if false && sb.has_ro(crate::sb::RO_METADATA_CSUM) {\n            let c = crate::csum::inode",
     "dies",
     "survived a sweep over the ext2 base alone, which has no "
     "metadata_csum: it needed the sweep to run over both layouts"),

    ("a free inode left on the orphan list not noticed", "src/check.rs",
     "        if ino.links_count == 0 && ino.dtime != 0 && ino.dtime < sb.inodes_count {",
     "        if false && ino.links_count == 0 && ino.dtime != 0 && ino.dtime < sb.inodes_count {",
     "dies", ""),

    ("the bitmap padding past the end of the filesystem not checked",
     "src/check.rs", "    if let Some(gd) = gs.last() {",
     "    if let Some(gd) = gs.last().filter(|_| false) {", "dies", ""),

    ("a block claimed by two inodes not reported", "src/check.rs",
     "            if prev != 0 && prev != num {",
     "            if false && prev != 0 && prev != num {", "dies",
     "dead code until a damaged image shared a block: nothing built "
     "by mke2fs does"),

    ("a deleted inode's dtime ignored", "src/check.rs",
     "    i.links_count > 0 && i.dtime == 0", "    i.links_count > 0",
     "survives",
     "the one deleted inode in the corpus has no links either, so the "
     "link count alone already excludes it. An inode with links and a "
     "dtime is itself an inconsistency, so a corpus image cannot have "
     "one and stay clean"),

    ("the orphan block magic not checked", "src/check.rs",
     "            if v.u32(sb.block_size as usize - 8)? != ORPHAN_BLOCK_MAGIC {",
     "            if false && v.u32(sb.block_size as usize - 8)? != ORPHAN_BLOCK_MAGIC {",
     "survives",
     "every orphan file block in the corpus carries the magic, and a "
     "damaged one is not reachable through debugfs, so nothing exercises "
     "the branch"),

    ("the orphan chain walked no further than its head", "src/check.rs",
     "        num = inode::read(img, sb, num)?.dtime;", "        num = 0;",
     "dies",
     ""),

    ("an extent node's depth not required to fall", "src/extent.rs",
     "    if ch.depth + 1 != parent.depth {",
     "    if false && ch.depth + 1 != parent.depth {", "dies",
     "survived twice: once until the damaged-image cases asserted which "
     "message the refusal carries, and again because the rule was written "
     "out in two places and only one of them was being mutated"),

    ("the extent magic not checked", "src/extent.rs",
     "    if magic != MAGIC {", "    if false && magic != MAGIC {", "dies", ""),

    ("the journal superblock read little-endian", "src/journal.rs",
     "        maxlen: v.u32be(16)?,", "        maxlen: v.u32(16)?,", "dies", ""),

    ("the journal's 64bit feature dropped from the table", "src/journal.rs",
     '    (0x0002, "journal_64bit"),', "", "dies", ""),

    ("a group descriptor's location not range checked", "src/group.rs",
     "    let past = match b.checked_add(span) {\n        None => true,\n        Some(e) => e > sb.blocks_count,\n    };",
     "    let past = b.wrapping_add(span) > sb.blocks_count;", "dies",
     "the wrapping form is the bug that was there: a 64-bit descriptor "
     "full of 0xff wraps to inside the filesystem"),

    ("a device inode walked as a block map", "src/map.rs",
     "    if ino.is_special() {", "    if false && ino.is_special() {",
     "dies", ""),

    ("the directory hash read through an unsigned char", "src/hash.rs",
     "        (b as i8) as i32 as u32", "        b as u32", "dies", ""),

    ("the half_md4 third round left out", "src/hash.rs",
     "    round!(h, b, c, d, a, input[4].wrapping_add(K3), 15);", "",
     "dies", ""),

    ("a bitmap checksum taken over one byte too few", "src/csum.rs",
     "pub fn bitmap(sb: &Sb, bits: &[u8]) -> u32 {\n    crc32c(seed(sb), bits)\n}",
     "pub fn bitmap(sb: &Sb, bits: &[u8]) -> u32 {\n    crc32c(seed(sb), &bits[..bits.len() - 1])\n}",
     "dies", ""),

    ("an extent block's tail length taken from the count", "src/csum.rs",
     "    let n = 12 + 12 * max as usize;",
     "    let n = 12 + 12 * u16::from_le_bytes([block[2], block[3]]) as usize;",
     "dies", ""),

    ("a directory block's checksum taken over its own tail",
     "src/csum.rs", "    crc32c(c, &block[..block.len() - 12])",
     "    crc32c(c, block)", "dies", ""),

    ("the crc32c polynomial changed by one bit", "src/crc.rs",
     "0x82F6_3B78 ^ (c >> 1)", "0x82F6_3B79 ^ (c >> 1)", "dies", ""),
]


def suite_is_green():
    """True when the suite passed, which for a mutant means it survived."""
    p = subprocess.run(["make", "test"], cwd=ROOT, stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT)
    text = p.stdout.decode("latin-1")
    m = re.search(r"^(\d+) passed, (\d+) failed", text, re.M)
    return p.returncode == 0 and m is not None and m.group(2) == "0"


def refuse_if_dirty(paths):
    """Mutating a tree that already differs from HEAD measures nothing.

    This is not hypothetical. The first run of this target was killed
    by a timeout part way through, which left the mutation it was
    holding in src/check.rs, and the next run then applied twenty-two
    mutations on top of it and reported 20 killed. The baseline was
    not the committed code and the numbers were about nothing in
    particular. One of the twenty-two could not even be applied,
    because its own text had already been replaced, and that is the
    only reason it was noticed.
    """
    p = subprocess.run(["git", "diff", "--name-only", "--"] + sorted(paths),
                       cwd=ROOT, stdout=subprocess.PIPE)
    dirty = [x for x in p.stdout.decode().split() if x]
    if dirty:
        print("mutants: these files differ from HEAD: %s" % " ".join(dirty))
        print("mutants: commit or stash them first. A mutation applied on"
              " top of an uncommitted change measures an unknown baseline,"
              " and an interrupted run of this target leaves exactly that.")
        sys.exit(2)


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    refuse_if_dirty({m[1] for m in MUTANTS})
    killed = 0
    survived = []
    wrong = []
    for label, path, old, new, expect, note in MUTANTS:
        if only and only not in label:
            continue
        print("--- %s" % label, flush=True)
        full = os.path.join(ROOT, path)
        src = open(full).read()
        if src.count(old) != 1:
            wrong.append("%s: the text to mutate appears %d times in %s, not "
                         "once, so the mutation would do nothing"
                         % (label, src.count(old), path))
            continue
        backup = src
        # A signal while the mutation is applied would otherwise leave
        # it in the tree, which is how the baseline was lost once.
        def restore(signum, frame):
            open(full, "w").write(backup)
            sys.exit(130)
        old_term = signal.signal(signal.SIGTERM, restore)
        old_int = signal.signal(signal.SIGINT, restore)
        try:
            open(full, "w").write(src.replace(old, new, 1))
            died = not suite_is_green()
        finally:
            open(full, "w").write(backup)
            signal.signal(signal.SIGTERM, old_term)
            signal.signal(signal.SIGINT, old_int)
        if died:
            killed += 1
            if expect != "dies":
                wrong.append("%s: expected to survive and the suite caught it"
                             % label)
            else:
                print("ok   killed: %s" % label, flush=True)
        else:
            survived.append((label, expect, note))
            if expect != "survives":
                wrong.append("%s: expected the suite to catch it and it did not"
                             % label)
            else:
                print("ok   survived as recorded: %s" % label, flush=True)

    print()
    print("--- survivors")
    if not survived:
        print("     none")
    for label, expect, note in survived:
        print("     %s" % label)
        if note:
            print("       %s" % note)
    print()
    for w in wrong:
        print("FAIL %s" % w)
    total = killed + len(survived)
    print("%d mutants, %d killed, %d survived, %d not as recorded"
          % (total, killed, len(survived), len(wrong)))
    # Rebuild unmutated, so a later `make test` is not run against the
    # last mutant's binary.
    subprocess.run(["make"], cwd=ROOT, stdout=subprocess.DEVNULL)
    sys.exit(1 if wrong else 0)


main()
