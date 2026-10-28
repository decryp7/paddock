#!/bin/bash
# usage: perf-post.sh LABEL STORE_DIR [IDLE_S]
# Reopens a store an import wrote with the perf_idle harness (crates/debrief-store-duck/tests/perf_idle.rs, ignored by
# default) and samples it with procsample.py; prints the per-phase summary.
# Environment: DEBRIEF_PERF_BIN (release target folder; the newest deps/perf_idle-* binary is used), DEBRIEF_LIB_DIR
# (libduckdb folder), DEBRIEF_PERF_OUT (results folder). macOS only.
set -eu
LABEL=$1; STORE=$2; IDLE=${3:-60}
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../../crates/debrief-store-duck" && pwd)
B=${DEBRIEF_PERF_BIN:?set DEBRIEF_PERF_BIN to the release target folder}
L=${DEBRIEF_LIB_DIR:?set DEBRIEF_LIB_DIR to the libduckdb folder}
OUT=${DEBRIEF_PERF_OUT:?set DEBRIEF_PERF_OUT to a results folder}
T=$(ls -t "$B"/deps/perf_idle-* | grep -v '\.d$' | head -1)
export DYLD_LIBRARY_PATH=$L DEBRIEF_PERF_STORE=$STORE DEBRIEF_PERF_IDLE_S=$IDLE
mkdir -p "$OUT"
cd "$CRATE"
uptime > "$OUT/$LABEL-post.uptime"
"$T" --ignored --nocapture --test-threads 1 > "$OUT/$LABEL-post.log" 2>&1 &
P=$!
python3 -I "$HERE/procsample.py" $P "$OUT/$LABEL-post.csv" 0.25 &
S=$!
rc=0; wait $P || rc=$?
echo "rc=$rc"; wait $S
uptime >> "$OUT/$LABEL-post.uptime"
grep -v '^PHASE\|^test \|^$\|^running' "$OUT/$LABEL-post.log" || true
python3 -I "$HERE/perfsum.py" "$OUT/$LABEL-post.csv" "$OUT/$LABEL-post.log"
