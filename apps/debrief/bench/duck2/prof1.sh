#!/bin/bash
# usage: prof1.sh LABEL BIN START DUR [ENV=VAL...] -- one import_ready run (stops after the import) with one sample(1) window.
D=/Volumes/d7sst7/dev-targets/duck2-bench; OUT=$D/prof; LABEL=$1; B=$D/prof/bin/$2; START=$3; DUR=$4; shift 4
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64; STORE=$D/st-$LABEL; rm -rf $STORE
touch $D/running; echo "$LABEL start load $(sysctl -n vm.loadavg) (sampled)" | tee -a $OUT/runs.txt
nice -n 10 env DYLD_LIBRARY_PATH=$L DEBRIEF_READY_CORPUS=$D/large10 DEBRIEF_READY_STORE=$STORE DEBRIEF_READY_WAIT=import "$@" $B --nocapture > $OUT/$LABEL.txt 2>&1 &
P=$!
( sleep $START; /usr/bin/sample $P $DUR 5 -mayDie -file $OUT/$LABEL.sample.txt >/dev/null 2>&1 ) &
python3 -I /Volumes/d7sst7/dev-targets/research-data/scripts/procsample.py $P $OUT/$LABEL.csv 0.5 &
wait $P; wait
grep "^ready:" $OUT/$LABEL.txt | tee -a $OUT/runs.txt; rm -rf $STORE; rm -f $D/running
