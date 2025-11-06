#!/bin/sh
# extv test suite. Run it through `make test`, never directly: the make
# target rebuilds first, and a suite run against yesterday's binary has
# reported mutants as surviving in three sibling repos.
#
# The last line is always of the form "N passed, M failed".
set -u

EXTV=${EXTV:-./target/release/extv}
pass=0
fail=0

ok() { pass=$((pass + 1)); echo "ok   $1"; }
no() { fail=$((fail + 1)); echo "FAIL $1"; }

check_exit() {
	# check_exit NAME WANT CMD...
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

check_exit "usage with no arguments" 3 "$EXTV"
check_exit "usage for an unknown command" 3 "$EXTV" nosuchcommand

echo
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
