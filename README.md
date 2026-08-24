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

659 assertions over 36 images, from the run of 20 October 2026. The
suite prints these totals itself; they are not maintained by hand.

| | |
|---|---|
| superblock fields against dumpe2fs | 1,621 |
| group descriptor fields | 3,704 across 284 groups |
| inode fields against debugfs stat | 365,082 over 21,600 inodes |
| inode checksums recomputed | 12,182 |
| group descriptor checksums recomputed | 249 |
| block numbers against debugfs | 199,477 in 50,966 runs |
| extent tree rows against dump_extents | 14,580 |
| directory entries against debugfs ls -l | 27,784 in 284 directories |
| names hashed against debugfs htree_dump | 12,168 in 9 indexed directories |
| extended attributes against ea_list and ea_get | 20,392, 625,360 value bytes |
| file bytes against debugfs dump | 60,298,105 over 544 files |
| journal superblock fields | 203 over 29 images with a journal |
| free block and free inode runs against dumpe2fs | 551 |
| consistency counters checked by `extv check` | 1,151 |
| link counts checked against the names that make them | 27,181 |

## The corpus

A corpus that is all 4096-byte-block default ext4 proves one layout,
so the suite prints what it actually holds on every run:

```
type: ext4=27 ext3=3 ext2=6
block size: 1024=23 2048=4 4096=7 65536=2
inode size: 128=7 256=29
tags: extent=20 64bit=18 csum=16 htree=9 journal=6 plain=3
      noextent=2 nocsum=2 uninitbg=2 bigblock=2 orphan=2 gap=2
      signed=2 unsigned=3 legacy=2 tea=2 half_md4=1
      bigalloc=1 inline=1 metabg=1 manygroups=1 nosparse=1
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
spellings dumpe2fs uses for a free run, and an image with no journal
at all.

## Damaged images

49 cases, built by breaking one thing in a clean image: 37 refusals,
12 inconsistencies, 0 wrong answers. Refusals are counted apart from
wrong answers because they are different results, and a run that
reported them together would let one hide inside the other.

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

One case is a deliberate difference and is counted as one: an orphan
list head past the end of the inode table. e2fsck leaves the orphan
list alone on a cleanly unmounted image and sees nothing; extv refuses
to follow a pointer out of the table.

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

Watched in both directions. Making `img.block` read 4096 bytes instead
of one block leaves the block plant silent and the target red; making
`inode::read` take 256 bytes instead of the inode size does the same
to the inode plant and leaves the other two passing.

## make mutants

Twenty-three mutations, one decode rule broken at a time, each with
the whole suite run against it. The table in `t/mutants.py` records
which are expected to survive and why, and the target fails when
reality disagrees with the table in either direction, so a mutant
that starts dying is as much a failure as one that starts surviving.

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

There is no fuzzer. The damaged images are 49 faults placed by hand,
not a search.

## Layout

```
src/        the reader: img, sb, group, inode, blockmap, extent, map,
            dir, dx, hash, xattr, journal, bitmap, check, csum, crc,
            show, err, main
t/run.sh    the suite: builds the corpus, runs every comparison,
            prints the shape census and the totals
t/corpus.sh mke2fs, tune2fs, debugfs and e2fsck -fD build 36 images
t/cmp_*.py  one comparison each, against dumpe2fs or debugfs
t/corrupt.py the damaged images and the refusal contract
t/mutants.py the mutation table
t/overread.sh the planted one-past-the-end reads
```

`make test` takes about a minute and builds roughly 3 GB of images in
a `mktemp -d` scratch directory. `EXTV_KEEP_FAILED=1` keeps them when
a run fails, which is how the one flake this project has had was
caught: mke2fs picks a random directory hash seed per image, so a
disagreement that depends on where a name lands moves between images
every time the corpus is rebuilt.
