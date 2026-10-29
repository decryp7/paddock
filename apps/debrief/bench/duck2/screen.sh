#!/bin/bash
cd /Volumes/d7sst7/dev-targets/duck2-bench
for spec in "thr12:DEBRIEF_EXP_THREADS=12" "thr13:DEBRIEF_EXP_THREADS=13" "chunk16:DEBRIEF_EXP_CHUNK_MB=16" "chunk256:DEBRIEF_EXP_CHUNK_MB=256" "none:DEBRIEF_EXP_COMPRESSION=none" "lz4:DEBRIEF_EXP_COMPRESSION=lz4" "snappy:DEBRIEF_EXP_COMPRESSION=snappy" "zstd3:DEBRIEF_EXP_COMPRESSION=zstd3" "rg32k:DEBRIEF_EXP_ROWGROUP=32768" "rg131k:DEBRIEF_EXP_ROWGROUP=131072" "dict0:DEBRIEF_EXP_DICT=0"; do
  ./tune.sh B-${spec%%:*} large03 2>&1 | tail -1
  ./tune.sh V-${spec%%:*} large03 ${spec#*:} 2>&1 | tail -1
done
echo SCREEN-DONE
