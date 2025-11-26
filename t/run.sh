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

check_exit() {
	name=$1; want=$2; shift 2
	"$@" >/dev/null 2>&1
	got=$?
	if [ "$got" = "$want" ]; then ok "$name"; else no "$name (exit $got, want $want)"; fi
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

# Every image has to be readable at all before any field is worth
# comparing.
while IFS="$(printf '\t')" read -r name img fstype bsize isize tags; do
	check_exit "$name super exits 0" 0 "$EXTV" super "$img"
done < "$scratch/manifest"

echo
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
