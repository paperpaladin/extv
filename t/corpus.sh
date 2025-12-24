#!/bin/sh
# Build the corpus of filesystem images and print a manifest on stdout:
#
#   name <tab> path <tab> fstype <tab> blocksize <tab> inodesize <tab> tags
#
# Everything here is built by mke2fs, tune2fs and e2fsck, which is the
# point: the corpus is the one part of this that extv controls, so its
# shape is counted and printed rather than described. A corpus that is all
# 4096-byte ext4 with default features proves one layout.
#
# Progress and warnings go to stderr so the manifest stays parseable.
set -u

dir=$1
src=$dir/src
big=$dir/bigsrc
list=$dir/manifest

mkdir -p "$dir"
python3 "$(dirname "$0")/content.py" "$src" >/dev/null || exit 1
: > "$list"

# One directory with enough entries to push a hash tree to a second level.
# Built separately because 6000 inodes is the slowest part of the corpus
# and only one image needs them.
mkbig() {
	mkdir -p "$big/hugedir"
	i=0
	while [ $i -lt 8000 ]; do
		echo x > "$big/hugedir/h$i"
		i=$((i + 1))
	done
}

# mk NAME SIZE FSTYPE BSIZE ISIZE TAGS [extra mke2fs args...]
mk() {
	name=$1; size=$2; fstype=$3; bsize=$4; isize=$5; tags=$6
	shift 6
	img=$dir/$name.img
	rm -f "$img"
	truncate -s "$size" "$img" || exit 1
	srcdir=$src
	case ",$tags," in *,hugedir,*) srcdir=$big ;; esac
	if ! mke2fs -q -F -t "$fstype" -b "$bsize" -I "$isize" \
	     -d "$srcdir" "$@" "$img" >&2 2>"$dir/mke2fs.err"; then
		echo "corpus: mke2fs failed for $name:" >&2
		sed 's/^/  /' "$dir/mke2fs.err" >&2
		exit 1
	fi
	case ",$tags," in
	*,htree,*)
		# mke2fs does not build hash trees. e2fsck -fD rebuilds every
		# directory and indexes the ones that need it, which is the
		# only way to get a hashed directory without mounting.
		e2fsck -fDy "$img" >/dev/null 2>&1
		;;
	esac
	# An uninitialised extent: allocated and never written. mke2fs
	# cannot produce one, and without it the top bit of ee_len is
	# never set anywhere in the corpus, so a reader that ignores it
	# agrees with everything. debugfs fallocate is the only way to
	# get one without a mount.
	case ",$tags," in
	*,extent,*)
		debugfs -w -R "fallocate /empty 0 99" "$img" >/dev/null 2>&1
		debugfs -w -R "fallocate /b4096 20 29" "$img" >/dev/null 2>&1
		;;
	esac
	# Extended attributes. A short value fits in the inode when the inode
	# is 256 bytes and has nowhere to go but a block when it is 128, so
	# both paths are covered by the same two commands across the corpus.
	debugfs -w -R "ea_set /small user.one firstvalue" "$img" >/dev/null 2>&1
	debugfs -w -R "ea_set /small user.two second" "$img" >/dev/null 2>&1
	debugfs -w -R "ea_set -f $dir/eabig /b1023 user.big" "$img" >/dev/null 2>&1
	# A corpus image that is already inconsistent makes every later
	# agreement meaningless, so this is checked at build time and is
	# fatal rather than counted.
	if ! e2fsck -fn "$img" >"$dir/fsck.out" 2>&1; then
		echo "corpus: $name is not clean when built:" >&2
		sed 's/^/  /' "$dir/fsck.out" >&2
		exit 1
	fi
	printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$img" "$fstype" "$bsize" "$isize" "$tags" >> "$list"
	echo "corpus: $name" >&2
}

# A value too big for any inode and too big for a 1024-byte block to hold
# twice, so it lands in an attribute block of its own.
python3 -c "import sys; sys.stdout.write('v' * 400)" > "$dir/eabig"

mk ext2-1024-128        64M  ext2 1024 128 plain
mk ext2-2048-128        64M  ext2 2048 128 plain
mk ext2-4096-256        64M  ext2 4096 256 plain
mk ext2-1024-256-nosparse 64M ext2 1024 256 nosparse -O ^sparse_super,^resize_inode
mk ext2-1024-128-nodirindex 64M ext2 1024 128 nodirindex -O ^dir_index
mk ext2-1024-128-htree  64M  ext2 1024 128 htree
mk ext3-1024-128        64M  ext3 1024 128 journal
mk ext3-2048-256        64M  ext3 2048 256 journal
mk ext3-4096-256        64M  ext3 4096 256 journal
mk ext4-1024-256        64M  ext4 1024 256 journal,extent,64bit,csum
mk ext4-2048-256        64M  ext4 2048 256 journal,extent,64bit,csum
mk ext4-4096-256        64M  ext4 4096 256 journal,extent,64bit,csum
mk ext4-4096-128        64M  ext4 4096 128 extent,64bit,csum
mk ext4-1024-256-nojournal 64M ext4 1024 256 extent,64bit,csum -O ^has_journal
mk ext4-1024-256-noextent 64M ext4 1024 256 noextent -O ^extent,^64bit
mk ext4-2048-128-noextent-nocsum 64M ext4 2048 128 noextent,nocsum \
	-O ^extent,^64bit,^metadata_csum
mk ext4-1024-256-no64bit 64M ext4 1024 256 extent,csum -O ^64bit
mk ext4-1024-256-nocsum 64M  ext4 1024 256 extent,64bit -O ^metadata_csum
# uninit_bg is the older group checksum, a crc16 over the UUID and the
# descriptor rather than a crc32c. Without an image that has it and not
# metadata_csum, that branch is never executed and agrees with anything.
mk ext4-1024-256-uninitbg 64M ext4 1024 256 uninitbg,extent,64bit \
	-O ^metadata_csum,uninit_bg
mk ext4-1024-256-uninitbg-32 64M ext4 1024 256 uninitbg,extent \
	-O ^metadata_csum,^64bit,uninit_bg
mk ext4-4096-256-bigalloc 64M ext4 4096 256 bigalloc,extent,64bit,csum \
	-O bigalloc -C 16384
mk ext4-1024-256-inline 64M  ext4 1024 256 inline,extent,64bit,csum -O inline_data
mk ext4-1024-256-metabg 64M  ext4 1024 256 metabg,extent,64bit,csum \
	-O meta_bg,^resize_inode
mk ext4-1024-256-flex1  64M  ext4 1024 256 extent,64bit,csum -G 1
mk ext4-1024-256-htree  64M  ext4 1024 256 htree,extent,64bit,csum
mk ext4-4096-256-htree  64M  ext4 4096 256 htree,extent,64bit,csum
mk ext4-1024-256-manygroups 256M ext4 1024 256 manygroups,extent,64bit,csum
mk ext4-4096-256-big    512M ext4 4096 256 extent,64bit,csum

# The two non-default hash functions. tune2fs sets the algorithm and
# e2fsck -fD then rebuilds every index with it.
for alg in tea legacy; do
	name=ext4-1024-256-htree-$alg
	img=$dir/$name.img
	rm -f "$img"
	truncate -s 64M "$img"
	mke2fs -q -F -t ext4 -b 1024 -I 256 -d "$src" "$img" >&2 || exit 1
	tune2fs -E hash_alg=$alg "$img" >/dev/null || exit 1
	e2fsck -fDy "$img" >/dev/null 2>&1
	if ! e2fsck -fn "$img" >"$dir/fsck.out" 2>&1; then
		echo "corpus: $name is not clean when built" >&2
		exit 1
	fi
	printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$img" ext4 1024 256 "htree,$alg" >> "$list"
	echo "corpus: $name" >&2
done

mkbig
mk ext4-1024-256-htree2 512M ext4 1024 256 htree,hugedir,extent,64bit,csum -N 40000

cat "$list"
