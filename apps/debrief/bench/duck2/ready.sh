#!/bin/bash
# usage: ready.sh LABEL CORPUS [sample] [ENV=VAL...] -- import CORPUS in-process until search is ready (tests/import_ready.rs);
# CPU/memory timeline with procsample.py; "sample" also records a call-tree profile of the whole run with macOS sample(1).
D=/Volumes/d7sst7/dev-targets/duck2-bench; R=/Volumes/d7sst7/dev-targets/research-data/scripts; OUT=$D/prof; mkdir -p $OUT
LABEL=$1; CORPUS=$2; shift 2
SAMPLE=0; [ "${1:-}" = sample ] && { SAMPLE=1; shift; }
B=${READY_BIN:-$(ls -t /Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel/release/deps/import_ready-* | grep -v '\.d$' | head -1)}
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
STORE=$D/st-$LABEL; rm -rf $STORE
touch $D/running
echo "$LABEL start load $(sysctl -n vm.loadavg) bin $(basename $B) env $*" | tee -a $OUT/runs.txt
nice -n 10 env DYLD_LIBRARY_PATH=$L DEBRIEF_READY_CORPUS=$CORPUS DEBRIEF_READY_STORE=$STORE "$@" $B --nocapture > $OUT/$LABEL.txt 2>&1 &
P=$!
python3 -I $R/procsample.py $P $OUT/$LABEL.csv 0.5 &
S=$!
if [ $SAMPLE = 1 ]; then sleep 1; /usr/bin/sample $P 600 5 -mayDie -file $OUT/$LABEL.sample.txt > /dev/null 2>&1 & SP=$!; fi
wait $P; wait $S; [ $SAMPLE = 1 ] && wait $SP
grep "^ready:" $OUT/$LABEL.txt | tee -a $OUT/runs.txt
echo "$LABEL end load $(sysctl -n vm.loadavg)" | tee -a $OUT/runs.txt
rm -rf $STORE
rm -f $D/running
