#!/bin/bash
# Import memory experiment: variants interleaved; the bench process is sampled with procsample.py (footprint, RSS).
D=/Volumes/d7sst7/dev-targets/duck2-bench; R=/Volumes/d7sst7/dev-targets/research-data/scripts; OUT=$D/imem; mkdir -p $OUT
B=$(ls -t /Volumes/d7sst7/dev-targets/debrief-duckdb2-target-rel/release/deps/bench-* | grep -v '\.d$' | head -1)
L=/Volumes/d7sst7/dev-targets/debrief-libduckdb/2.0.0/duckdb-shared-libs-osx-arm64
run(){ # label env...
  label=$1; shift
  echo "$label load $(uptime | sed 's/.*averages: //')" >> $OUT/runs.txt
  env DYLD_LIBRARY_PATH=$L DEBRIEF_BENCH_EVENTS=3000000 DEBRIEF_BENCH_DIR=$D DEBRIEF_BENCH_IMPORT_ONLY=1 DEBRIEF_BENCH_IDLE_SECS=30 "$@" $B --nocapture > $OUT/$label.txt 2>&1 &
  P=$!
  python3 -I $R/procsample.py $P $OUT/$label.csv 0.25 &
  S=$!
  wait $P; wait $S
  python3 -I $R/perfsum.py $OUT/$label.csv 2>&1 | head -4 >> $OUT/$label.txt
}
for r in 1 2 3; do
  run A-$r
  run B-$r DEBRIEF_BENCH_IMPORT_MB=512 DEBRIEF_BENCH_LOWER=1
  run C-$r DEBRIEF_BENCH_IMPORT_MB=512
done
echo DONE >> $OUT/runs.txt
