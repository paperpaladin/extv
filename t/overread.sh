#!/bin/sh
# One planted one-past-the-end read, and the panic it has to produce.
#
# Run through `make overread`, which builds extv with the plant
# compiled in and then calls this. METHOD records eighteen buffers in
# the sibling C repositories that were larger than their data, which
# turns the sanitizer off over everything past the real end. Rust is
# not exempt: a Vec with spare capacity, or a block buffer sized to
# the largest block rather than to this filesystem's, is indexed
# against its own length and a read past the end of the structure
# lands quietly inside it.
#
# So every structure here is read into a buffer of exactly its own
# size, and this is the check that the consequence holds. Each plant
# reads one byte past the end of one of those buffers. With exact
# sizing that is a panic and exit 101; with a rounded-up buffer it is
# a zero and exit 0, and the gate goes red, which is the direction
# that was watched.
set -u

plant=$1
PATH=$PATH:/sbin:/usr/sbin
export PATH
EXTV=target/release/extv

for tool in mke2fs; do
	if ! command -v $tool >/dev/null 2>&1; then
		echo "overread: $tool is not installed, so no image can be built"
		exit 2
	fi
done

scratch=$(mktemp -d "${TMPDIR:-/tmp}/extv-overread.XXXXXX") || exit 2
trap 'rm -rf "$scratch"' EXIT INT TERM

img=$scratch/o.img
mkdir -p "$scratch/src/d"
echo hello > "$scratch/src/d/f"
truncate -s 8M "$img" || exit 2
# 1024-byte blocks and 128-byte inodes, because the plants read one
# byte past a block and one byte past an inode and the smaller both
# are, the further the planted read is from the end of anything else.
if ! mke2fs -q -F -t ext2 -b 1024 -I 128 -d "$scratch/src" "$img" >/dev/null 2>&1; then
	echo "overread: mke2fs failed"
	exit 2
fi

case $plant in
plant_overread_block)  set -- groups "$img" ;;
plant_overread_dirent) set -- ls "$img" 2 ;;
plant_overread_inode)  set -- stat "$img" 2 ;;
*)
	echo "overread: no command for plant $plant"
	exit 2
	;;
esac

"./$EXTV" "$@" > "$scratch/out" 2> "$scratch/err"
rc=$?

# 101 is Rust's panic status, and extv never exits 101 by any other
# route: it uses 0, 1, 2 and 3, and err.rs turns a closed pipe into 0
# for exactly this reason, so a report here cannot be confused with
# `extv groups img | head`.
if [ "$rc" != 101 ]; then
	echo "FAIL $plant: extv exited $rc, wanted 101 (a panic)"
	echo "     the planted read past the end of the buffer was not caught,"
	echo "     which means the buffer is bigger than the structure in it"
	sed 's/^/     /' "$scratch/err" | head -3
	exit 1
fi
if ! grep -q 'index out of bounds' "$scratch/err"; then
	echo "FAIL $plant: panicked, but not on a bounds check"
	sed 's/^/     /' "$scratch/err" | head -3
	exit 1
fi
echo "ok   $plant: the read one byte past the buffer panicked on the bound"
