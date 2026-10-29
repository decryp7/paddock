#!/bin/bash
# usage: ab.sh LABEL VARIANT(156|200) CORPUS_DIR   -- real pipeline: release CLI + store worker, sampled with procsample.py
set -u
LABEL=$1; V=$2; CORPUS=$3
D=/Volumes/d7sst7/dev-targets/research-data; OUT=/Volumes/d7sst7/dev-targets/duck2-bench/results; mkdir -p $OUT
if [ "$V" = 156 ]; then B=/Volumes/d7sst7/dev-targets/perf-debrief-target/release; L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/1.5.6/libduckdb-osx-universal
else B=/Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel/release; L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64; fi
export DEBRIEF_LIB_DIR=$L DEBRIEF_STORE_WORKER=$B/debrief-store-worker DYLD_LIBRARY_PATH=$L
STORE=/Volumes/d7sst7/dev-targets/duck2-bench/st-$LABEL; rm -rf $STORE
t0=$(python3 -c 'import time;print(time.time())')
"$B/debrief" import "$CORPUS" --store "$STORE" --json > $OUT/$LABEL.json 2> $OUT/$LABEL.err &
CLI=$!
python3 -I $D/scripts/procsample.py $CLI $OUT/$LABEL.csv 0.25 &
SAMP=$!
wait $CLI; rc=$?; wait $SAMP
t1=$(python3 -c 'import time;print(time.time())')
echo "$LABEL rc=$rc wall=$(python3 -c "print(round($t1-$t0,1))")s load=$(uptime | sed 's/.*averages: //')" | tee -a $OUT/summary.txt
python3 -I $D/scripts/perfsum.py $OUT/$LABEL.csv 2>&1 | head -8 | tee -a $OUT/summary.txt
rm -rf $STORE
