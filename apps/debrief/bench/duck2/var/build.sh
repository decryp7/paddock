#!/bin/bash
# Item 3 builds: the same events (store-ro, 3M) written four ways by DuckDB 2.0, timed; sizes after.
T=/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb; D=/Volumes/d7sst7/dev-targets/duck2-bench; V=$D/var
rm -f $V/*.duckdb $V/*.parquet
H="SET memory_limit='1GB'; SET threads=4; SET temp_directory='$D/tmpq'; ATTACH '$D/store-ro/data.duckdb' AS s (READ_ONLY); USE s; .timer on"
run(){ echo "== $1 load $(sysctl -n vm.loadavg)"; printf '%s\n%s\n' "$H" "$2" | $T -no-agent 2>&1 | grep -E "Run Time|Error"; }
run "parquet MAP (COPY, like today's segments)" "COPY (SELECT * FROM events) TO '$V/map.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);"
run "parquet VARIANT (COPY)" "COPY (SELECT * REPLACE (to_json(extra)::VARIANT AS extra) FROM events) TO '$V/var.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);"
run "native v2 MAP" "ATTACH '$V/nmap.duckdb' AS n (STORAGE_VERSION 'v2.0.0'); CREATE TABLE n.events AS SELECT * FROM events; CHECKPOINT n;"
run "native v2 VARIANT" "ATTACH '$V/nvar.duckdb' AS n (STORAGE_VERSION 'v2.0.0'); CREATE TABLE n.events AS SELECT * REPLACE (to_json(extra)::VARIANT AS extra) FROM events; CHECKPOINT n;"
ls -l $V/*.parquet $V/*.duckdb | awk '{printf "%s %.0f MB\n", $9, $5/1e6}'
du -sh $D/store-ro/segments
