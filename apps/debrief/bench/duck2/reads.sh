#!/bin/bash
# Query benchmark, 1.5.6 against 2.0, interleaved: builds both bench_reads binaries (release), creates one 10M-event
# store (copies of the samples, 360 x 28k events), then runs A B A B A B and logs the load before each run.
set -u
D=/Volumes/d7sst7/dev-targets/duck2-bench; OUT=$D/reads; mkdir -p $OUT
W2=/Volumes/d7sst7/dev-targets/wt/debrief-duckdb2/apps/debrief; W1=/Volumes/d7sst7/dev-targets/wt/debrief-156-baseline/apps/debrief
L1=/Volumes/d7sst7/dev-targets/debrief-libduckdb/1.5.6/libduckdb-osx-universal; L2=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
T1=/Volumes/d7sst7/dev-targets/debrief-156-target-rel; T2=/Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel
export CARGO_BUILD_JOBS=2
build(){ (cd $1/crates/debrief-store-duck && DUCKDB_LIB_DIR=$2 DUCKDB_INCLUDE_DIR=$2 CARGO_TARGET_DIR=$3 cargo test --release --test bench_reads --no-run 2>&1 | tail -1); }
bin(){ ls -t $1/release/deps/bench_reads-* | grep -v '\.d$' | head -1; }
build $W2 $L2 $T2 | tee $OUT/build2.txt
build $W1 $L1 $T1 | tee $OUT/build1.txt
B1=$(bin $T1); B2=$(bin $T2)
STORE=$D/store10m
if [ ! -f $STORE/data.duckdb ]; then
  DYLD_LIBRARY_PATH=$L2 DEBRIEF_BENCH_READS=360 DEBRIEF_BENCH_STORE=$STORE $B2 --nocapture > $OUT/import.txt 2>&1
fi
for r in 1 2 3; do
  for v in 156 200; do
    if [ $v = 156 ]; then B=$B1; L=$L1; else B=$B2; L=$L2; fi
    echo "run $r v$v load $(uptime | sed 's/.*averages: //')" >> $OUT/runs.txt
    DYLD_LIBRARY_PATH=$L DEBRIEF_BENCH_STORE=$STORE $B --nocapture > $OUT/v$v-$r.txt 2>&1
    echo "done $(uptime | sed 's/.*averages: //')" >> $OUT/runs.txt
  done
done
echo ALL-DONE >> $OUT/runs.txt
