#!/bin/bash
# usage: perf-post.sh LABEL STORE_DIR [IDLE_S] : reopens the store with the perf_idle harness and samples it.
set -u
LABEL=$1; STORE=$2; IDLE=${3:-60}
D=/Volumes/d7sst7/dev-targets/research-data; OUT=$D/results/perf-2026-10-28
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/1.5.6/libduckdb-osx-universal
T=$(ls -t /Volumes/d7sst7/dev-targets/perf-debrief-target/release/deps/perf_idle-* | grep -v '\.d$' | head -1)
export DYLD_LIBRARY_PATH=$L DEBRIEF_PERF_STORE=$STORE DEBRIEF_PERF_IDLE_S=$IDLE
cd /Volumes/d7sst7/dev-targets/wt/perf-debrief/apps/debrief/crates/debrief-store-duck
uptime > "$OUT/$LABEL-post.uptime"
"$T" --nocapture --test-threads 1 > "$OUT/$LABEL-post.log" 2>&1 &
P=$!
python3 -I $D/scripts/procsample.py $P "$OUT/$LABEL-post.csv" 0.25 &
S=$!
wait $P; echo "rc=$?"; wait $S
uptime >> "$OUT/$LABEL-post.uptime"
grep -v '^PHASE\|^test \|^$\|^running' "$OUT/$LABEL-post.log"
python3 -I $D/scripts/perfsum.py "$OUT/$LABEL-post.csv" "$OUT/$LABEL-post.log"
