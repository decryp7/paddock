#!/bin/bash
# Interleaved A/B runs for docs/research/2026-10-28-duckdb-server-mode.md (macOS, one benchmark at a time).
# usage: bench.sh <phase> [rounds]   phase: import | query | loop | cancel | probe
# Needs: H (harness binary), BENCH (debrief-store-duck bench test binary, built on main), QD (data folder holding
# store/ and events.arrows), LIB (libduckdb 2.0.0 folder), LIB156 (libduckdb 1.5.6 folder, for BENCH).
# Every server binds 127.0.0.1 only and is stopped by its pid. Runs wait while the 1-minute load is above 40.
set -u
PHASE=$1; ROUNDS=${2:-3}
: "${H:?}" "${QD:?}" "${LIB:?}"
export QH_TMP=$QD/tmp QH_EXT_DIR=$QD/ext
OUT=$QD/results; mkdir -p "$OUT" "$QH_TMP"
TOKEN=$(head -c 16 /dev/urandom | xxd -p)
PORT=$((20000 + RANDOM % 20000))

load1() { sysctl -n vm.loadavg | awk '{print $2}'; }
wait_load() { while [ "$(load1 | cut -d. -f1)" -ge 40 ]; do sleep 10; done; }
h() { DYLD_LIBRARY_PATH=$LIB "$H" "$@"; }
log() { echo "$*" | tee -a "$OUT/$PHASE.log"; }

# start_server <args...>: a Quack server fed by a fifo (stdin: stats/exit); sets SRV_PID; fd 7 feeds it.
start_server() {
  [ -n "${QH_LOCK:-}" ] || unset QH_LOCK
  [ -n "${QH_SERVER_MEM:-}" ] || unset QH_SERVER_MEM
  local fifo=$QH_TMP/srv.fifo; rm -f "$fifo"; mkfifo "$fifo"
  DYLD_LIBRARY_PATH=$LIB "$H" serve "$@" < "$fifo" > "$QH_TMP/srv.out" 2>&1 &
  SRV_PID=$!
  exec 7> "$fifo"
  until grep -q -E 'READY|Error|error' "$QH_TMP/srv.out" 2>/dev/null; do sleep 0.2; done
  grep -E 'serving|Error|error' "$QH_TMP/srv.out" | tee -a "$OUT/$PHASE.log" >/dev/null
}
stop_server() {
  echo exit >&7; exec 7>&-
  wait "$SRV_PID" 2>/dev/null; kill "$SRV_PID" 2>/dev/null
  grep STATS "$QH_TMP/srv.out" | tail -1 | sed 's/^/  server: /' | tee -a "$OUT/$PHASE.log"
}
start_ipc() {
  local fifo=$QH_TMP/ipc.fifo; rm -f "$fifo"; mkfifo "$fifo"
  DYLD_LIBRARY_PATH=$LIB "$H" ipc-serve "$1" "$QH_TMP/w.sock" < "$fifo" > "$QH_TMP/ipc.out" 2>&1 &
  IPC_PID=$!
  exec 8> "$fifo"
  until grep -q READY "$QH_TMP/ipc.out" 2>/dev/null; do sleep 0.2; done
}
stop_ipc() {
  echo exit >&8; exec 8>&-
  wait "$IPC_PID" 2>/dev/null; kill "$IPC_PID" 2>/dev/null
  grep -E 'IMPORT' "$QH_TMP/ipc.out" | sed 's/^/  worker: /' | tee -a "$OUT/$PHASE.log"
  grep STATS "$QH_TMP/ipc.out" | tail -1 | sed 's/^/  worker: /' | tee -a "$OUT/$PHASE.log"
}
run() { wait_load; log "## $* (load $(load1))"; "$@" 2>&1 | grep -E '^(IMPORT|Q |LOOP|CANCEL|PROBE|STATS|connect_ms|open_ms|server events|import:|thread|Error)' | sed 's/^/  /' | tee -a "$OUT/$PHASE.log"; }

# Debrief's own import (parse + Parquet segments + catalog), DuckDB 1.5.6 as on main. /usr/bin/env would drop DYLD_*.
real_import() { DYLD_LIBRARY_PATH=$LIB156 DEBRIEF_BENCH_EVENTS=3000000 DEBRIEF_BENCH_DIR=$QD DEBRIEF_BENCH_STORE=$QH_TMP/bstore DEBRIEF_BENCH_IMPORT_ONLY=1 "$BENCH" --nocapture; }
SEG=$QD/store/segments
case $PHASE in
import)
  for r in $(seq 1 "$ROUNDS"); do
    log "### round $r"
    rm -rf "$QH_TMP/bstore"; run real_import
    run h import-null "$QD/events.arrows"
    for n in 8 12; do rm -rf "$QH_TMP/seg"; run h import-parquet "$QD/events.arrows" $n "$QH_TMP/seg"; done
    for n in 1 4; do run h import-inproc "$QD/events.arrows" $n "$QH_TMP/inproc.duckdb"; done
    # The remote appender fails on this data (see the note), so only the staged INSERT path is timed.
    for spec in "insert 1 192MB" "insert 4 192MB" "insert 4 1GB" "insert 8 1GB"; do
      set -- $spec
      rm -f "$QH_TMP/srv.duckdb" "$QH_TMP/srv.duckdb.wal"
      log "## server memory_limit $3"
      QH_SERVER_MEM=$3 start_server "$QH_TMP/srv.duckdb" $PORT "$TOKEN" table "$QD/events.arrows"
      run h import-quack "$QD/events.arrows" "$2" $PORT "$TOKEN" "$1"
      stop_server
    done
    rm -rf "$QH_TMP/seg" "$QH_TMP/inproc.duckdb"* "$QH_TMP/srv.duckdb"*
  done ;;
query)
  for r in $(seq 1 "$ROUNDS"); do
    log "### round $r"
    run h query-inproc "$SEG"
    start_server :memory: $PORT "$TOKEN" view "$SEG"; run h query-quack $PORT "$TOKEN"; stop_server
    start_ipc "$SEG"; run h query-ipc "$QH_TMP/w.sock"; stop_ipc
  done ;;
loop)
  for r in $(seq 1 "$ROUNDS"); do
    log "### round $r"
    rm -rf "$QH_TMP/segB"; run h loop-inproc "$SEG" "$QD/events.arrows" 8 "$QH_TMP/segB"
    rm -rf "$QH_TMP/segB"; start_ipc "$SEG"; run h loop-ipc "$QH_TMP/w.sock" "$QD/events.arrows" 8 "$QH_TMP/segB"; stop_ipc
    rm -f "$QH_TMP/stop" "$QH_TMP/srv.duckdb"*
    start_server "$QH_TMP/srv.duckdb" $PORT "$TOKEN" wtable "$QD/events.arrows" view "$SEG"
    # Reads go to the `events` view over the segments; the import writes the table `events_w`.
    wait_load; log "## loop-quack + import-quack insert 1 (load $(load1))"
    ( h loop-quack $PORT "$TOKEN" "$QH_TMP/stop" 2>&1 | grep -E '^(LOOP|STATS)' | sed 's/^/  /' | tee -a "$OUT/$PHASE.log" ) &
    QP=$!
    h import-quack "$QD/events.arrows" 1 $PORT "$TOKEN" insert events_w 2>&1 | grep -E '^(IMPORT|STATS|server|Error)' | sed 's/^/  /' | tee -a "$OUT/$PHASE.log"
    touch "$QH_TMP/stop"; wait $QP
    stop_server
    rm -rf "$QH_TMP/segB" "$QH_TMP/srv.duckdb"*
  done ;;
cancel)
  for r in $(seq 1 "$ROUNDS"); do
    log "### round $r"
    run h cancel-inproc "$SEG"
    start_server :memory: $PORT "$TOKEN" view "$SEG"
    run h cancel-quack $PORT "$TOKEN"
    echo stats >&7; sleep 0.5; a=$(grep STATS "$QH_TMP/srv.out" | tail -1 | sed 's/.*cpu_s=//')
    sleep 5
    echo stats >&7; sleep 0.5; b=$(grep STATS "$QH_TMP/srv.out" | tail -1 | sed 's/.*cpu_s=//')
    log "  server CPU in the 5 s after the client's cancel returned: $(echo "$b - $a" | bc) s"
    stop_server
  done ;;
probe)
  # Outside the temp folder (DuckDB always allows its temp_directory). Server and client run in different working folders, so a relative COPY TO shows which process ran it.
  P=$QD/probe; mkdir -p "$P"
  echo "synthetic line one" > "$P/probe.txt"
  mkdir -p "$P/srvcwd" "$P/clicwd"
  for lock in "" "$SEG"; do
    rm -f "$P"/srvcwd/* "$P"/clicwd/*
    log "### server $([ -n "$lock" ] && echo 'locked (allowed_directories = segments, enable_external_access = false, lock_configuration = true)' || echo 'with defaults')"
    cd "$P/srvcwd"; QH_LOCK=$lock start_server :memory: $PORT "$TOKEN" view "$SEG"; grep lock "$QH_TMP/srv.out" | tee -a "$OUT/$PHASE.log"
    cd "$P/clicwd"
    for s in query connect attach; do QH_STYLE=$s run h probe $PORT "$TOKEN" "$P/probe.txt"; done
    [ -n "$lock" ] && QH_STYLE=query run h query-quack $PORT "$TOKEN"
    log "  files written by COPY: server folder: $(ls "$P/srvcwd" | tr '\n' ' '); client folder: $(ls "$P/clicwd" | tr '\n' ' ')"
    stop_server
  done ;;
esac
