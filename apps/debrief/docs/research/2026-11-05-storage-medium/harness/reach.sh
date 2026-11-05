#!/bin/sh
# Rerun of the recursive reachability with its result kept: the first run's filter dropped the count.
R=/Volumes/d7sst7/dev-targets/scratch/storage-research; D=$R/data/cookedr; DUCK=/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb
rm -rf $D; mkdir -p $D/tmp
$DUCK -no-agent <<S
SET threads=4; SET memory_limit='768MB'; SET temp_directory='$D/tmp';
COPY (SELECT s::UINTEGER src, (CASE WHEN hash(s,j)%4=0 THEN hash(s,j,1)%20000000 ELSE (s+1+hash(s,j,2)%1000)%20000000 END)::UINTEGER dst FROM (SELECT s, unnest(range((hash(s)%11)::BIGINT)) j FROM range(20000000) r(s))) TO '$D/edges.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);
SELECT count(*) edges, count(*) FILTER (WHERE src=0) out_of_root FROM '$D/edges.parquet';
S
$DUCK -no-agent -csv -c "SELECT src, count(*) c FROM '$D/edges.parquet' WHERE src < 50 GROUP BY 1 ORDER BY c DESC, src LIMIT 3;"
for start in 0 7; do
/usr/bin/time -l $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real|^│ *[0-9]'
.timer on
SET threads=4; SET memory_limit='768MB'; SET temp_directory='$D/tmp';
WITH RECURSIVE r(n) AS (SELECT $start::UINTEGER UNION SELECT e.dst FROM r JOIN read_parquet('$D/edges.parquet') e ON e.src=r.n) SELECT count(*) FROM r;
S
done
rm -rf $D
