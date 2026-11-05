#!/usr/bin/env python3
"""Measures a store the product wrote (or its multiplied copy) with the product's own statements (read.rs, pos.rs, sql.rs),
through the DuckDB 2.0.0 CLI, read-only. usage: real.py LABEL STORE [--scan] [--cli]. Generated data only (ER-14)."""
import json, os, random, re, shutil, subprocess, sys, time
ROOT = "/Volumes/d7sst7/dev-targets/scratch/storage-research"; DUCK = "/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
LABEL, S = sys.argv[1], sys.argv[2]; OPTS = sys.argv[3:]
CAT = f"{S}/data.duckdb"; SEG = f"{S}/segments"; OUT = f"{ROOT}/results/{LABEL}.jsonl"; AN = f"{ROOT}/data/an-{LABEL}.duckdb"; TMP = f"{ROOT}/data/tmp-{LABEL}"
os.makedirs(TMP, exist_ok=True)
Q = f"SET memory_limit='192MB'; SET threads=4; SET async_threads=4; SET temp_directory='{TMP}'; SET GLOBAL parquet_metadata_cache=true;\n"
FROM = "events_wide e JOIN sources s ON s.id = e.source"; READY = "source IN (SELECT id FROM sources WHERE ready)"
ORD = "ORDER BY coalesce(e.ts, 9223372036854775807), e.source, e.line, e.id"
COLS = "e.id, e.ts, e.ts_state, e.level, e.source, e.line, e.byte_offset, e.message, e.template, e.exception, e.markers, e.extra"
def log(**k):
    k.update(t=time.strftime("%H:%M:%S"), load=round(os.getloadavg()[0], 1)); print(json.dumps(k), flush=True); open(OUT, "a").write(json.dumps(k) + "\n")
def guard():
    if shutil.disk_usage(ROOT).free < 200 << 30: sys.exit("::error::d7 under 200 GiB free; stopping")
def cli(sql, db=CAT, ro=True, name=None, timeout=1500):
    guard()
    a = ["/usr/bin/time", "-l", DUCK, "-no-agent", "-csv"] + (["-readonly"] if ro else []) + [db]
    t = time.time()
    try: p = subprocess.run(a, input=sql, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        if name: log(step=name, timeout=timeout)
        return ""
    err = [l for l in (p.stdout + p.stderr).splitlines() if "Error" in l]
    if name:
        pk = re.search(r"(\d+)\s+peak memory footprint", p.stderr); rs = re.search(r"(\d+)\s+maximum resident", p.stderr)
        r = dict(step=name, runs=[float(x) for x in re.findall(r"real ([0-9.]+)", p.stdout + p.stderr)][-3:], peak_mib=int(pk.group(1)) >> 20 if pk else None, rss_mib=int(rs.group(1)) >> 20 if rs else None, wall=round(time.time() - t, 2))
        if err: r["error"] = err[0][:240]
        log(**r)
    elif err: log(step="sql-error", error=err[0][:300], sql=sql[:200])
    return p.stdout
def val(sql, db=CAT):
    return [l for l in cli(Q + sql, db=db).splitlines()][1:]
def twice(name, sql):  # one process: first bind, then with the metadata cache warm, as the long-lived worker has it. OS cache warm.
    cli(Q + ".mode trash\n.timer on\n" + sql + "\n" + sql + "\n", name=name)
def plist(fs): return "read_parquet([" + ", ".join(f"'{f}'" for f in fs) + "], union_by_name = true)"
def files_of(sql):
    return [f"{SEG}/{x}" for x in val(sql)]
def shape():
    log(step="shape", segments=val("SELECT count(*), sum(rows), min(rows), median(rows), max(rows) FROM segments;"), sources=val("SELECT count(*), count(*) FILTER (WHERE ready) FROM sources;"),
        histogram=val("SELECT count(*) FROM histogram;"), fields=val("SELECT count(*) FROM segment_fields;"), ord=val("SELECT count(*) FROM ord;"), promoted=val("SELECT max(length(promoted)) FROM segments;"), cat_bytes=os.path.getsize(CAT),
        seg_bytes=sum(os.path.getsize(f"{SEG}/{f}") for f in os.listdir(SEG)), seg_files=len(os.listdir(SEG)), index_bytes=sum(os.path.getsize(os.path.join(r, f)) for r, _, fs in os.walk(f"{S}/index") for f in fs))
    log(step="file-minute", rows=val("SELECT count(*) FROM (SELECT DISTINCT file, minute FROM histogram);"), file_source=val("SELECT count(*) FROM (SELECT DISTINCT file, source FROM histogram);"))
    log(step="columns", cols=val("SELECT count(*) FROM (DESCRIBE events_wide);"), sources_per_file=val("SELECT median(n), max(n) FROM (SELECT file, count(DISTINCT source) n FROM histogram GROUP BY file);"))
def zone():
    if os.path.exists(AN): os.remove(AN)
    st = lambda c, f: f"{f}(try_cast(stats_{f}_value AS BIGINT)) FILTER (path_in_schema='{c}')"
    cli(f"""SET threads=4; SET memory_limit='1GB'; SET temp_directory='{TMP}';
CREATE TABLE rg AS SELECT parse_filename(file_name) file, row_group_id rg, max(row_group_num_rows) nrows, {st('ts','min')} ts_min, {st('ts','max')} ts_max, {st('source','min')} src_min, {st('source','max')} src_max, {st('level','min')} lvl_min, {st('level','max')} lvl_max
 FROM parquet_metadata('{SEG}/*.parquet') GROUP BY ALL;
CREATE TABLE fz AS SELECT file, sum(nrows) nrows, min(ts_min) ts_min, max(ts_max) ts_max, min(src_min) src_min, max(src_max) src_max, min(lvl_min) lvl_min, max(lvl_max) lvl_max FROM rg GROUP BY file;
ATTACH '{CAT}' AS c (READ_ONLY); CREATE TABLE h AS SELECT file, minute, level, source, n FROM c.histogram;""", db=AN, ro=False, name="zone-build")
    v = val("SELECT count(*), min(ts_min), max(ts_max), median(ts_max-ts_min), quantile_cont(ts_max-ts_min,0.9) FROM fz;", db=AN)[0].split(",")
    nf, lo, hi = int(v[0]), int(v[1]), int(v[2]); span = hi - lo
    log(step="file-spans", files=nf, span_ms=span, median_file_span_share=int(float(v[3])) / span, p90=int(float(v[4])) / span, rgs=val("SELECT count(*), median(ts_max-ts_min) FROM rg;", db=AN))
    r = random.Random(11)
    # Windows centred where events are: minutes drawn by weight from the histogram.
    mins = [int(x) for x in val("SELECT minute FROM (SELECT minute, sum(n) w FROM h WHERE minute IS NOT NULL GROUP BY minute) USING SAMPLE 100 ROWS (reservoir, 11);", db=AN)]
    for den in (5000, 500, 50):
        width = span // den; res = {"mm": [], "rg": [], "hist": [], "rows_win": [], "rows_mm": [], "rows_hist": []}
        vals = ",".join(f"({m*60000},{m*60000+width})" for m in mins)
        out = val(f"""WITH w(lo,hi) AS (VALUES {vals})
 SELECT quantile_cont(a,0.5), quantile_cont(a,0.9), quantile_cont(b,0.5), quantile_cont(b,0.9), quantile_cont(c,0.5), quantile_cont(c,0.9), quantile_cont(ra,0.5), quantile_cont(rc,0.5), quantile_cont(rw,0.5) FROM (
 SELECT w.lo, (SELECT count(*) FROM fz WHERE ts_max>=lo AND ts_min<hi) a, (SELECT coalesce(sum(nrows),0) FROM fz WHERE ts_max>=lo AND ts_min<hi) ra, (SELECT count(*) FROM rg WHERE ts_max>=lo AND ts_min<hi) b,
 (SELECT count(DISTINCT file) FROM h WHERE minute*60000+60000>lo AND minute*60000<hi) c,
 (SELECT coalesce(sum(nrows),0) FROM fz WHERE file IN (SELECT file FROM h WHERE minute*60000+60000>lo AND minute*60000<hi)) rc,
 (SELECT coalesce(sum(n),0) FROM h WHERE minute*60000+60000>lo AND minute*60000<hi) rw FROM w);""", db=AN)
        x = [float(t) for t in out[0].split(",")]
        log(step=f"gate-window-1/{den}", files=nf, minmax_median=x[0], minmax_p90=x[1], minmax_median_pct=round(100*x[0]/nf,2), rowgroups_median=x[2], rowgroups_p90=x[3], hist_files_median=x[4], hist_files_p90=x[5], hist_median_pct=round(100*x[4]/nf,2), rows_minmax=x[6], rows_hist=x[7], rows_in_window=x[8])
    srcs = val("SELECT source FROM (SELECT source, sum(n) w FROM h GROUP BY source) USING SAMPLE 100 ROWS (reservoir, 11);", db=AN); sl = ",".join(srcs)
    log(step="gate-source", files=nf, row=val(f"""SELECT median(a), quantile_cont(a,0.9), median(c), quantile_cont(c,0.9) FROM (SELECT s, (SELECT count(*) FROM fz WHERE s BETWEEN src_min AND src_max) a, (SELECT count(DISTINCT file) FROM h WHERE source=s) c FROM (SELECT unnest([{sl}]) s));""", db=AN))
    log(step="gate-level", files=nf, row=val("SELECT l, (SELECT count(*) FROM fz WHERE l BETWEEN lvl_min AND lvl_max), (SELECT count(DISTINCT file) FROM h WHERE level=l), (SELECT sum(n) FROM h WHERE level=l) FROM (SELECT DISTINCT level l FROM h) ORDER BY l;", db=AN))
def pick():
    v = val("SELECT source, level, sum(n) c FROM histogram GROUP BY ALL HAVING c BETWEEN 1000 AND 100000 ORDER BY c DESC LIMIT 1;")
    s, l, c = v[0].split(",") if v else (val("SELECT source FROM histogram GROUP BY 1 ORDER BY sum(n) DESC LIMIT 1;")[0], "2", "0")
    m = val("SELECT quantile_disc(minute, 0.5) FROM histogram WHERE minute IS NOT NULL;")[0]
    return s, l, int(c), int(m)
def seq():
    n = int(val("SELECT sum(rows) FROM segments;")[0]); s, l, c, m = pick(); span = [int(x) for x in val("SELECT min(minute), max(minute) FROM histogram;")[0].split(",")]; width = max(1, (span[1]-span[0]) // 500); lo, hi = m*60000, (m+width)*60000
    log(step="picked", n=n, source=s, level=l, count=c, lo=lo, hi=hi)
    # ctx(): every page (read.rs:188-247). Ids at file_list: rows_by_id's catalogue read.
    twice("ctx-sources", "SELECT id, name FROM sources;")
    twice("ctx-promoted", "SELECT file, promoted FROM segments;")
    twice("ctx-fields", "SELECT file, key FROM segment_fields;")
    twice("rows-by-id-file-list", "SELECT file, first_id, end_id FROM segments;")
    twice("view-bind", "SELECT * FROM events_wide LIMIT 0;")
    def rows(name, ids):
        if not ids: log(step=name, skipped="no ids"); return
        sids = sorted(int(i) for i in ids)
        fl = val("SELECT file, first_id, end_id FROM segments;"); fs = []
        for row in fl:
            f, a, b = row.rsplit(",", 2); a, b = int(a), int(b)
            if any(a <= i < b for i in sids): fs.append(f"{SEG}/{f}")
        log(step=name + "-files", files=len(fs)); twice(name, f"SELECT {COLS} FROM {plist(fs)} e WHERE e.id IN ({','.join(ids)}) AND {READY};")
    # Unfiltered, not importing: ord.
    has_ord = int(val("SELECT count(*) FROM ord;")[0])
    if has_ord:
        twice("u-count-ord", "SELECT count(*) FROM ord;"); q = f"SELECT id FROM ord WHERE pos >= {has_ord//2} AND pos < {has_ord//2+200} ORDER BY pos;"; twice("u-ids-ord", q); rows("u-rows", val(q))
    # Filtered exact: read.rs:600-644.
    for tag, w in (("f-source-level", f"e.source = {s} AND e.level = {l}"), ("f-level", f"e.level = {l}"), ("f-window", f"e.ts >= {lo} AND e.ts < {hi}")):
        twice(f"{tag}-count", f"SELECT count(*) FROM {FROM} WHERE {w};")
        q = f"SELECT e.id FROM {FROM} WHERE {w} {ORD} LIMIT 200 OFFSET 0;"; twice(f"{tag}-ids", q)
        if tag == "f-source-level": rows(f"{tag}-rows", val(q))
    twice("f-level-ids-deep", f"SELECT e.id FROM {FROM} WHERE e.level = {l} {ORD} LIMIT 200 OFFSET {n//20};")
    # While importing, or without ord: w is TRUE.
    twice("i-count", f"SELECT count(*) FROM {FROM};"); twice("i-ids-first", f"SELECT e.id FROM {FROM} {ORD} LIMIT 200 OFFSET 0;"); twice("i-ids-mid", f"SELECT e.id FROM {FROM} {ORD} LIMIT 200 OFFSET {n//2};")
    # Pruned variants of the window and source+level pages, same predicate.
    fm = files_of(f"SELECT file FROM an.fz WHERE ts_max >= {lo} AND ts_min < {hi};".replace("an.fz", "fz")) if False else [f"{SEG}/{x}" for x in val(f"SELECT file FROM fz WHERE ts_max >= {lo} AND ts_min < {hi};", db=AN)]
    fh = [f"{SEG}/{x}" for x in val(f"SELECT DISTINCT file FROM h WHERE minute*60000+60000 > {lo} AND minute*60000 < {hi};", db=AN)]
    log(step="window-files", minmax=len(fm), hist=len(fh))
    wq = lambda fs: f"SELECT e.id FROM {plist(fs)} e WHERE e.ts >= {lo} AND e.ts < {hi} AND e.{READY} {ORD} LIMIT 200;"
    if fm: twice("p-window-minmax", wq(fm))
    if fh: twice("p-window-hist", wq(fh))
    fs = [f"{SEG}/{x}" for x in val(f"SELECT DISTINCT file FROM h WHERE source = {s} AND level = {l};", db=AN)]; log(step="source-level-files", hist=len(fs))
    if fs: twice("p-source-level-hist", f"SELECT e.id FROM {plist(fs)} e WHERE e.source = {s} AND e.level = {l} AND e.{READY} {ORD} LIMIT 200;")
    twice("hist-select-window", f"SELECT DISTINCT file FROM histogram WHERE minute >= {lo//60000} AND minute < {hi//60000};")
    twice("hist-select-source-level", f"SELECT DISTINCT file FROM histogram WHERE source = {s} AND level = {l};")
    twice("hist-select-level", f"SELECT DISTINCT file FROM histogram WHERE level = {l};")
    twice("count-from-histogram", f"SELECT sum(n) FROM histogram WHERE source = {s} AND level = {l};")
    if "--scan" in OPTS: twice("t-text-scan-view", f"SELECT count(*) FROM {FROM} WHERE contains(e.message, 'order=123456');")
def sorted_():
    D = f"{ROOT}/data/sorted-{LABEL}"; shutil.rmtree(D, ignore_errors=True); os.makedirs(D)
    cli(f"SET memory_limit='768MB'; SET threads=4; SET temp_directory='{TMP}'; .timer on\nCOPY (SELECT e.* FROM events_wide e ORDER BY coalesce(e.ts, 9223372036854775807), e.source, e.line, e.id) TO '{D}' (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 65536, ROW_GROUPS_PER_FILE 2);".replace("; .timer on", ";\n.timer on"), name="sorted-rewrite")
    fs = sorted(f"{D}/{f}" for f in os.listdir(D)); log(step="sorted-size", files=len(fs), bytes=sum(os.path.getsize(f) for f in fs))
    c = f"{ROOT}/data/sorted-{LABEL}.duckdb"
    cli(f"CREATE TABLE fz AS SELECT file_name file, min(try_cast(stats_min_value AS BIGINT)) ts_min, max(try_cast(stats_max_value AS BIGINT)) ts_max FROM parquet_metadata('{D}/*.parquet') WHERE path_in_schema='ts' GROUP BY 1;", db=c, ro=False)
    lo, hi = [int(x) for x in val("SELECT min(ts_min), max(ts_max) FROM fz;", db=c)[0].split(",")]; w = (hi - lo) // 500
    log(step="sorted-gate", row=val(f"SELECT median(n), quantile_cont(n,0.9) FROM (SELECT (SELECT count(*) FROM fz WHERE ts_max>=a AND ts_min<a+{w}) n FROM (SELECT {lo}+i*{(hi-lo)//100} a FROM range(100) r(i)));", db=c))
    shutil.rmtree(D); os.remove(c)
def real_cli():
    B = f"{ROOT}/bin"; P = f"{ROOT}/profile/{LABEL}"; os.makedirs(f"{P}/tmp", exist_ok=True)
    env = dict(os.environ, HOME=P, TMPDIR=f"{P}/tmp", XDG_DATA_HOME=f"{P}/data", DEBRIEF_STORE_WORKER=f"{B}/debrief-store-worker", DEBRIEF_PLUGINS=f"{B}/debrief-plugins", DEBRIEF_LIB_DIR=B, DYLD_LIBRARY_PATH=B)
    s, l, c, m = pick()
    for name, ql in (("all", "*"), ("source-level", f"level = {['trace','debug','info','warn','error','fatal'][min(int(l),5)]}")):
        t = time.time(); p = subprocess.Popen([f"{B}/debrief", "query", ql, "--store", S, "--limit", "200", "--json", "--quiet"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        peak = 0
        while p.poll() is None:
            o = subprocess.run(["ps", "-axo", "ppid=,rss=,comm="], capture_output=True, text=True).stdout
            for ln in o.splitlines():
                a = ln.split(None, 2)
                if len(a) == 3 and a[0] == str(p.pid) and "worker" in a[2]: peak = max(peak, int(a[1]))
            time.sleep(0.1)
        out, err = p.communicate(); log(step=f"cli-query-{name}", wall=round(time.time()-t, 2), worker_rss_mib=peak >> 10, rc=p.returncode, tail=(out[-120:] if p.returncode else ""), err=err[-200:])
STEPS = {"shape": shape, "zone": zone, "seq": seq, "cli": real_cli, "sorted": sorted_}
for st in [o[2:] for o in OPTS if o[2:] in STEPS] or ["shape", "zone", "seq"]: log(step=f"begin-{st}"); STEPS[st](); log(step=f"end-{st}")
