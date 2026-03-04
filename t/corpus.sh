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
	# A journal that has been used. Nothing here mounts anything, so
	# the journal of a fresh image has no features, sequence 1 and
	# start 0, and the feature decode would never run. debugfs can
	# open the journal and commit a transaction through it, which
	# sets journal_64bit, which is the journal's own copy of the
	# filesystem's 64bit feature, so the image has to be a 64bit one
	# for anything to appear. It is the only journal state reachable
	# without a mount, and the README says so.
	case ",$tags," in
	*,jused,*)
		printf 'journal_open\nmkdir /jdir\njournal_close\n' > "$dir/jcmds"
		debugfs -w -f "$dir/jcmds" "$img" >/dev/null 2>&1
		;;
	esac
	# Garbage in the inode bitmap of a group marked INODE_UNINIT.
	# That bitmap is not read by anything: e2fsck still calls the
	# image clean and dumpe2fs still prints the free inode list it
	# computes from the group's layout rather than the bytes on
	# disk. Without an image like this, a reader that ignored the
	# INODE_UNINIT flag agreed with the oracle anyway, because
	# mke2fs leaves those bitmap blocks zeroed, which is the same
	# answer. It was a surviving mutant until this went in.
	case ",$tags," in
	*,uninitgarbage,*)
		ug_blk=$(dumpe2fs "$img" 2>/dev/null |
			awk '/^Group 1:/ {f = 1} f && /Inode bitmap at/ {print $4; exit}')
		if [ -z "$ug_blk" ]; then
			echo "corpus: $name has no group 1 inode bitmap" >&2
			exit 1
		fi
		head -c "$bsize" /dev/zero | tr '\000' '\377' > "$dir/ff.bin"
		dd if="$dir/ff.bin" of="$img" bs="$bsize" seek="$ug_blk" \
			count=1 conv=notrunc status=none || exit 1
		;;
	esac
	# A hole in the middle of the allocated blocks. Without one every
	# free list in the corpus is a single run reaching the end of the
	# group, and dumpe2fs prints a one-block run as a bare number and
	# a longer one as a range, so one of the two spellings would never
	# be compared. On the bigalloc image the same deletion frees a
	# single cluster, which dumpe2fs prints as a range because the
	# cluster is four blocks wide.
	case ",$tags," in
	*,gap,*)
		debugfs -w -R "rm /b1025" "$img" >/dev/null 2>&1
		;;
	esac
	# An empty directory block, which is the only place a rec_len of a
	# whole block occurs. expand_dir adds a block to a directory and
	# leaves it empty, which is legal and which e2fsck -fn accepts.
	case ",$tags," in
	*,bigblock,*)
		debugfs -w -R "expand_dir /emptydir" "$img" >/dev/null 2>&1
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
mk ext2-4096-256        64M  ext2 4096 256 plain,gap
mk ext2-1024-256-nosparse 64M ext2 1024 256 nosparse -O ^sparse_super,^resize_inode
mk ext2-1024-128-nodirindex 64M ext2 1024 128 nodirindex -O ^dir_index
mk ext2-1024-128-htree  64M  ext2 1024 128 htree
mk ext3-1024-128        64M  ext3 1024 128 journal
mk ext3-2048-256        64M  ext3 2048 256 journal
mk ext3-4096-256        64M  ext3 4096 256 journal
mk ext4-1024-256        64M  ext4 1024 256 journal,uninitgarbage,extent,64bit,csum
mk ext4-2048-256        64M  ext4 2048 256 journal,jused,extent,64bit,csum
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
mk ext4-4096-256-bigalloc 64M ext4 4096 256 bigalloc,gap,extent,64bit,csum \
	-O bigalloc -C 16384
mk ext4-1024-256-inline 64M  ext4 1024 256 inline,extent,64bit,csum -O inline_data
mk ext4-1024-256-metabg 64M  ext4 1024 256 metabg,extent,64bit,csum \
	-O meta_bg,^resize_inode
mk ext4-1024-256-flex1  64M  ext4 1024 256 extent,64bit,csum -G 1
mk ext4-1024-256-htree  64M  ext4 1024 256 htree,extent,64bit,csum
mk ext4-4096-256-htree  64M  ext4 4096 256 htree,extent,64bit,csum
# 65536-byte blocks. mke2fs warns that the block is too big for a
# machine with 4096-byte pages and builds the image anyway, and
# dumpe2fs, debugfs and e2fsck all read it, so it is an oracle-backed
# image rather than a hand-forged one. It is the only block size where
# a directory entry can be longer than the sixteen-bit rec_len field,
# and the only one where a block offset needs more than sixteen bits.
#
# The empty block is the point of the second of the two. A whole-block
# rec_len only occurs in a directory block with nothing in it, and
# debugfs expand_dir is the way to get one without a mount. With
# metadata_csum the block is not empty: the twelve-byte checksum tail
# takes rec_len to 65524, which fits. So the image that reaches the
# encoding is the one without metadata_csum, and the one with it is
# kept as the other half of the pair.
mk ext4-65536-256 128M ext4 65536 256 bigblock,extent,64bit,csum
mk ext4-65536-256-nocsum 128M ext4 65536 256 bigblock,extent,64bit,nocsum \
	-O ^metadata_csum
mk ext4-1024-256-manygroups 256M ext4 1024 256 manygroups,extent,64bit,csum
mk ext4-4096-256-big    512M ext4 4096 256 extent,64bit,csum

# The hash functions, and the signedness the filesystem records. The
# algorithm comes from tune2fs and the signed or unsigned flag from a
# direct superblock edit; e2fsck -fD then rebuilds every index with
# that combination. mke2fs on this machine writes signed_directory_hash
# and nothing else, so without the edit half the hash code would never
# run.
for spec in tea,1 legacy,1 half_md4,2 tea,2 legacy,2; do
	alg=${spec%,*}
	flagval=${spec#*,}
	case $flagval in
	1) sign=signed ;;
	*) sign=unsigned ;;
	esac
	name=ext4-1024-256-htree-$alg-$sign
	img=$dir/$name.img
	rm -f "$img"
	truncate -s 64M "$img"
	mke2fs -q -F -t ext4 -b 1024 -I 256 -d "$src" "$img" >&2 || exit 1
	tune2fs -E hash_alg=$alg "$img" >/dev/null || exit 1
	debugfs -w -R "ssv flags $flagval" "$img" >/dev/null 2>&1
	e2fsck -fDy "$img" >/dev/null 2>&1
	if ! e2fsck -fn "$img" >"$dir/fsck.out" 2>&1; then
		echo "corpus: $name is not clean when built" >&2
		sed 's/^/  /' "$dir/fsck.out" >&2
		exit 1
	fi
	printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$img" ext4 1024 256 \
		"htree,$alg,$sign" >> "$list"
	echo "corpus: $name" >&2
done

mkbig
mk ext4-1024-256-htree2 512M ext4 1024 256 htree,hugedir,extent,64bit,csum -N 40000

cat "$list"
