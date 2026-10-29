"""Item 3 queries: a common key (MachineName) and a rare key (Outcome, 206 of 3M rows) over the same events stored four ways
(plus today's promoted column). p50 / p95 over 2 rounds x 7 repetitions, 192 MB limit, 4 threads, like fmt/suite.py."""
import subprocess, re, statistics
T = "/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
D = "/Volumes/d7sst7/dev-targets/duck2-bench"; V = D + "/var"
MAPK = lambda k: "extra['%s']" % k
VARK = lambda k: "variant_extract(extra, '%s')::VARCHAR" % k
Q = [("common facet", "SELECT count(*) FROM (SELECT {C} AS k, count(*) FROM events GROUP BY 1 ORDER BY 2 DESC LIMIT 20)"),
     ("common filter", "SELECT count(*) FROM events WHERE {C} = 'WEB03'"),
     ("rare exists", "SELECT count(*) FROM events WHERE {R} IS NOT NULL"),
     ("rare rows", "SELECT count(*) FROM (SELECT id, {R} FROM events WHERE {R} IS NOT NULL LIMIT 200)")]
VAR = {
 "today: Parquet MAP + view": ("ATTACH '%s/store-ro/data.duckdb' AS s (READ_ONLY); USE s; SET GLOBAL parquet_metadata_cache=true;" % D, MAPK),
 "today: promoted p_ column": ("ATTACH '%s/store-ro/data.duckdb' AS s (READ_ONLY); USE s; SET GLOBAL parquet_metadata_cache=true; CREATE TEMP VIEW events AS SELECT * FROM s.events_wide;" % D,
                               lambda k: '"p_%s"' % k if k == "MachineName" else MAPK(k)),
 "Parquet MAP (COPY)": ("CREATE VIEW events AS SELECT * FROM '%s/map.parquet';" % V, MAPK),
 "Parquet VARIANT (COPY)": ("CREATE VIEW events AS SELECT * FROM '%s/var.parquet';" % V, VARK),
 "native v2 MAP": ("ATTACH '%s/nmap.duckdb' AS s (READ_ONLY); USE s;" % V, MAPK),
 "native v2 VARIANT": ("ATTACH '%s/nvar.duckdb' AS s (READ_ONLY); USE s;" % V, VARK),
 "arrow-rs VARIANT (id, extra)": ("CREATE VIEW events AS SELECT * FROM '%s/arrow-variant.parquet';" % V, VARK),
 "arrow-rs shredded VARIANT (id, extra)": ("CREATE VIEW events AS SELECT * FROM '%s/arrow-variant-shredded.parquet';" % V, VARK),
}
REPS = 7
res = {v: {q: [] for q, _ in Q} for v in VAR}; chk = {}
for rnd in range(2):
    for v, (setup, key) in VAR.items():
        sql = "SET memory_limit='192MB'; SET threads=4; SET temp_directory='%s/tmpq';\n%s\n.timer on\n" % (D, setup)
        for q, s in Q:
            for _ in range(REPS):
                sql += s.replace("{C}", key("MachineName")).replace("{R}", key("Outcome")) + ";\n"
        p = subprocess.run([T, "-no-agent", "-csv"], input=sql, capture_output=True, text=True)
        times = [float(m) for m in re.findall(r"Run Time \(s\): real ([0-9.]+)", p.stdout)]
        if len(times) != REPS * len(Q):
            print("ERROR", v, p.stderr[:400]); continue
        vals = [l for l in p.stdout.splitlines() if l.strip().isdigit()]
        chk[v] = vals[::REPS][:len(Q)]
        n = 0
        for q, _ in Q:
            res[v][q] += times[n:n + REPS]; n += REPS
def pct(a, p): a = sorted(a); return a[min(len(a) - 1, int(round(p * (len(a) - 1))))] if a else float("nan")
print("| Query | " + " | ".join(VAR) + " |"); print("|---|" + "---|" * len(VAR))
for q, _ in Q:
    print("| %s | " % q + " | ".join("%.0f / %.0f" % (1000 * statistics.median(res[v][q]) if res[v][q] else float('nan'), 1000 * pct(res[v][q], .95)) for v in VAR) + " |")
print("results (must agree):", chk)
