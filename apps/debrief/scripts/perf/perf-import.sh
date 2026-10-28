#!/bin/bash
# usage: perf-import.sh LABEL CORPUS_DIR STORE_DIR
# Imports CORPUS_DIR with the release CLI and store worker built from the perf-debrief worktree, sampling the CLI and
# its children (store worker, plugin processes) every 250 ms with procsample.py (footprint, RSS, CPU, wake-ups).
set -u
LABEL=$1; CORPUS=$2; STORE=$3
D=/Volumes/d7sst7/dev-targets/research-data; B=/Volumes/d7sst7/dev-targets/perf-debrief-target/release
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/1.5.6/libduckdb-osx-universal
export DEBRIEF_LIB_DIR=$L DEBRIEF_STORE_WORKER=$B/debrief-store-worker DYLD_LIBRARY_PATH=$L
OUT=$D/results/perf-2026-10-28; mkdir -p "$OUT"
rm -rf "$STORE"
uptime > "$OUT/$LABEL.uptime"
t0=$(python3 -c 'import time;print(time.time())')
"$B/debrief" import "$CORPUS" --store "$STORE" --json > "$OUT/$LABEL.json" 2> "$OUT/$LABEL.err" &
CLI=$!
python3 -I $D/scripts/procsample.py $CLI "$OUT/$LABEL.csv" 0.25 &
SAMP=$!
wait $CLI; rc=$?
wait $SAMP
t1=$(python3 -c 'import time;print(time.time())')
echo "$LABEL rc=$rc wall=$(python3 -c "print(round($t1-$t0,1))")s store=$(du -sh "$STORE" | cut -f1)" | tee -a "$OUT/summary.txt"
uptime >> "$OUT/$LABEL.uptime"
