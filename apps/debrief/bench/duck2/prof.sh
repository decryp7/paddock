#!/bin/bash
# usage: prof.sh LABEL CORPUS -- one import_ready run with sample(1) call trees in three windows: the import (1-47 s),
# index plus catalogue (60-200 s) and catalogue alone (240-420 s).
D=/Volumes/d7sst7/dev-targets/duck2-bench; OUT=$D/prof; LABEL=$1; CORPUS=$2
B=$D/prof/bin/ready-base; L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
STORE=$D/st-$LABEL; rm -rf $STORE
echo "$LABEL start load $(sysctl -n vm.loadavg) (sampled)" | tee -a $OUT/runs.txt
nice -n 10 env DYLD_LIBRARY_PATH=$L DEBRIEF_READY_CORPUS=$CORPUS DEBRIEF_READY_STORE=$STORE $B --nocapture > $OUT/$LABEL.txt 2>&1 &
P=$!
( sleep 1; /usr/bin/sample $P 46 2 -mayDie -file $OUT/$LABEL.import.sample.txt >/dev/null 2>&1
  sleep 13; /usr/bin/sample $P 140 5 -mayDie -file $OUT/$LABEL.index.sample.txt >/dev/null 2>&1
  sleep 40; /usr/bin/sample $P 180 5 -mayDie -file $OUT/$LABEL.cat.sample.txt >/dev/null 2>&1 ) &
S=$!
wait $P; wait $S
grep "^ready:" $OUT/$LABEL.txt | tee -a $OUT/runs.txt
echo "$LABEL end load $(sysctl -n vm.loadavg)" | tee -a $OUT/runs.txt
rm -rf $STORE
