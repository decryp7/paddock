#!/bin/bash
# usage: perf-import.sh LABEL CORPUS_DIR STORE_DIR
# Imports CORPUS_DIR (synthetic data only, e.g. from gen_large.py) with the release CLI and store worker, sampling the
# CLI and its children (store worker, plugin processes) every 250 ms with procsample.py (footprint, RSS, CPU, wake-ups).
# Environment: DEBRIEF_PERF_BIN (release target folder holding debrief and debrief-store-worker: the release CLI starts only
# the worker that sits in its own folder, DEBRIEF_STORE_WORKER is ignored there), DEBRIEF_LIB_DIR
# (libduckdb folder), DEBRIEF_PERF_OUT (results folder). macOS only (procsample.py reads proc_pid_rusage).
set -eu
LABEL=$1; CORPUS=$2; STORE=$3
HERE=$(cd "$(dirname "$0")" && pwd)
B=${DEBRIEF_PERF_BIN:?set DEBRIEF_PERF_BIN to the release target folder}
L=${DEBRIEF_LIB_DIR:?set DEBRIEF_LIB_DIR to the libduckdb folder}
OUT=${DEBRIEF_PERF_OUT:?set DEBRIEF_PERF_OUT to a results folder}
export DYLD_LIBRARY_PATH=$L  # the release CLI finds debrief-store-worker next to itself ($B)
mkdir -p "$OUT"
rm -rf "$STORE"
uptime > "$OUT/$LABEL.uptime"
t0=$(python3 -c 'import time;print(time.time())')
"$B/debrief" import "$CORPUS" --store "$STORE" --json > "$OUT/$LABEL.json" 2> "$OUT/$LABEL.err" &
CLI=$!
python3 -I "$HERE/procsample.py" $CLI "$OUT/$LABEL.csv" 0.25 &
SAMP=$!
rc=0; wait $CLI || rc=$?
wait $SAMP
t1=$(python3 -c 'import time;print(time.time())')
echo "$LABEL rc=$rc wall=$(python3 -c "print(round($t1-$t0,1))")s store=$(du -sh "$STORE" | cut -f1)" | tee -a "$OUT/summary.txt"
uptime >> "$OUT/$LABEL.uptime"
python3 -I "$HERE/perfsum.py" "$OUT/$LABEL.csv"
