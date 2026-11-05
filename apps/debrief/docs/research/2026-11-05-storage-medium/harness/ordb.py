#!/usr/bin/env python3
"""The product's ordering statement (pos.rs insert_sql, with 768 MB, 4 threads). usage: ordb.py LABEL STORE"""
import json, os, re, subprocess, sys, time
ROOT = "/Volumes/d7sst7/dev-targets/scratch/storage-research"; DUCK = "/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
L, S = sys.argv[1:3]; c = f"{S}/data.duckdb"; tmp = f"{ROOT}/data/tmp-{L}"; os.makedirs(tmp, exist_ok=True)
q = f"SET memory_limit='768MB'; SET threads=4; SET temp_directory='{tmp}'; BEGIN; DELETE FROM ord; INSERT INTO ord SELECT (row_number() OVER () - 1)::UBIGINT, id FROM (SELECT id FROM events ORDER BY coalesce(ts, 9223372036854775807), source, line, id); COMMIT; CHECKPOINT;"
t = time.time(); p = subprocess.run(["/usr/bin/time", "-l", DUCK, "-no-agent", "-csv", c], input=q, capture_output=True, text=True)
pk = re.search(r"(\d+)\s+peak memory footprint", p.stderr); e = [x for x in (p.stdout + p.stderr).splitlines() if "Error" in x]
r = dict(step="ord-rebuild", secs=round(time.time() - t, 1), peak_mib=int(pk.group(1)) >> 20 if pk else None, cat_bytes=os.path.getsize(c), load=round(os.getloadavg()[0], 1), error=e[:1]); print(json.dumps(r)); open(f"{ROOT}/results/{L}.jsonl", "a").write(json.dumps(r) + "\n")
