"""usage: python3 -I cputl.py RUN.csv [BUCKET_S] -- cores busy per time bucket of the root process (from procsample.py), and peak footprint."""
import sys, csv
rows = [r for r in csv.DictReader(open(sys.argv[1]))]
b = float(sys.argv[2]) if len(sys.argv) > 2 else 2.0
root = rows[0]["pid"]; rows = [r for r in rows if r["pid"] == root]
t0 = int(rows[0]["t_ms"]); out = []; prev = rows[0]; edge = b
for r in rows[1:]:
    t = (int(r["t_ms"]) - t0) / 1000
    if t >= edge or r is rows[-1]:
        dt = (int(r["t_ms"]) - int(prev["t_ms"])) / 1000
        cores = (int(r["cpu_ns"]) - int(prev["cpu_ns"])) / 1e9 / dt if dt > 0 else 0
        out.append((t, cores, int(r["footprint"]) >> 20)); prev = r; edge = t + b
for t, c, fp in out:
    print(f"{t:6.1f}s {c:5.1f} cores {'#' * int(round(c * 2))} {fp} MB")
print("total CPU %.0f s, wall %.1f s, mean %.1f cores, peak footprint %d MB" % (
    int(rows[-1]["cpu_ns"]) / 1e9, (int(rows[-1]["t_ms"]) - t0) / 1000,
    int(rows[-1]["cpu_ns"]) / 1e9 / max(0.001, (int(rows[-1]["t_ms"]) - t0) / 1000),
    max(int(r["max_footprint"]) for r in rows) >> 20))
