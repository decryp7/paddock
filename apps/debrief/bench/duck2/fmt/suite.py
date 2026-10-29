import subprocess,re,sys,statistics,os
T="/Volumes/d7sst7/dev-targets/research-data/tools/v200/duckdb"
D="/Volumes/d7sst7/dev-targets/duck2-bench"; F=D+"/fmt"
Q=[("count","SELECT count(*) FROM events"),
("level facet","SELECT count(*) FROM (SELECT level, count(*) FROM events GROUP BY ALL)"),
("minute x level","SELECT count(*) FROM (SELECT ts // 60000 AS m, level, count(*) FROM events WHERE ts IS NOT NULL GROUP BY ALL)"),
("first page","SELECT count(*) FROM (SELECT id, ts, level, source, message FROM events ORDER BY coalesce(ts, 9223372036854775807), source, line, id LIMIT 200)"),
("extra facet","SELECT count(*) FROM (SELECT EXTRA AS k, count(*) FROM events GROUP BY 1 ORDER BY 2 DESC LIMIT 20)"),
("ILIKE","SELECT count(*) FROM events WHERE message ILIKE '%timeout%'"),
("regex","SELECT count(*) FROM events WHERE regexp_matches(message, 'took [0-9]{3} ms')"),
("by id","SELECT count(*) FROM events WHERE id = 1234567"),
("deep page","SELECT count(*) FROM (SELECT id FROM events ORDER BY coalesce(ts, 9223372036854775807), source, line, id LIMIT 200 OFFSET 2000000)")]
REPS=7
V={"parquet+view":("ATTACH '%s/store-ro/data.duckdb' AS s (READ_ONLY); USE s; SET GLOBAL parquet_metadata_cache=true;"%D,"extra['MachineName']"),
"native v2.0.0":("ATTACH '%s/n2.duckdb' AS s (READ_ONLY); USE s;"%F,"extra['MachineName']"),
"native v1.5.0":("ATTACH '%s/n15.duckdb' AS s (READ_ONLY); USE s;"%F,"extra['MachineName']"),
"vortex":("LOAD vortex; CREATE VIEW events AS SELECT * FROM read_vortex('%s/ev.vortex');"%F,"json_extract_string(extra,'$.MachineName')")}
res={v:{q:[] for q,_ in Q} for v in V}; rss={v:[] for v in V}
for rnd in range(2):
  for v,(setup,ex) in V.items():
    sql="SET memory_limit='192MB'; SET threads=4; SET temp_directory='%s/tmpq';\n%s\n.timer on\n"%(D,setup)
    for q,s in Q:
      for _ in range(REPS): sql+=s.replace("EXTRA",ex)+";\n"
    p=subprocess.run(["/usr/bin/time","-l",T,"-no-agent","-csv"],input=sql,capture_output=True,text=True)
    times=[float(m) for m in re.findall(r"Run Time \(s\): real ([0-9.]+)",p.stdout)]
    n=0
    for q,_ in Q:
      res[v][q]+=times[n:n+REPS]; n+=REPS
    m=re.search(r"(\d+)\s+maximum resident",p.stderr); rss[v].append(int(m.group(1))>>20 if m else 0)
def pct(a,p): a=sorted(a); return a[min(len(a)-1,int(round(p*(len(a)-1))))]
print("| Query | "+" | ".join(V)+" |"); print("|---|"+"---|"*len(V))
for q,_ in Q:
  print("| %s | "%q+" | ".join("%.0f / %.0f ms"%(1000*statistics.median(res[v][q]),1000*pct(res[v][q],0.95)) for v in V)+" |")
print("| peak RSS (MB) | "+" | ".join("%d to %d"%(min(rss[v]),max(rss[v])) for v in V)+" |")
