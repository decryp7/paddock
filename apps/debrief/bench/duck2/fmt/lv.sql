INSTALL vortex; LOAD vortex; SET threads=4;
ATTACH '/Volumes/d7sst7/dev-targets/duck2-bench/store-ro/data.duckdb' AS s (READ_ONLY);
USE s;
.timer on
COPY (SELECT * REPLACE (to_json(extra)::VARCHAR AS extra) FROM s.events) TO '/Volumes/d7sst7/dev-targets/duck2-bench/fmt/ev.vortex' (FORMAT vortex);
