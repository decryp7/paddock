#!/usr/bin/env python3
"""Multiplies a store the product wrote into K copies (same files layout, same times, new ids and sources), to reach 1B events
with the real mix of sources per segment. Rewritten with DuckDB, so footers differ a little from arrow-rs's.
usage: mix1b.py FROM_STORE TO_STORE K. Generated data only (ER-14)."""
import os, re, shutil, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor
ROOT = "/Volumes/d7sst7/dev-targets/scratch/storage-research"; DUCK = "/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
A, B, K = sys.argv[1], sys.argv[2], int(sys.argv[3]); SA, SB = f"{A}/segments", f"{B}/segments"
shutil.rmtree(B, ignore_errors=True); os.makedirs(SB); tmp = f"{B}/tmp"; os.makedirs(tmp)
def sql(s, db=None, ro=False):
    p = subprocess.run([DUCK, "-no-agent", "-csv"] + (["-readonly"] if ro else []) + ([db] if db else []), input=s, capture_output=True, text=True)
    if "Error" in p.stdout + p.stderr: sys.exit(f"::error::{(p.stdout + p.stderr)[:400]}")
    return p.stdout
cat = f"{B}/data.duckdb"; shutil.copyfile(f"{A}/data.duckdb", cat)
if os.path.exists(f"{A}/data.duckdb.wal"): shutil.copyfile(f"{A}/data.duckdb.wal", cat + ".wal")
sql("CHECKPOINT;", cat)
step = int(sql("SELECT max(end_id) FROM segments;", cat).splitlines()[1]) + 1; sstep = int(sql("SELECT max(id) FROM sources;", cat).splitlines()[1]) + 1
fs = sorted(f for f in os.listdir(SA) if f.endswith(".parquet"))
for f in fs: os.link(f"{SA}/{f}", f"{SB}/{f}")
def batch(k):
    if shutil.disk_usage(ROOT).free < 200 << 30: sys.exit("::error::d7 under 200 GiB free; stopping")
    s = f"SET threads=2; SET memory_limit='1GB'; SET temp_directory='{tmp}'; SET preserve_insertion_order=true;\n"
    for f in fs:
        s += f"COPY (SELECT * REPLACE (id + {k*step} AS id, source + {k*sstep} AS source) FROM read_parquet('{SA}/{f}')) TO '{SB}/k{k}-{f}' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536);\n"
    sql(s)
t = time.time()
with ThreadPoolExecutor(3) as e: list(e.map(batch, range(1, K)))
print("rewrite secs", round(time.time() - t), flush=True)
cols = lambda tbl: [l for l in sql(f"SELECT column_name FROM duckdb_columns() WHERE table_name='{tbl}' ORDER BY column_index;", cat).splitlines()[1:]]
s = "BEGIN;\n"
for k in range(1, K):
    s += f"INSERT INTO sources SELECT * REPLACE (id + {k*sstep} AS id, name || '#{k}' AS name) FROM sources WHERE id < {sstep};\n"
    s += f"INSERT INTO segments SELECT * REPLACE ('k{k}-' || file AS file, first_id + {k*step} AS first_id, end_id + {k*step} AS end_id) FROM segments WHERE file NOT LIKE 'k%-%';\n"
    s += f"INSERT INTO segment_fields SELECT * REPLACE ('k{k}-' || file AS file) FROM segment_fields WHERE file NOT LIKE 'k%-%';\n"
    s += f"INSERT INTO histogram SELECT * REPLACE ('k{k}-' || file AS file, source + {k*sstep} AS source) FROM histogram WHERE file NOT LIKE 'k%-%';\n"
s += "COMMIT; DELETE FROM ord; CHECKPOINT;\n"
sql(s, cat)
all_ = sorted(os.listdir(SB)); lst = ", ".join(f"'{SB}/{f}'" for f in all_)
v = sql("SELECT sql FROM duckdb_views() WHERE view_name='events';", cat)
m = re.search(r"SELECT .*? FROM read_parquet", v, re.S); head = m.group(0).strip('"') if m else None
cols_ev = [l for l in sql("SELECT column_name FROM (DESCRIBE events);", cat).splitlines()[1:]]
sel = ", ".join(f'"{c}"' for c in cols_ev)
frm = f"read_parquet([{lst}], union_by_name = true)"; READY = "source IN (SELECT id FROM sources WHERE ready)"
sql(f"CREATE OR REPLACE VIEW events AS SELECT {sel} FROM {frm} WHERE {READY}; CREATE OR REPLACE VIEW events_wide AS SELECT * FROM {frm} WHERE {READY}; CHECKPOINT;", cat)
print("done files", len(all_), "catalog bytes", os.path.getsize(cat), flush=True)
