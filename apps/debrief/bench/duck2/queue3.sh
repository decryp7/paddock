#!/bin/bash
# Item 3, after E3: DuckDB builds (timed), the arrow-rs probe (timed), then the query suite; each at load < 10.
D=/Volumes/d7sst7/dev-targets/duck2-bench
while pgrep -f "$D/exp.sh e3" > /dev/null; do sleep 30; done
while [ -e $D/pause ]; do sleep 10; done
touch $D/running
for i in 1 2 3; do $D/waitload.sh 10 5400; echo "build round $i"; nice -n 10 $D/var/build.sh; done
for i in 1 2 3; do $D/waitload.sh 10 5400; echo "probe round $i load $(sysctl -n vm.loadavg)"; nice -n 10 $D/variant/target/release/variant-probe $D/store-ro/segments $D/var; done
$D/waitload.sh 10 5400; echo "queries load $(sysctl -n vm.loadavg)"; nice -n 10 python3 -I $D/var/q.py; echo "queries end load $(sysctl -n vm.loadavg)"
rm -f $D/running; echo QUEUE3 DONE
