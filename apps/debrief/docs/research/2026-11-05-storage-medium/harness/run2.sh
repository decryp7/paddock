#!/bin/sh
R=/Volumes/d7sst7/dev-targets/scratch/storage-research; D=$R/data/cooked; C=$R/cooked/cooked; DUCK=/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb
rm -rf $D; mkdir -p $D $D/tmp
T='/usr/bin/time -l'
$C gen $D 20000000 100000000
$T $C validate $D 2>&1 | grep -E 'step|peak memory'
echo "== parquet edges, same shape, sorted by src"
$T $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real'
.timer on
SET threads=4; SET memory_limit='768MB'; SET temp_directory='$D/tmp';
COPY (SELECT s::UINTEGER src, (CASE WHEN hash(s,j)%4=0 THEN hash(s,j,1)%20000000 ELSE (s+1+hash(s,j,2)%1000)%20000000 END)::UINTEGER dst FROM (SELECT s, unnest(range((hash(s)%11)::BIGINT)) j FROM range(20000000) r(s))) TO '$D/edges.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);
S
ls -l $D/edges.parquet | awk '{print $5,$9}'
echo "== one object's refs, twice"
$T $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real|^│ *[0-9]'
.timer on
SET threads=4; SET memory_limit='192MB'; SET temp_directory='$D/tmp';
SELECT count(*) FROM read_parquet('$D/edges.parquet') WHERE src = 12345678;
SELECT count(*) FROM read_parquet('$D/edges.parquet') WHERE src = 12345678;
S
echo "== 200 random objects' refs, twice"
$T $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real'
.timer on
SET threads=4; SET memory_limit='192MB'; SET temp_directory='$D/tmp';
SELECT count(*) FROM read_parquet('$D/edges.parquet') WHERE src IN (SELECT (i*7919*1009)%20000000 FROM range(200) r(i));
SELECT count(*) FROM read_parquet('$D/edges.parquet') WHERE src IN (SELECT (i*7919*1009)%20000000 FROM range(200) r(i));
S
echo "== reachability by recursive SQL, 768 MB, stopped after 900 s"
( $T $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real' ) &
.timer on
SET threads=4; SET memory_limit='768MB'; SET temp_directory='$D/tmp';
WITH RECURSIVE r(n) AS (SELECT 0::UINTEGER UNION SELECT e.dst FROM r JOIN read_parquet('$D/edges.parquet') e ON e.src=r.n) SELECT count(*) FROM r;
S
W=$!; n=0; while kill -0 $W 2>/dev/null && [ $n -lt 900 ]; do sleep 5; n=$((n+5)); done; if kill -0 $W 2>/dev/null; then pkill -P $W; echo "recursive stopped at ${n}s"; fi; wait $W
echo "== samples as Parquet"
$DUCK -no-agent <<S 2>&1 | grep -E 'Error'
SET threads=4; SET memory_limit='768MB';
COPY (SELECT i*1000 AS time, (hash(i)%200000)::UINTEGER stack, (1+hash(i+1)%100)::UBIGINT value FROM range(20000000) r(i)) TO '$D/samples.parquet' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);
S
ls -l $D/samples.parquet | awk '{print $5,$9}'
$T $DUCK -no-agent <<S 2>&1 | grep -E 'Error|peak memory|real'
.timer on
SET threads=4; SET memory_limit='192MB'; SET temp_directory='$D/tmp';
SELECT count(*) FROM (SELECT stack, sum(value) FROM read_parquet('$D/samples.parquet') WHERE time >= 5000000000 AND time < 10000000000 GROUP BY stack);
SELECT count(*) FROM (SELECT stack, sum(value) FROM read_parquet('$D/samples.parquet') WHERE time >= 5000000000 AND time < 10000000000 GROUP BY stack);
S
rm -rf $D; echo cooked2-done
