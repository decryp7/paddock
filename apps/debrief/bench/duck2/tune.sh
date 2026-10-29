#!/bin/bash
# usage: tune.sh LABEL CORPUS [VAR=value ...]  -- one real import (release CLI + store worker) with experiment knobs,
# sampled with procsample.py; prints events/s, wall, load before/after, peak footprint, CPU and the store size.
set -u
LABEL=$1; CORPUS=$2; shift 2
D=/Volumes/d7sst7/dev-targets/duck2-bench; R=/Volumes/d7sst7/dev-targets/research-data/scripts; OUT=$D/tune; mkdir -p $OUT
B=/Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel/release; L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
STORE=$D/st-$LABEL; rm -rf $STORE
for i in $(seq 1 90); do l=$(uptime | sed 's/.*averages: //' | cut -d' ' -f1 | cut -d. -f1); [ "$l" -lt ${LOADMAX:-8} ] && break; sleep 5; done
load0=$(uptime | sed 's/.*averages: //' | cut -d' ' -f1)
env DEBRIEF_LIB_DIR=$L DEBRIEF_STORE_WORKER=$B/debrief-store-worker DYLD_LIBRARY_PATH=$L "$@" $B/debrief import "$CORPUS" --store $STORE --json --quiet > $OUT/$LABEL.json 2> $OUT/$LABEL.err &
CLI=$!
python3 -I $R/procsample.py $CLI $OUT/$LABEL.csv 0.25 &
SAMP=$!
if [ -n "${SAMPLE_SECS:-}" ]; then sleep 2; W=$(pgrep -P $CLI | head -1); [ -n "$W" ] && sample $W $SAMPLE_SECS 1 -file $OUT/$LABEL.sample.txt > /dev/null 2>&1; fi
wait $CLI; rc=$?; wait $SAMP
load1=$(uptime | sed 's/.*averages: //' | cut -d' ' -f1)
python3 -I - "$OUT/$LABEL" "$STORE" "$load0" "$load1" "$rc" "$*" <<'P'
import json,sys,os,csv,subprocess
lab,store,l0,l1,rc,knobs=sys.argv[1:7]
d=json.load(open(lab+'.json'))
ev=d['events']; ms=d['elapsedMs']
rows=[r for r in csv.DictReader(open(lab+'.csv')) if 'worker' in r['name']]
foot=max(int(r['max_footprint']) for r in rows)>>20 if rows else 0
cpu=(max(int(r['cpu_ns']) for r in rows))/1e9 if rows else 0
size=int(subprocess.run(['du','-sk',store],capture_output=True,text=True).stdout.split()[0])>>10
print(f"{os.path.basename(lab)}: rc={rc} {ev} events in {ms/1000:.1f} s = {ev/ms*1000/1000:.0f}k/s, peak footprint {foot} MB, CPU {cpu:.0f} s, store {size} MB, load {l0}->{l1}  [{knobs}]")
P
rm -rf $STORE
