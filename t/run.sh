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

ok() { pass=$((pass + 1)); echo "ok   $1"; }
no() { fail=$((fail + 1)); echo "FAIL $1"; }

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
trap 'rm -rf "$scratch"' EXIT INT TERM

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
	pass=$((pass + _h_p))
	fail=$((fail + _h_f))
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

echo
echo "--- measured totals"
awk '{t[$2] += $3} END {for (k in t) printf "     %s %d\n", k, t[k]}' \
	"$scratch/counts" | sort

echo
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
