#!/bin/sh
# gen, then real import through the release CLI, private HOME and TMPDIR
R=/Volumes/d7sst7/dev-targets/scratch/storage-research; cd $R; L=$1; shift
B=$R/bin; P=$R/profile/$L; mkdir -p $P/tmp
export HOME=$P TMPDIR=$P/tmp XDG_DATA_HOME=$P/data DEBRIEF_STORE_WORKER=$B/debrief-store-worker DEBRIEF_PLUGINS=$B/debrief-plugins DEBRIEF_LIB_DIR=$B DYLD_LIBRARY_PATH=$B
/usr/bin/time -l $B/debrief import $R/data/src-$L --store $R/data/store-$L --json --quiet "$@" > results/$L-import.json 2> results/$L-import.err; echo "rc=$?" >> results/$L-import.err
