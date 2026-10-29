#!/bin/bash
# usage: exp.sh SET ROUNDS -- interleaved variants (each line of $D/SET.variants: LABEL ENV=VAL...), warm corpus, load < 10 at start.
D=/Volumes/d7sst7/dev-targets/duck2-bench; SET=$1; ROUNDS=${2:-3}
export READY_BIN=$D/prof/bin/ready-knobs
for r in $(seq 1 $ROUNDS); do
  while read -r label envs; do
    [ -z "$label" ] && continue
    while [ -e $D/pause ]; do sleep 10; done
    (cd $D && find large10 -type f -print0 | xargs -0 cat > /dev/null)
    $D/waitload.sh 10 5400 >/dev/null || { echo "$SET-$label-$r skipped: load"; continue; }
    bin=$(echo "$envs" | sed -n 's/.*BIN=\([^ ]*\).*/\1/p'); envs=$(echo "$envs" | sed 's/BIN=[^ ]*//')
    READY_BIN=$D/prof/bin/${bin:-ready-knobs} $D/ready.sh $SET-$label-$r $D/large10 $envs > /dev/null
    echo "$SET-$label-$r $(grep '^ready: \(import\|index\)' $D/prof/$SET-$label-$r.txt | cut -c1-400 | tr '\n' ' ')"
  done < $D/$SET.variants
done
echo "$SET DONE"
