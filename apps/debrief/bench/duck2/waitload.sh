#!/bin/bash
# usage: waitload.sh MAX_LOAD MAX_WAIT_S -- waits until the 1-minute load is under MAX_LOAD and the 5-minute load under 15
# (checked every 15 s); exit 1 on timeout.
max=$1; limit=$2; t=0
while :; do
  read -r l1 l5 <<<"$(sysctl -n vm.loadavg | awk '{print $2, $3}')"
  if awk -v a="$l1" -v b="$l5" -v m="$max" 'BEGIN{exit !(a<m && b<15)}'; then echo "load $l1/$l5 ok after ${t}s"; exit 0; fi
  [ $t -ge $limit ] && { echo "timeout: load $l1/$l5 after ${t}s"; exit 1; }
  sleep 15; t=$((t+15))
done
