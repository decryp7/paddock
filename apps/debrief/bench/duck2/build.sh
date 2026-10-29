#!/bin/bash
# usage: build.sh OUT_NAME -- pauses exp.sh, waits for the current measured run to end, builds import_ready, copies it to prof/bin/OUT_NAME.
D=/Volumes/d7sst7/dev-targets/duck2-bench; touch $D/pause
while [ -e $D/running ]; do sleep 5; done
$D/waitload.sh 25 3000 >/dev/null
cd /Volumes/d7sst7/dev-targets/wt/debrief-duckdb2/apps/debrief/crates/debrief-store-duck
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
out=$(DUCKDB_LIB_DIR=$L DUCKDB_INCLUDE_DIR=$L CARGO_TARGET_DIR=/Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel CARGO_BUILD_JOBS=2 nice cargo test --release --test import_ready --no-run 2>&1)
rc=$?; echo "$out" | grep -E "^(error|warning)" -A6 | head -40
B=$(echo "$out" | sed -n 's/.*Executable .*(\(.*\))/\1/p'); [ $rc = 0 ] && cp "$B" $D/prof/bin/$1 && echo "built $1"
rm -f $D/pause; exit $rc
