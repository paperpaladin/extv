#!/bin/sh
# extv test suite. Run it through `make test`, never directly: the make
# target rebuilds first, and a suite run against yesterday's binary has
# reported mutants as surviving in three sibling repos.
#
# The last line is always of the form "N passed, M failed".
set -u

here=$(dirname "$0")
# e2fsprogs installs into /sbin on most distributions and is not always on
# an interactive PATH.
PATH=$PATH:/sbin:/usr/sbin
export PATH

EXTV=${EXTV:-./target/release/extv}
pass=0
fail=0
skip=0

ok() { pass=$((pass + 1)); echo "ok   $1"; }
no() { fail=$((fail + 1)); echo "FAIL $1"; }
# A skip is loud and counted. It is for a comparison the oracle can
# make and extv cannot yet, which is a different thing from a passing
# test and must not be added to one.
skipped() { skip=$((skip + 1)); echo "SKIP $1"; }

# Shell functions have no local scope, so every variable in one of these
# is prefixed. The first version assigned to "name", which is also the
# loop variable the caller is iterating with, and the suite spent a run
# reporting "ext4-1024-256 super exits 0 super (48 fields)" as the name
# of an image.
check_exit() {
	_ce_name=$1; _ce_want=$2; shift 2
	"$@" >/dev/null 2>&1
	_ce_got=$?
	if [ "$_ce_got" = "$_ce_want" ]; then
		ok "$_ce_name"
	else
		no "$_ce_name (exit $_ce_got, want $_ce_want)"
	fi
}

if [ ! -x "$EXTV" ]; then
	echo "no binary at $EXTV; run make"
	echo "0 passed, 1 failed"
	exit 1
fi
if [ "$EXTV" -ot src/main.rs ] 2>/dev/null; then
	echo "$EXTV is older than src/main.rs: run make, not t/run.sh."
	echo "0 passed, 1 failed"
	exit 1
fi

# e2fsprogs is the only oracle for every fact this suite checks, and the
# only way to build an image. Without it the run would print a number
# having compared nothing, so it refuses instead, the way corex does when
# gdb is missing.
for tool in mke2fs dumpe2fs debugfs e2fsck tune2fs; do
	if ! command -v $tool >/dev/null 2>&1; then
		echo "extv: $tool is not installed."
		echo "e2fsprogs is the only oracle here and the only way to build"
		echo "a corpus, so this suite refuses rather than report 0 failed."
		exit 2
	fi
done

scratch=$(mktemp -d "${TMPDIR:-/tmp}/extv.XXXXXX") || exit 2
# The corpus is about 3 GB, so it goes away at the end. With
# EXTV_KEEP_FAILED=1 a failing run leaves it behind instead: a
# disagreement with an oracle is about one image, and rebuilding the
# corpus to look at it rebuilds a different one if anything in the
# build is not deterministic. That is exactly the case a loop chasing
# an intermittent failure cannot afford to lose.
cleanup() {
	if [ "${EXTV_KEEP_FAILED:-0}" != 0 ] && [ "$fail" != 0 ]; then
		echo "corpus kept at $scratch"
		return
	fi
	rm -rf "$scratch"
}
trap cleanup EXIT
trap 'rm -rf "$scratch"; exit 130' INT TERM

echo "--- oracles: $(dumpe2fs -V 2>&1 | head -1)"
echo "--- building the corpus in $scratch"
if ! "$here/corpus.sh" "$scratch" > "$scratch/manifest" 2> "$scratch/corpus.log"; then
	sed 's/^/     /' "$scratch/corpus.log"
	echo "corpus build failed"
	echo "0 passed, 1 failed"
	exit 1
fi
images=$(wc -l < "$scratch/manifest")
echo "--- corpus: $images images"

# The census. A number measured over a corpus is a number about that
# corpus, so the suite prints what it actually holds rather than leaving
# the reader to assume a spread.
census() {
	echo "--- corpus shapes"
	awk -F'\t' '{n[$3]++} END {printf "     type:"; for (k in n) printf " %s=%d", k, n[k]; print ""}' "$scratch/manifest"
	awk -F'\t' '{n[$4]++} END {printf "     block size:"; for (k in n) printf " %s=%d", k, n[k]; print ""}' "$scratch/manifest"
	awk -F'\t' '{n[$5]++} END {printf "     inode size:"; for (k in n) printf " %s=%d", k, n[k]; print ""}' "$scratch/manifest"
	awk -F'\t' '{split($6, t, ","); for (i in t) n[t[i]]++} END {printf "     tags:"; for (k in n) printf " %s=%d", k, n[k]; print ""}' "$scratch/manifest"
}
census

: > "$scratch/counts"

# Run a comparison helper and fold its result into the running totals.
# The DONE line is the point: a helper that dies halfway through prints
# neither ok nor FAIL for the work it did not reach, and the silence
# would otherwise read as nothing to report.
helper() {
	_h_name=$1
	shift
	"$@" > "$scratch/helper.out" 2> "$scratch/helper.err"
	_h_rc=$?
	grep -v '^COUNT \|^DONE' "$scratch/helper.out"
	_h_p=$(grep -c '^ok   ' "$scratch/helper.out")
	_h_f=$(grep -c '^FAIL ' "$scratch/helper.out")
	_h_s=$(grep -c '^SKIP ' "$scratch/helper.out")
	pass=$((pass + _h_p))
	fail=$((fail + _h_f))
	skip=$((skip + _h_s))
	grep '^COUNT ' "$scratch/helper.out" >> "$scratch/counts"
	if [ "$_h_rc" != 0 ] || ! grep -q '^DONE' "$scratch/helper.out"; then
		fail=$((fail + 1))
		echo "FAIL $_h_name: helper did not finish (exit $_h_rc)"
		sed 's/^/     /' "$scratch/helper.err"
	fi
}

echo "--- superblock against dumpe2fs -h"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	check_exit "$name super exits 0" 0 "$EXTV" super "$img"
	helper "$name" python3 "$here/cmp_super.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- block group descriptors against dumpe2fs"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_groups.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- inodes against debugfs stat"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_inodes.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- block maps and extent trees against debugfs"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_blocks.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- directories against debugfs ls -l"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_dirs.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- hash trees against debugfs htree_dump"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_htree.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- whole files against debugfs dump"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_cat.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- extended attributes against debugfs ea_list and ea_get"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_ea.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- extent trees against debugfs dump_extents"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_extents.py" "$EXTV" "$img" "$name" 600
done < "$scratch/manifest"

echo
echo "--- the orphan list against dumpe2fs"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_orphans.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- extv check against e2fsck -fn, clean direction"
# Half of the hard invariant. The other half, that extv calls a broken
# image broken, needs images that are broken on purpose and lives with
# the corrupt corpus below. Both exit statuses are required to agree,
# not just extv's: an e2fsck that has started refusing these images
# would otherwise turn into a silent pass here.
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	"$EXTV" check "$img" > "$scratch/check.out" 2>"$scratch/check.err"
	ec=$?
	e2fsck -fn "$img" > "$scratch/fsck.out" 2>&1
	fc=$?
	# An exit status of zero is not enough from e2fsck. It prints
	# "Fix? no" for every problem it finds under -n and for some of
	# them it still exits 0, so a clean exit with that line in the
	# output is e2fsck reporting a problem quietly.
	if grep -q 'Fix? no' "$scratch/fsck.out"; then
		fc=4
	fi
	if [ "$ec" = 0 ] && [ "$fc" = 0 ]; then
		ok "$name check: both extv and e2fsck call it clean"
		awk '/^summary/ {for (i = 2; i < NF; i += 2)
			printf "COUNT check_%s %s\n", $i, $(i + 1)}' \
			"$scratch/check.out" >> "$scratch/counts"
	else
		no "$name check: extv exit $ec, e2fsck exit $fc"
		head -4 "$scratch/check.out" | sed 's/^/     /'
		head -4 "$scratch/check.err" | sed 's/^/     /'
		grep -v '^Pass \|^$' "$scratch/fsck.out" | head -4 | sed 's/^/     /'
	fi
done < "$scratch/manifest"

echo
echo "--- damaged images: refusals counted apart from wrong answers"
# Two bases, because the shape of the damage depends on the layout:
# the 1024-byte-block image is the only one where the superblock is
# not in block 0, and the 4096-byte one is the only one with an extent
# tree to break.
corrupt_b1=$(awk -F'\t' '$1 == "ext4-4096-256" {print $2}' "$scratch/manifest")
corrupt_b2=$(awk -F'\t' '$1 == "ext2-1024-128" {print $2}' "$scratch/manifest")
# A third, because inline_data puts a file's contents in the inode and
# the only size bound on them is the inode's own, which no other base
# here has. The feature is not on in either of the two above.
corrupt_b3=$(awk -F'\t' '$1 == "ext4-1024-256-inline" {print $2}' "$scratch/manifest")
if [ -z "$corrupt_b1" ] || [ -z "$corrupt_b2" ] || [ -z "$corrupt_b3" ]; then
	no "corrupt: the corpus no longer holds the three base images"
else
	EXTV="$EXTV" helper corrupt python3 "$here/corrupt.py" "$scratch" \
		corrupt "$corrupt_b1" "$corrupt_b2" "$corrupt_b3"
fi

echo
echo "--- single-bit faults in the metadata, against e2fsck"
# Both layouts. The 1024-byte ext2 image has no metadata_csum, so a
# sweep over it alone never reaches the checksum comparisons, and
# deleting the inode checksum check from `extv check` survived until
# the ext4 image was added here.
if [ -n "$corrupt_b1" ] && [ -n "$corrupt_b2" ]; then
	EXTV="$EXTV" helper fuzz python3 "$here/fuzz.py" "$scratch" fuzz-ext2 \
		"$corrupt_b2" 60 20261020
	EXTV="$EXTV" helper fuzz python3 "$here/fuzz.py" "$scratch" fuzz-ext4 \
		"$corrupt_b1" 60 20261021
fi

echo
echo "--- block and inode bitmaps against the dumpe2fs free lists"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_bitmaps.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- the journal superblock against dumpe2fs"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	helper "$name" python3 "$here/cmp_journal.py" "$EXTV" "$img" "$name"
done < "$scratch/manifest"

echo
echo "--- metadata checksums recomputed"
: > "$scratch/forms"
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	if ! "$EXTV" csum "$img" > "$scratch/csum.out" 2> "$scratch/csum.err"; then
		no "$name csum: extv exited $? ($(head -1 "$scratch/csum.err"))"
		continue
	fi
	nbad=$(grep -c ' BAD$' "$scratch/csum.out")
	nok=$(grep -c ' ok$' "$scratch/csum.out")
	sed -n 's/^form //p' "$scratch/csum.out" >> "$scratch/forms"
	if [ "$nbad" != 0 ]; then
		no "$name csum: $nbad checksums do not match what mke2fs wrote"
		grep ' BAD$' "$scratch/csum.out" | head -3 | sed 's/^/     /'
	else
		ok "$name csum ($nok recomputed and matching)"
	fi
	echo "COUNT checksums $nok" >> "$scratch/counts"
	echo "COUNT bitmap_checksums $(grep -c '^blockbitmap\|^inodebitmap' \
		"$scratch/csum.out")" >> "$scratch/counts"
	echo "COUNT dirent_checksums $(grep -c '^direntcsum' \
		"$scratch/csum.out")" >> "$scratch/counts"
	echo "COUNT extent_block_checksums $(grep -c '^extentcsum' \
		"$scratch/csum.out")" >> "$scratch/counts"
done < "$scratch/manifest"

# Assert the paths ran. A crc16 implementation that no image reaches
# agrees with everything, and the corpus had exactly that shape until an
# image with uninit_bg and without metadata_csum was added.
for form in crc32c crc16 none; do
	n=$(grep -c "^$form\$" "$scratch/forms")
	if [ "$n" -gt 0 ]; then
		ok "corpus reaches the $form group descriptor checksum ($n images)"
	else
		no "corpus reaches the $form group descriptor checksum (0 images)"
	fi
done

# Assert the paths ran. An indirect map test on a corpus whose deepest
# file fits in twelve direct blocks passes without executing a line of
# the indirect code, and the same goes for the extent tree's interior
# nodes.
echo
echo "--- paths reached by the corpus"
for path in path_ind path_dind path_tind path_etb path_inline \
            fast_symlinks extent_uninit extent_rows_deep \
            ea_in_inode ea_in_block dirent_tails dirs_inline path_special \
            ea_prefix_user ea_prefix_trusted ea_prefix_security ea_prefix_system \
            dirent_wide_rec_len journal_none journal_features \
            bitmap_single_runs bitmap_range_runs bitmap_block_uninit \
            bitmap_inode_uninit inodes_deleted orphan_chain \
            bitmap_checksums special_inodes dirent_checksums \
            extent_block_checksums fuzz_refused fuzz_inconsistent \
            corrupt_shared corrupt_symlink_size corrupt_inline_size \
            cat_fast_symlink_targets cat_slow_symlink_targets \
            orphan_file_blocks orphan_lists_empty corrupt_refusals \
            htree_levels htree_version_0 htree_version_1 htree_version_2 \
            htree_version_3 htree_version_4 htree_version_5; do
	n=$(awk -v k="$path" '$2 == k {t += $3} END {print t + 0}' "$scratch/counts")
	if [ "$n" -gt 0 ]; then
		ok "corpus reaches $path ($n times)"
	else
		no "corpus reaches $path (never: that code is untested)"
	fi
done

echo
echo "--- the mutation table still quotes code that exists"
# The sweep in t/mutants.py is half an hour and nobody is present for
# it, so this asks the one question that rots on its own: does every
# pattern still appear exactly once in the file it names. One entry
# had been dead for several commits because a line was inserted into
# the branch it quoted, and a dead entry is neither killed nor
# surviving, it does nothing while the total still reads as complete.
#
# It has to stand down while the sweep is running, and that is not a
# convenience. A mutation is applied by replacing one of these exact
# patterns, so with this check in the suite every mutant failed it,
# every mutant was recorded as killed, and the three survivors the
# table expects were reported as "expected to survive and the suite
# caught it". Two full sweeps measured nothing except that the
# mutation had been applied. The skip is loud and counted, so a run
# with it set cannot be mistaken for a run that asked the question.
if [ "${EXTV_MUTATION_RUN:-0}" != 0 ]; then
	skipped "the mutation table's own patterns: a mutation is applied, so one of them is deliberately not in the tree"
else
	helper mutants python3 "$here/mutants.py" --check
fi

echo
echo "--- measured totals"
awk '{t[$2] += $3} END {for (k in t) printf "     %s %d\n", k, t[k]}' \
	"$scratch/counts" | sort

echo
[ "$skip" = 0 ] || echo "$skip skipped, each named above"
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
