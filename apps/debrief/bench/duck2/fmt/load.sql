SET memory_limit='1GB'; SET threads=4;
ATTACH '/Volumes/d7sst7/dev-targets/duck2-bench/store-ro/data.duckdb' AS s (READ_ONLY);
ATTACH '/Volumes/d7sst7/dev-targets/duck2-bench/fmt/n15.duckdb' AS n (STORAGE_VERSION 'v1.5.0');
USE s;
.timer on
CREATE TABLE n.events AS SELECT * FROM s.events;
CHECKPOINT n;
