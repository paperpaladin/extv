CARGO ?= cargo

# --remap-path-prefix is the Rust spelling of the -ffile-prefix-map rule the
# C repos here follow: debug info is on in the release profile, and without
# this the build directory ends up inside the binary, so two checkouts of the
# same commit produce different bytes.
export RUSTFLAGS = --remap-path-prefix=$(CURDIR)=.

BIN = target/release/extv

all: $(BIN)

$(BIN): $(wildcard src/*.rs) Cargo.toml
	$(CARGO) build --release

test: all
	./t/run.sh

clean:
	$(CARGO) clean
	rm -rf t/scratch

# A buffer larger than its data turns the bounds check off. Rust checks every
# index, but only against the length of the slice it is given: a block buffer
# allocated at 4096 and handed to code that should stop at a 1024-byte block
# has exactly the blind spot the C repos here found eighteen times. So every
# block is read into a Vec of exactly block_size bytes, straight from the
# file, rather than sliced out of one image-sized buffer, and this target
# asserts the consequence instead of trusting it.
#
# Each plant reads one byte past the end of a buffer. With exact sizing that
# is a panic; with a rounded-up buffer it is a quiet zero. Exit 101 is Rust's
# panic status and extv never exits 101 by any other route (it uses 0, 1, 2
# and 3), so the gate cannot confuse a report with a refusal. The message is
# checked too, because a panic from some unrelated unwrap would otherwise
# count as a pass.
overread:
	@set -e; for plant in plant_overread_block plant_overread_dirent \
	                      plant_overread_inode; do \
		$(CARGO) build --release --features $$plant -q; \
		./t/overread.sh $$plant; \
	done; $(CARGO) build --release -q

.PHONY: all test clean overread
