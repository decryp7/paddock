#!/usr/bin/env python3
"""Throwaway storage probes on generated data (ER-14). usage: probe.py <rows> <label> [steps...]"""
import json, os, re, shutil, subprocess, sys, time
ROOT = "/Volumes/d7sst7/dev-targets/scratch/storage-research"
DUCK = "/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
N = int(sys.argv[1]); LABEL = sys.argv[2]; STEPS = sys.argv[3:]
D = f"{ROOT}/data/{LABEL}"; SEG = f"{D}/segments"; CAT = f"{D}/data.duckdb"; TMP = f"{D}/tmp"; OUT = f"{ROOT}/results/{LABEL}.jsonl"
SPAN = 30 * 86400000; T0 = 1_780_000_000_000
QUERY = f"SET memory_limit='192MB'; SET threads=4; SET async_threads=4; SET temp_directory='{TMP}'; SET GLOBAL parquet_metadata_cache=true; SET autoinstall_known_extensions=false; SET autoload_known_extensions=false;\n"
IMPORT = QUERY.replace("192MB", "768MB")
KEY = "coalesce(ts, 9223372036854775807), source, line, id"
def log(**k):
    k.update(t=time.strftime("%H:%M:%S"), load=os.getloadavg()[0]); print(json.dumps(k), flush=True)
    open(OUT, "a").write(json.dumps(k) + "\n")
def run(name, sql, db=CAT, readonly=False, record=True):
    if shutil.disk_usage(ROOT).free < 100 << 30: sys.exit("::error::under 100 GB free; stopping")
    args = ["/usr/bin/time", "-l", DUCK, "-no-agent", "-csv"] + (["-readonly"] if readonly else []) + ([db] if db else [])
    t = time.time()
    try:
        p = subprocess.run(args, input=".timer on\n" + sql, capture_output=True, text=True, timeout=1500)
    except subprocess.TimeoutExpired:
        log(step=name, timeout=1500); return ""
    wall = time.time() - t
    peak = re.search(r"(\d+)\s+peak memory footprint", p.stderr); rss = re.search(r"(\d+)\s+maximum resident", p.stderr)
    runs = [float(x) for x in re.findall(r"real ([0-9.]+)", p.stdout + p.stderr)]
    rec = dict(step=name, wall=round(wall, 2), runs=runs[-3:], peak_mb=int(peak.group(1)) >> 20 if peak else None, rss_mb=int(rss.group(1)) >> 20 if rss else None, rc=p.returncode)
    err = [l for l in (p.stdout + p.stderr).splitlines() if "Error" in l]
    if err: rec["error"] = err[0][:300]
    if record: log(**rec)
    return p.stdout
def size(path):
    return sum(os.path.getsize(os.path.join(r, f)) for r, _, fs in os.walk(path) for f in fs) if os.path.isdir(path) else os.path.getsize(path)
def files(): return sorted(f"{SEG}/{f}" for f in os.listdir(SEG) if f.endswith(".parquet"))
def plist(fs): return "read_parquet([" + ", ".join(f"'{f}'" for f in fs) + "], union_by_name = true)"
def gen():
    shutil.rmtree(D, ignore_errors=True); os.makedirs(D); os.makedirs(TMP)
    per = N // 64; step = SPAN // per
    sql = f"""SET memory_limit='3GB'; SET threads=4; SET temp_directory='{TMP}'; SET preserve_insertion_order=true;
COPY (SELECT i::UBIGINT id, CASE WHEN i%1000=999 THEN NULL ELSE {T0}+(i%{per})*{step} END::BIGINT ts, 0::UTINYINT ts_state, ((i*31)%7%6)::UTINYINT level,
 (i//{per})::UINTEGER source, (i%{per}+1)::UINTEGER line, ((i%{per})*120)::UBIGINT byte_offset,
 'svc-'||(i%97)||' order='||((i*2654435761)%1000003)||' took '||(i%4096)||' ms' message, 'svc-<n> order=<n> took <n> ms' template, CASE WHEN i%500=0 THEN 'System.Exception at Foo.Bar()' END exception,
 0::UBIGINT markers, '2026-01-01 svc-'||(i%97)||' order='||((i*2654435761)%1000003)||' took '||(i%4096)||' ms' raw, MAP{{'MachineName':'host-'||(i%16)}} extra
 FROM range({per*64}) r(i)) TO '{SEG}' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536, ROW_GROUPS_PER_FILE 2);"""
    run("gen", sql, db=None)
    fs = files(); log(step="gen-size", files=len(fs), bytes=size(SEG), footer=footer(fs[len(fs)//2]))
def footer(f):
    with open(f, "rb") as b:
        b.seek(-8, 2); import struct; return struct.unpack("<I", b.read(4))[0]
def catalog():
    if os.path.exists(CAT): os.remove(CAT)
    fs = files()
    fq = plist(fs).replace("union_by_name = true", "union_by_name = false")
    sql = IMPORT + f"""CREATE TABLE sources(id UINTEGER PRIMARY KEY, name VARCHAR, ready BOOLEAN); INSERT INTO sources SELECT i, 'source-'||i, true FROM range(64) r(i);
CREATE TABLE ord (pos UBIGINT NOT NULL, id UBIGINT NOT NULL);
CREATE TABLE segments AS SELECT file_name AS file,
 min(try_cast(stats_min_value AS BIGINT)) FILTER (path_in_schema='id') first_id, max(try_cast(stats_max_value AS BIGINT)) FILTER (path_in_schema='id') last_id,
 min(try_cast(stats_min_value AS BIGINT)) FILTER (path_in_schema='ts') ts_min, max(try_cast(stats_max_value AS BIGINT)) FILTER (path_in_schema='ts') ts_max,
 min(try_cast(stats_min_value AS BIGINT)) FILTER (path_in_schema='source') src_min, max(try_cast(stats_max_value AS BIGINT)) FILTER (path_in_schema='source') src_max
 FROM parquet_metadata('{SEG}/*.parquet') GROUP BY file_name;
"""
    run("catalog-segments", sql)
    view = f"CREATE OR REPLACE VIEW events AS SELECT * FROM {plist(fs)} WHERE source IN (SELECT id FROM sources WHERE ready);\nCREATE OR REPLACE VIEW events_wide AS SELECT * FROM {plist(fs)} WHERE source IN (SELECT id FROM sources WHERE ready);\n"
    log(step="view-sql-bytes", bytes=len(view))
    run("view-create-first", QUERY + view)
    run("view-replace-again", QUERY + view + view)
    log(step="catalog-size", bytes=size(CAT))
def ord_():
    run("ord-rebuild", IMPORT + f"BEGIN; DELETE FROM ord; INSERT INTO ord SELECT (row_number() OVER () - 1)::UBIGINT, id FROM (SELECT id FROM events ORDER BY {KEY}); COMMIT; CHECKPOINT;")
    log(step="catalog-size-after-ord", bytes=size(CAT))
def hist():
    fs = files()
    run("histogram-build", IMPORT + f"CREATE OR REPLACE TABLE histogram AS SELECT filename file, ts//60000 AS minute, level, source, count(*)::UBIGINT n FROM read_parquet('{SEG}/*.parquet', filename=true, union_by_name=true) GROUP BY ALL; CHECKPOINT;")
    log(step="catalog-size-after-hist", bytes=size(CAT), rows=re.findall(r"^\d+$", run("x","SELECT count(*) FROM histogram;",record=False,readonly=True), re.M))
def twice(name, q, pre=QUERY):
    run(name, pre + q + "\n" + q + "\n", readonly=True)
def queries():
    mid_ts = T0 + SPAN // 2; bucket = SPAN // 1000
    twice("q-bind-only", "SELECT * FROM events LIMIT 0;")
    twice("q-count", "SELECT count(*) FROM events;")
    twice("q-level-facet", "SELECT level, count(*) FROM events GROUP BY ALL;")
    twice("q-1000-buckets-scan", f"SELECT ts//{bucket}, level, count(*) FROM events WHERE ts IS NOT NULL GROUP BY ALL;")
    twice("q-1000-buckets-histogram", f"SELECT minute*60000//{bucket}, level, sum(n) FROM histogram WHERE minute IS NOT NULL GROUP BY ALL;")
    twice("q-page-by-ord-ids-mid", f"SELECT id FROM ord WHERE pos >= {N//2} AND pos < {N//2+200} ORDER BY pos;")
    out = run("x", QUERY + f"SELECT id FROM ord WHERE pos >= {N//2} AND pos < {N//2+200} ORDER BY pos;", readonly=True, record=False)
    ids = [int(x) for x in re.findall(r"^\d+$", out, re.M)]
    if ids:
        idl = ",".join(map(str, ids))
        rows = run("x", QUERY + f"SELECT file FROM segments WHERE " + " OR ".join(f"({i} BETWEEN first_id AND last_id)" for i in sorted(set(ids))[:200]) + ";", readonly=True, record=False)
        pf = re.findall(r"^.*\.parquet$", rows, re.M)
        log(step="page-files", files=len(pf))
        twice("q-page-rows-pruned-list", f"SELECT * FROM {plist(pf)} WHERE id IN ({idl});")
        twice("q-page-rows-full-view", f"SELECT * FROM events WHERE id IN ({idl});")
    twice("q-position-of-id", f"SELECT pos FROM ord WHERE id = {N//3};")
    tk = f"SELECT id, ts, level, source, message FROM events WHERE ts >= {mid_ts} ORDER BY {KEY} LIMIT 200;"
    twice("q-keyset-mid-view", tk)
    pf = run("x", QUERY + f"SELECT file FROM segments WHERE ts_max >= {mid_ts} AND ts_min <= {mid_ts + SPAN//500};", readonly=True, record=False)
    pf = re.findall(r"^.*\.parquet$", pf, re.M); log(step="keyset-files", files=len(pf))
    twice("q-keyset-mid-pruned-list", f"SELECT id, ts, level, source, message FROM {plist(pf)} WHERE ts >= {mid_ts} AND ts <= {mid_ts + SPAN//500} ORDER BY {KEY} LIMIT 200;")
    twice("q-filtered-page-view", f"SELECT id, ts, message FROM events WHERE level=5 AND source=3 ORDER BY {KEY} LIMIT 200;")
    pf = run("x", QUERY + "SELECT file FROM segments WHERE 3 BETWEEN src_min AND src_max;", readonly=True, record=False)
    pf = re.findall(r"^.*\.parquet$", pf, re.M); log(step="source-files", files=len(pf))
    twice("q-filtered-page-pruned-list", f"SELECT id, ts, message FROM {plist(pf)} WHERE level=5 AND source=3 ORDER BY {KEY} LIMIT 200;")
    twice("q-top-first-page-view", f"SELECT id, ts, message FROM events ORDER BY {KEY} LIMIT 200;")
TK = "coalesce(e.ts, 9223372036854775807), e.source, e.line, e.id"
FROM_ = "events_wide e JOIN sources s ON s.id = e.source"
def same():
    # P1-1: one window on every side; the old unbounded query is kept to show what the earlier 100x compared.
    mid = T0 + SPAN // 2; hi = mid + SPAN // 500; cols = "id, ts, level, source, message"
    twice("s-view-no-upper-bound", f"SELECT {cols} FROM events WHERE ts >= {mid} ORDER BY {KEY} LIMIT 200;")
    twice("s-view-same-window", f"SELECT {cols} FROM events WHERE ts >= {mid} AND ts <= {hi} ORDER BY {KEY} LIMIT 200;")
    pf = run("x", QUERY + f"SELECT file FROM segments WHERE ts_max >= {mid} AND ts_min <= {hi};", readonly=True, record=False)
    pf = re.findall(r"^.*\.parquet$", pf, re.M); log(step="s-window-files", files=len(pf), of=len(files()))
    twice("s-pruned-same-window", f"SELECT {cols} FROM {plist(pf)} WHERE ts >= {mid} AND ts <= {hi} ORDER BY {KEY} LIMIT 200;")
    # The product's sequences (read.rs): filtered page, then the page while importing (no ord).
    w = "e.level = 5 AND e.source = 3"
    twice("s-real-filtered-count", f"SELECT count(*) FROM {FROM_} WHERE {w};")
    twice("s-real-filtered-ids-off0", f"SELECT e.id FROM {FROM_} WHERE {w} ORDER BY {TK} LIMIT 200 OFFSET 0;")
    twice("s-real-filtered-ids-off-deep", f"SELECT e.id FROM {FROM_} WHERE {w} ORDER BY {TK} LIMIT 200 OFFSET 50000;")
    twice("s-real-importing-count", f"SELECT count(*) FROM {FROM_};")
    twice("s-real-importing-ids-off0", f"SELECT e.id FROM {FROM_} ORDER BY {TK} LIMIT 200 OFFSET 0;")
    twice("s-real-importing-ids-mid", f"SELECT e.id FROM {FROM_} ORDER BY {TK} LIMIT 200 OFFSET {N//2};")
def lake():
    L = f"{D}/lake"; shutil.rmtree(L, ignore_errors=True); os.makedirs(L); ext = f"{ROOT}/ext"
    pre = IMPORT + f"SET extension_directory='{ext}'; LOAD ducklake; ATTACH 'ducklake:{L}/cat.ducklake' AS lake (DATA_PATH '{L}/data', READ_ONLY false);\n"
    run("lake-install", f"SET extension_directory='{ext}'; INSTALL ducklake;", db=None)
    ddl = """CREATE TABLE lake.events (id UBIGINT, ts BIGINT, ts_state UTINYINT, level UTINYINT, source UINTEGER, line UINTEGER, byte_offset UBIGINT, message VARCHAR, template VARCHAR, exception VARCHAR, markers UBIGINT, raw VARCHAR, extra MAP(VARCHAR, VARCHAR));"""
    run("lake-create", pre + ddl, db=None)
    # A copy of the links, so the lake's ownership of the files never touches the view store's segments.
    LS = f"{L}/segs"; os.makedirs(LS)
    fs = files()
    for f in fs: os.link(f, f"{LS}/{os.path.basename(f)}")
    run("lake-add-all", pre + f"CALL ducklake_add_data_files('lake', 'events', '{LS}/*.parquet');", db=None)
    log(step="lake-catalog-size", bytes=size(f"{L}/cat.ducklake"))
    q = pre.replace("768MB", "192MB")
    mid_ts = T0 + SPAN // 2; bucket = SPAN // 1000
    def tw(name, s): run(name, q + s + "\n" + s + "\n", db=None)
    tw("lake-bind-only", "SELECT * FROM lake.events LIMIT 0;")
    tw("lake-count", "SELECT count(*) FROM lake.events;")
    tw("lake-1000-buckets-scan", f"SELECT ts//{bucket}, level, count(*) FROM lake.events WHERE ts IS NOT NULL GROUP BY ALL;")
    tw("lake-keyset-mid", f"SELECT id, ts, level, source, message FROM lake.events WHERE ts >= {mid_ts} AND ts <= {mid_ts + SPAN//500} ORDER BY {KEY} LIMIT 200;")
    tw("lake-filtered-page", f"SELECT id, ts, message FROM lake.events WHERE level=5 AND source=3 ORDER BY {KEY} LIMIT 200;")
    run("lake-remove-source", pre + "DELETE FROM lake.events WHERE source=3;", db=None)
    log(step="lake-size-after-delete", catalog=size(f"{L}/cat.ducklake"), files=sum(len(f) for _, _, f in os.walk(f"{L}/data")))
    shutil.rmtree(L, ignore_errors=True)
def client(name, args, sql):
    p = subprocess.run([DUCK, "-no-agent", "-csv"] + args, input=sql, capture_output=True, text=True)
    txt = (p.stdout + p.stderr).strip().replace(CAT, "<cat>").replace(D, "<store>")
    log(step=name, rc=p.returncode, out=txt[:220])
def mgmt():
    # A plays Debrief: a read-write connection held open, with a committed change still in the WAL.
    a = subprocess.Popen([DUCK, "-no-agent", "-csv", CAT], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    a.stdin.write("SET checkpoint_threshold='1GB'; CREATE OR REPLACE TABLE marker(x INTEGER); INSERT INTO marker VALUES (42); SELECT 'ready';\n"); a.stdin.flush()
    while "ready" not in a.stdout.readline(): pass
    client("client-readonly-open-while-held", ["-readonly", CAT], "SELECT count(*) FROM sources;")
    client("client-attach-readonly-while-held", [], f"ATTACH '{CAT}' AS c (READ_ONLY); SELECT count(*) FROM c.sources;")
    client("client-parquet-direct-while-held", [], f"SELECT count(*) FROM read_parquet('{SEG}/*.parquet', union_by_name=true);")
    copy = f"{D}/copy.duckdb"; shutil.copyfile(CAT, copy)
    client("client-copy-without-wal", ["-readonly", copy], "SELECT count(*) FROM sources; SELECT count(*) FROM events; SELECT count(*) FROM marker;")
    if os.path.exists(CAT + ".wal"):
        log(step="wal-present", bytes=os.path.getsize(CAT + ".wal"))
        shutil.copyfile(CAT + ".wal", copy + ".wal")
        client("client-copy-with-wal-readwrite", [copy], "SELECT count(*) FROM marker;")
    a.stdin.write("DROP TABLE marker;\n.exit\n"); a.stdin.flush(); a.wait()
    for x in (copy, copy + ".wal"):
        if os.path.exists(x): os.remove(x)
    client("client-readonly-after-close", ["-readonly", CAT], "SELECT count(*) FROM events;")
    # v1.5.6 client on the pinned format and on the default 2.0 format.
    old = DUCK.replace("v200", "v156")
    for ver, opt in (("pinned", ["-storage-version", "v1.5.0"]), ("default", [])):
        f = f"{D}/fmt-{ver}.duckdb"
        subprocess.run([DUCK, "-no-agent"] + opt + [f], input="CREATE TABLE t(x INTEGER); INSERT INTO t VALUES (1); CHECKPOINT;", capture_output=True, text=True)
        p = subprocess.run([old, "-csv", "-readonly", f], input="SELECT count(*) FROM t;", capture_output=True, text=True)
        log(step=f"old-client-1.5.6-on-{ver}", rc=p.returncode, out=(p.stdout + p.stderr).strip().replace(D, "<store>")[:200]); os.remove(f)
    # DuckLake on a SQLite catalog: can a second process read while the first holds it?
    L = f"{D}/lakesql"; shutil.rmtree(L, ignore_errors=True); os.makedirs(L); ext = f"{ROOT}/ext"
    pre = f"SET extension_directory='{ext}'; LOAD ducklake; LOAD sqlite;"
    r = subprocess.run([DUCK, "-no-agent"], input=f"SET extension_directory='{ext}'; INSTALL sqlite; INSTALL ducklake;", capture_output=True, text=True)
    a = subprocess.Popen([DUCK, "-no-agent", "-csv"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    a.stdin.write(f"{pre} ATTACH 'ducklake:sqlite:{L}/cat.sqlite' AS lake (DATA_PATH '{L}/data'); CREATE TABLE lake.t(x INTEGER); INSERT INTO lake.t VALUES (1),(2); SELECT 'ready';\n"); a.stdin.flush()
    for _ in range(50):
        l = a.stdout.readline()
        if "ready" in l: break
        if "Error" in l: log(step="lake-sqlite-error", out=l[:200]); break
    client("lake-sqlite-second-process-read", [], f"{pre} ATTACH 'ducklake:sqlite:{L}/cat.sqlite' AS lake (DATA_PATH '{L}/data', READ_ONLY); SELECT count(*) FROM lake.t;")
    a.stdin.write("INSERT INTO lake.t VALUES (3);\n.exit\n"); a.stdin.flush(); a.wait()
    log(step="lake-sqlite-parquet-visible", parquet=sum(f.endswith(".parquet") for _, _, fs in os.walk(L) for f in fs))
    shutil.rmtree(L, ignore_errors=True)
for s in STEPS:
    t = time.time(); log(step=f"begin-{s}")
    {"gen": gen, "catalog": catalog, "ord": ord_, "hist": hist, "queries": queries, "lake": lake, "mgmt": mgmt, "same": same}[s]()
    log(step=f"end-{s}", secs=round(time.time() - t, 1))
