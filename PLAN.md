# extv

## Contract

Read an ext2, ext3 or ext4 filesystem image from an ordinary file, in
userspace, with no mount and no privilege, and report what is in it: the
superblock, every block group descriptor, inodes, the blocks each inode
owns (classic indirect maps and ext4 extent trees), directory entries
(linear and hash-indexed), extended attributes in the inode and in a
separate block, and the journal's state. Then check the image for
consistency: bitmaps against actual allocation, link counts against the
directory entries that were found, extent tree invariants, the orphan
list, and whether the journal needs replaying.

Refuse a damaged image rather than guess at it.

## Hard invariant

Every fact extv prints about an image agrees with e2fsprogs, and the two
agree in both directions on whether the image is consistent. A field extv
prints that no oracle checks is a field the README has to admit is
unverified.

## Oracle

Ranked by how little control we have over them.

1. `e2fsck -fn`. The consistency oracle, and the hardest to argue with,
   because it is the program everyone else runs. Both directions: clean
   must mean clean, dirty must mean dirty.
2. `dumpe2fs`. Superblock and group descriptor fields, field by field,
   plus the block and inode bitmaps (`-b`, and the per-group free lists).
3. `debugfs`. Inode contents (`stat`), directory listings (`ls -l`),
   extent trees (`dump_extents`), block maps (`blocks`), extended
   attributes (`ea_list`, `ea_get`), hash trees (`htree_dump`, which
   prints the hash of every name, so our hash implementation is checked
   name by name rather than in aggregate).
4. `mke2fs`, `tune2fs` and `e2fsck -fD` build the corpus. We control the
   corpus completely, which is why its shape is counted and printed:
   a corpus that is all 4096-byte ext4 proves one layout.

## Non-goals

Writing. Repair. Mounting. Reading a block device (it would work, but
nothing here tests it, so it is not claimed). ext4 encryption, verity,
casefolding and quota files. Replaying the journal, as opposed to reading
its superblock and reporting what state it is in.

## Commit sequence

- [x] Skeleton: Cargo.toml with no dependencies, Makefile, .gitignore, LICENSE.
    why: the collection's gate runs `make`, so the first commit has to build.
- [x] Image reader: open a file, read one block at a time into a buffer of
      exactly block_size bytes.
    why: an image-sized buffer makes every block-boundary over-read invisible,
      which is the defect class METHOD records eighteen times.
- [x] Superblock parse: the first 1024 bytes at offset 1024, magic and the
      fields that do not depend on a feature flag.
    why: nothing else can be located without block size and inodes per group.
- [x] `extv super` printing one `key value` per line.
    why: a flat key/value output is comparable field by field, which is the
      granularity the oracle needs.
- [x] t/run.sh: build one image with mke2fs, compare `extv super` against
      dumpe2fs field by field, print `N passed, M failed`.
    why: the harness exists before the features do, or it never measures them.
- [x] Feature flags: compat, incompat and ro_compat decoded to names.
    why: everything after this branches on them, and dumpe2fs prints the
      same list so the decode is checked rather than assumed.
- [x] Corpus builder covering block size 1024, 2048 and 4096.
    why: the 1024 case alone catches a whole class of offset bug, because it
      is the only one where the superblock is not in block 0.
- [x] Corpus shapes: ext2, ext3 and ext4, with and without a journal.
    why: no-journal and no-extent images take different code paths and are
      what old images actually look like.
- [x] Block group descriptors, 32-byte and 64-byte forms.
    why: the 64bit feature doubles the descriptor and splits every field into
      a low and a high half, which is where a reader usually breaks.
- [x] `extv groups` compared against dumpe2fs per group, per field.
    why: one image has many groups, so this is thousands of comparisons, not
      one.
- [x] Group descriptor checksums: crc16 and crc32c forms, verified.
    why: dumpe2fs prints the checksum, so a wrong crc32c is caught by a number
      rather than by nothing.
- [x] Inode table read: locate an inode, decode the 128-byte base.
    why: the base layout is shared by every version, so it comes first.
- [x] Inode extra fields: i_extra_isize, the 64-bit timestamps, crtime.
    why: a 256-byte inode holds the nanosecond and epoch-extension bits, and
      debugfs prints all four timestamps so they can be checked.
- [x] `extv stat` compared against `debugfs -R stat` field by field.
    why: the inode is where every later feature is read from.
- [x] Classic indirect block maps: direct, single, double, triple.
    why: this is every ext2 file and every ext4 file without the extent flag.
- [x] `extv blocks` compared against `debugfs -R blocks`, block for block.
    why: an ordered list of thousands of block numbers is a sharper
      instrument than a block count.
- [x] Sparse files: a hole in the middle of an indirect map.
    why: debugfs prints nothing for a hole, and a reader that prints 0 instead
      of skipping disagrees only on a file that has one.
- [x] Extent tree: the leaf case, header validation, magic.
    why: the common ext4 file, and the only place where a bad magic has to be
      a refusal rather than a guess.
- [x] Extent tree: interior nodes, depth greater than zero.
    why: a one-level tree is the only shape a small corpus produces, and the
      recursion is untested until a file is big enough to need it.
- [x] `extv extents` compared against `debugfs -R dump_extents`.
    why: that command prints level, entries, logical range, physical range and
      flags, so each is a separate comparison.
- [x] Uninitialised extents.
    why: the length field's top bit changes the meaning, and a reader that
      ignores it reports a 32768-block extent as a 0-block one.
- [x] Linear directory reading, with rec_len and name_len validated against
      the block end.
    why: the first place where a malformed image can walk a pointer off the
      end of a buffer.
- [x] `extv ls` compared against `debugfs -R ls -l` for every directory in
      the corpus.
    why: inode number, mode, uid, gid, size and name per entry, over every
      directory, is the broadest comparison available.
- [x] Path resolution, so `ls` takes a path rather than an inode number.
    why: it is how the tool is actually used, and it exercises the directory
      reader at every level of the tree.
- [x] The 64KiB rec_len encoding for 65536-byte blocks.
    why: a documented special case that no default corpus reaches.
- [x] Directory entry checksums (metadata_csum) and the tail entry.
    why: the tail looks like an entry with inode 0 and has to be skipped, not
      listed, and the checksum is a number the oracle also has.
- [x] htree: read the root and interior index blocks, report depth and the
      hash of each block.
    why: the structure, before the hash function that fills it.
- [x] The three directory hash functions: legacy, half_md4 and tea, signed
      and unsigned.
    why: `debugfs htree_dump` prints the hash of every name, so every name in
      a hashed directory is one comparison.
- [x] A corpus image with hashed directories, built by `e2fsck -fD`.
    why: mke2fs does not build htrees, so without this step every htree test
      passes vacuously. Assert the path ran.
- [x] Extended attributes in the inode body, after i_extra_isize.
    why: the common case, and the one mke2fs produces on its own through
      SELinux labels.
- [x] Extended attributes in a separate block, via i_file_acl.
    why: a different header, a refcount, and the case a large value forces.
- [x] `extv ea` compared against `debugfs -R ea_list`, name and value.
    why: value bytes, not just names, because the offset arithmetic is what
      goes wrong.
- [x] inline_data: file and directory contents inside the inode.
    why: it changes where directory entries live, and a reader that assumes
      blocks reports an empty directory.
- [x] Journal superblock: read the inode, report sequence, start and state.
    why: whether the image needs recovery is a fact e2fsck also reports.
- [x] Block bitmap check: every block the inodes claim, against the bitmap.
    why: the first real consistency check, and the one e2fsck's pass 5 does.
- [x] Inode bitmap check, and the free counts in each group descriptor.
    why: same comparison on the other table, plus the summary counters.
- [x] Link count check: count directory entries per inode, compare to
      i_links_count.
    why: e2fsck pass 4, and it needs the whole directory tree walked first.
- [x] Orphan inode list, and the orphan_file feature.
    why: a list that is normally empty is one nothing tests until it is not.
- [ ] `extv check` exit status, and the e2fsck agreement test in both
      directions over the whole corpus.
    why: the hard invariant. Clean must mean clean too.
- [ ] Corrupt corpus: bad superblock magic, truncated image, garbage group
      descriptors.
    why: refusals counted separately from wrong answers, always.
- [ ] Corrupt corpus: extent tree with a bad magic, a depth that does not
      decrease, a loop.
    why: the recursion is where a hostile image gets a reader to run forever.
- [ ] Corrupt corpus: directory rec_len of 0, rec_len past the block end, a
      name_len that overruns.
    why: a dirent loop that never advances is the classic hang.
- [ ] A refusal has to be a refusal, not a wrong answer: assert the exit code
      and that nothing was printed as fact.
    why: METHOD's point about `unimpl()`. A missing feature's failure mode is
      quiet nonsense, not a missing message.
- [ ] `make overread`: three planted one-past-the-end reads, each required to
      panic.
    why: watched to go red with the exact-sizing reverted, or it is decoration.
- [ ] `make mutants`: mutate each decode path, confirm the suite goes red,
      write down the survivors.
    why: a test that cannot fail is not a test, and the survivors are the
      honest part of the report.
- [x] Corpus shape census printed by the suite: how many images of each block
      size, inode size, feature set.
    why: a number from a corpus of one shape is a number about one shape.
- [ ] bigalloc.
    why: the cluster/block distinction changes every bitmap offset, and it is
      the feature most likely to be silently wrong.
- [ ] 64bit with more than 2^32 blocks is out of reach (the image would be
      16 TiB), so say so rather than claim it.
    why: a README that lists only successes is not finished.
- [ ] README: what it does, the oracles, the measured agreement numbers, the
      corpus census, and what does not work.
    why: the numbers go stale the moment they are written by hand rather than
      printed by the suite.
