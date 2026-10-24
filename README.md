# extv

Reads an ext2, ext3 or ext4 filesystem image from an ordinary file and
checks it for consistency. No mount, no loop device, no privilege, no
dependencies: Rust with an empty `[dependencies]` and a Makefile.

```sh
make
./target/release/extv super disk.img
./target/release/extv ls disk.img /etc
./target/release/extv check disk.img ; echo $?
```

`check` exits 0 when the image is consistent, 1 when it is not, and 2
when the image says something impossible and extv refuses to guess. 3
is a usage error. Every other command prints one fact per line, in a
flat key-and-value form chosen so that it can be compared against
e2fsprogs field by field rather than as a block of prose.

Commands: `super`, `groups`, `csum`, `stat`, `map`, `blocks`,
`extents`, `ls`, `htree`, `ea`, `journal`, `bitmaps`, `orphans`,
`check`, `cat`.

## What it is measured against

Nothing here is checked against a number worked out by hand. Every
fact extv prints is compared against e2fsprogs, which is the program
everyone else runs:

- `e2fsck -fn` is the consistency oracle, in both directions. When
  extv says an image is clean e2fsck has to agree, and when extv says
  an image is broken some outside authority has to agree with that
  too.
- `dumpe2fs` for the superblock, the group descriptors, the free block
  and free inode lists, and the journal superblock.
- `debugfs` for inodes (`stat`), directory listings (`ls -l`), block
  maps (`blocks`), extent trees (`dump_extents`), hash trees
  (`htree_dump`, which prints the hash of every name), extended
  attributes (`ea_list`, `ea_get`) and file contents (`dump`).
- `mke2fs`, `tune2fs` and `e2fsck -fD` build the corpus.

`make test` refuses to run at all if e2fsprogs is missing. It is the
only oracle for every fact in the suite and the only way to build an
image, so a reduced run would print a number having compared nothing.

## What the suite measures

769 assertions over 37 images, from the run of 2 November 2026. The
suite prints these totals itself; they are not maintained by hand.

| | |
|---|---|
| superblock fields against dumpe2fs | 1,669 |
| group descriptor fields | 3,718 across 285 groups |
| inode fields against debugfs stat | 375,792 over 22,200 inodes |
| inode checksums recomputed | 12,737 |
| metadata checksums recomputed and matching | 1,140 |
| block numbers against debugfs | 205,599 in 51,589 runs |
| extent tree rows against dump_extents | 15,171 |
| directory entries against debugfs ls -l | 28,355 in 292 directories |
| names hashed against debugfs htree_dump | 12,168 in 9 indexed directories |
| extended attributes against ea_list and ea_get | 21,005, 643,477 value bytes |
| file bytes against debugfs dump | 67,833,318 over 612 files |
| symlink targets | 72 fast against debugfs stat, 36 slow against dump |
| journal superblock fields | 210 over 30 images with a journal |
| free block and free inode runs against dumpe2fs | 553 |
| consistency counters checked by `extv check` | 1,157 |
| link counts checked against the names that make them | 27,735 |
| single-bit faults in the metadata | 120 |
| damaged-image cases | 85 |

The byte count moves by a few bytes between runs and the file count
does not. Two of the 612 files are taken from the front of a 521-entry
directory, and in the images where that directory is hash indexed the
front of it depends on the per-image hash seed mke2fs picks at random,
so a slightly different pair gets read. The two runs behind this table
differ by 11 bytes.

The 1,140 recomputed checksums are the ones `extv csum` recalculates
from the image and compares with what is stored. The suite counts
three kinds of them separately: 703 directory block tails, 150
bitmaps and 36 extent blocks. The rest are the group descriptors and
the superblock, and there is no separate count for those, so no
number is given here rather than one worked out by subtraction. The
12,737 inode checksums are a different total because `extv check`
verifies those rather than `extv csum`.

## The corpus

A corpus that is all 4096-byte-block default ext4 proves one layout,
so the suite prints what it actually holds on every run:

```
type: ext4=28 ext3=3 ext2=6
block size: 1024=24 2048=4 4096=7 65536=2
inode size: 128=7 256=30
tags: extent=21 64bit=19 csum=17 htree=9 journal=6 plain=3
      noextent=2 nocsum=2 uninitbg=2 bigblock=2 orphan=2 gap=2
      bigalloc=2 devnodes=2 signed=2 legacy=2 tea=2 unsigned=3
      half_md4=1 inline=1 metabg=1 manygroups=1 nosparse=1
      nodirindex=1 hugedir=1 jused=1 uninitgarbage=1
```

The 1024-byte-block images are the only ones where the superblock is
not in block 0, which is a whole class of offset bug on its own. The
65536-byte-block images exist because that is the only block size
where a directory entry can be longer than the 16-bit field that
records its length; mke2fs warns that the block is too big for a
machine with 4096-byte pages and builds the image anyway, and
dumpe2fs, debugfs and e2fsck all read it.

Several images exist because a mutation survived without them, which
is recorded in the commit that added each one. The clearest is
`ext4-1024-256`, which has 0xff written over the inode bitmap of a
group marked INODE_UNINIT: a reader that ignored the flag agreed with
the oracle anyway, because mke2fs leaves those blocks zeroed and zero
is the same answer.

### Asserting that the corpus reaches the code

An htree test on a corpus with no hashed directories passes having
run nothing. So the suite counts the paths it reaches and fails when
one is never taken: single, double and triple indirect maps, extent
trees deeper than one node, uninitialised extents, inline data in
both files and directories, fast symlinks, attributes in the inode
and in a block, directory checksum tails, the 65536-byte rec_len
encoding, all six directory hash functions, each of the three group
descriptor checksum forms, both uninitialised group flags, both
spellings dumpe2fs uses for a free run, both kinds of symlink target,
and an image with no journal at all.

## Damaged images

85 cases, built by breaking one thing in a clean image: 66 refusals,
19 inconsistencies, 0 wrong answers. Refusals are counted apart from
wrong answers because they are different results, and a run that
reported them together would let one hide inside the other.

Three base images, because the shape of the damage depends on the
layout: a 4096-byte ext4 one, which is the only one with an extent
tree to break, a 1024-byte ext2 one, which is the only one where the
superblock is not in block 0, and a 1024-byte ext4 one with
inline_data, which is the only one where a file's contents live in
the inode.

A refusal has to be all three of exit 2, a single line on stderr
beginning with the words malformed image, and nothing at all on
stdout. The third is the one that matters: a reader that prints forty
plausible fields and then refuses has already told the operator
something false. Where the refusal could happen for more than one
reason, the case names the substring the message has to contain.

e2fsck is the authority on every case, and getting that right took
two attempts:

- Its exit status is not its verdict. Changing the superblock's free
  inode count gets a line saying the count is wrong and an exit status
  of zero. The suite looks for the refused Fix prompt as well, in the
  damaged cases and in the clean-direction check.
- It recovers where extv refuses. Given a damaged primary superblock
  or descriptor table it reads the backups, checks the filesystem
  through those and exits 0. extv has no backup fallback. The
  authority is three voices now: a non-zero exit, e2fsck saying in
  words that it fell back to the backups, or dumpe2fs failing to parse
  the superblock. If none of the three notices, the case fails, which
  is what stops extv from agreeing with itself.

Four cases are deliberate differences and are counted as such. Three
are the same damage on each base: an orphan list head past the end of
the inode table, where e2fsck leaves the orphan list alone on a
cleanly unmounted image and sees nothing, and extv refuses to follow a
pointer out of the table.

The fourth is an inline_data inode whose i_size is past the end of the
inline area, and there the two oracles disagree with each other.
e2fsck 1.47.2 does not compare an inline inode's i_size with the bytes
it holds and calls the image clean; debugfs dump clamps to what is
there and writes 61 bytes without saying that the inode asked for 200.
extv refuses, because a file whose data is inline cannot be longer
than the inline area.

Two of the cases are about a length that comes off the image rather
than about a structure. A fast symlink keeps its target in the sixty
bytes of i_block and terminates it there, and `extv cat` used to take
the length from i_size instead, clamped with `.min(60)`: a five-byte
target under a forged i_size of 60 printed the target and then 55
bytes of whatever else was in the inode, which on a reused one is the
block pointers it held before. The clamp is what hid it. It kept the
read inside the array, so Rust's bound did not fire and no sanitizer
could have seen it. The cases forge the size to 60 and to 3, with
recognisable bytes written into i_block where the target is not, and
e2fsck calls the symlink invalid for both.

### Single-bit faults

The 85 cases above are faults somebody thought of. `t/fuzz.py` flips
one bit at a time at a random offset inside the metadata region of two
bases and asks the same two questions of each result: did extv stay
inside its contract, which is exit 0, 1 or 2 with no panic and no
hang, and when extv called the image clean did e2fsck agree. 120
faults per run, 60 on each base, from a fixed seed so a failure can be
reproduced by running the suite again, with a digest of the offsets
printed because a seeded check that ignores its seed passes while
generating something else.

Of the 120, 10 are refused, 14 come out as inconsistent, and 96 are
not noticed by either program, which is what a bit flipped in padding
or in a field nothing reads looks like. The count that has to stay at
zero is the misses, where extv says clean and e2fsck does not. It was
three when this first ran, and all three were real gaps: i_blocks
never compared against the blocks an inode owns, the inode checksum
never verified by `check`, and i_blocks compared as distinct blocks
when it counts references.

## make overread

METHOD records eighteen buffers in the sibling C repositories that
were larger than their data, which switches the sanitizer off over
everything past the real end. Rust is not exempt. Every index is
checked, but only against the length of the slice it was handed, so a
block buffer sized to the largest block rather than to this
filesystem's has the same blind spot: a read one byte past the end of
a 1024-byte block lands inside a live 4096-byte Vec and comes back as
whatever is there.

So every structure is read into a buffer of exactly its own size,
straight from the file, and `make overread` is the check that the
consequence holds. Three plants, each one byte past the end of a
block, a directory block and an inode, each required to panic with a
bounds check and exit 101. 101 is a status extv cannot produce any
other way: it uses 0, 1, 2 and 3, and a closed pipe is turned into 0
in `err.rs` rather than being allowed to panic.

`make test` depends on it. For nine commits it did not: the target was
here and nothing invoked it, so the plants had never been watched go
red on the way past an ordinary run, which is the only thing a gate is
for.

Each of the three has been watched go red, which takes a different
edit for each:

- `img.block` reading 4096 bytes instead of one block leaves the block
  plant silent.
- `inode::read` taking 256 bytes instead of the inode size does the
  same to the inode plant and leaves the other two green.
- The dirent plant needs two changes at once, and this is the
  direction the README used to leave out. Its buffer is the block
  buffer, and `parse_block` bounds the walk by `buf.len()`, so the
  planted read is one past the end whatever size the buffer is: an
  oversized `img.block` on its own does not silence it. Bounding the
  walk by `sb.block_size` as well does. That pair is the realistic
  defect, a rounded-up buffer and a length taken from the superblock
  rather than from the slice in hand, and neither half of it is
  enough alone.

## make mutants

38 mutations, one decode rule broken at a time, each with the whole
suite run against it: 35 killed, 3 survived, 0 not as recorded, from
the run of 2 November 2026. The table in `t/mutants.py` records which
are expected to survive and why, and the target fails when reality
disagrees with the table in either direction, so a mutant that starts
dying is as much a failure as one that starts surviving. This section
said twenty-three for a while, with 35 entries in the table.

The three survivors, each a fact about where the corpus stops rather
than about the code:

- The cluster merge without the logical position. The clause only
  matters for a data block landing in the same cluster as the one
  before it at a mismatched offset inside that cluster, and mke2fs
  does not produce an allocation that fragmented.
- A deleted inode's dtime ignored. The corpus's deleted inodes have no
  links either, so the link count already excludes them. An inode with
  links and a dtime set is itself an inconsistency, so an image cannot
  have one and stay clean.
- The orphan block magic not checked. Every orphan file block in the
  corpus carries the magic and a damaged one cannot be built through
  debugfs, so nothing reaches the branch.

### --check, and the two sweeps it made vacuous

The sweep is half an hour, so nobody is present for it and the table
rots quietly. One entry had been dead for several commits: "an inode's
attribute block not claimed" quoted a two-line `if` body and
`sectors += 1;` had been inserted into that branch, so the pattern
matched nothing. A dead entry is neither killed nor surviving. It does
nothing, and the run still prints a total that reads as complete.

`t/mutants.py --check` asks only whether every pattern still appears
exactly once in the file it names, which takes under a second, so
`make test` asks it on every run. It was watched red by inserting a
line into the branch that entry describes.

Wiring it into the suite then broke the sweep, in the direction that
flatters: a mutation is applied by replacing one of those exact
patterns, so every mutant failed the check and every mutant was
recorded as killed, including all three survivors above, each reported
as "expected to survive and the suite caught it". Two full sweeps ran
that way and measured nothing except that the mutation had been
applied. `suite_is_green` sets `EXTV_MUTATION_RUN=1` now and the suite
prints a counted SKIP for that one check while a mutation is in the
tree. The numbers above are from the first run with the interlock in
place.

## What does not work

It does not write, repair or mount anything. Reading a block device
would work and nothing here tests it, so it is not claimed. ext4
encryption, verity, casefolding, quota files and snapshots are not
read at all.

There is no backup superblock fallback. extv reads the primary
superblock and the primary descriptor table and refuses when they are
damaged, where e2fsck reads the backups and carries on. That is why
five of the damaged-image cases are caught by e2fsck's fallback
message rather than by its exit status.

The journal is read and never replayed. Its superblock is reported,
which includes whether a replay is needed. Nothing here mounts
anything, so every journal in the corpus is nearly pristine, at
sequence 1 and start 0. debugfs can commit one transaction through the
journal, which sets `journal_64bit`, and that is the only journal
state reachable without a mount, so one image has it.
`journal_checksum`, `journal_checksum_v2` and `v3`, `async_commit` and
`fast_commit` are set by the kernel at mount time and no image here
has them.

Four journal superblock fields have no oracle: `s_max_transaction`,
`s_max_trans_data`, `s_dynsuper` and `s_head`. dumpe2fs prints a line
called Max transaction length and it is not that field. Setting
`s_max_transaction` to 1234 by hand left the line reading the
journal's length, so what dumpe2fs prints there is `s_maxlen` a second
time.

Most of the orphan list has no oracle either. dumpe2fs prints the head
inode and the orphan file's inode number; the rest of the chain and
the contents of the orphan file blocks are read and reported and
nothing outside this program checks them. A longer chain cannot be
built to test against: it needs an inode with no links and a dtime
pointing at the next one, and e2fsck calls a deleted inode with a zero
dtime an error, so a realistic multi-entry list cannot exist in an
image that is otherwise clean.

64bit beyond 2^32 blocks is out of reach. The high halves of the
descriptor fields are read and compared, because the corpus has 64bit
images, but a filesystem with more than 2^32 blocks would be 16 TiB at
4096-byte blocks. The claim is that the wide fields are decoded, not
that a filesystem that large works.

The search for damage is shallow. 85 of the damaged cases are faults
placed by hand and the other 120 are single bit flips from a fixed
seed inside a two-megabyte metadata region, which is 120 points in a
space of sixteen million. It finds what it finds; it is not a
structure-aware fuzzer and it does not try to be.

This paragraph used to say "there is no fuzzer. The damaged images are
49 faults placed by hand, not a search". That sentence was added two
commits after `t/fuzz.py` was committed, so it was false the day it
was written.

## Layout

```
src/        the reader: img, sb, group, inode, blockmap, extent, map,
            dir, dx, hash, xattr, journal, bitmap, check, csum, crc,
            show, err, main
t/run.sh    the suite: builds the corpus, runs every comparison,
            prints the shape census and the totals
t/corpus.sh mke2fs, tune2fs, debugfs and e2fsck -fD build 37 images
t/cmp_*.py  one comparison each, against dumpe2fs or debugfs
t/corrupt.py the damaged images and the refusal contract
t/fuzz.py   the single-bit fault sweep
t/mutants.py the mutation table, and --check for its patterns
t/overread.sh the planted one-past-the-end reads
```

`make test` takes about a minute and builds roughly 3 GB of images in
a `mktemp -d` scratch directory. `EXTV_KEEP_FAILED=1` keeps them when
a run fails, which is how the one flake this project has had was
caught: mke2fs picks a random directory hash seed per image, so a
disagreement that depends on where a name lands moves between images
every time the corpus is rebuilt.
