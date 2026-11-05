#!/usr/bin/env python3
"""Invented text bundle, the upper bound of the bundle's shape (ER-14): every file parses.
usage: gen_mixed.py OUT [files=14255] [bytes=8171319343] [families=64] [days=60]
Families have rolled series: each rolled file covers the next slice of its family's time span, so a file is narrow in
time and a family is wide. File counts per family and sizes are heavy tailed. Deterministic (seeded). All text invented."""
import datetime as dt, os, random, sys
from concurrent.futures import ProcessPoolExecutor
OUT = sys.argv[1]; FILES = int(sys.argv[2]) if len(sys.argv) > 2 else 14255; BYTES = int(sys.argv[3]) if len(sys.argv) > 3 else 8171319343
FAMS = int(sys.argv[4]) if len(sys.argv) > 4 else 64; DAYS = int(sys.argv[5]) if len(sys.argv) > 5 else 60
T0 = dt.datetime(2026, 6, 1)
LV = ["INFO"] * 11 + ["DEBUG"] * 6 + ["WARN"] * 2 + ["ERROR"]
W = "alpha bravo charlie delta echo service worker queue module driver buffer session request reply timeout retry connect started stopped failed ready pending".split()
def plan():
    r = random.Random(7)
    wf = [1 / (i + 1) ** 1.1 for i in range(FAMS)]
    counts = [max(1, int(FILES * w / sum(wf))) for w in wf]
    counts[0] += FILES - sum(counts)
    sizes = [r.lognormvariate(10.5, 2.0) for _ in range(FILES)]
    CAP = 48 << 20  # above this the int worker's text path is the folder-repro agent's open crash, not this question
    for _ in range(6):
        k = BYTES / sum(sizes); sizes = [min(CAP, max(200, s * k)) for s in sizes]
    sizes = [int(s) for s in sizes]; r.shuffle(sizes)
    jobs = []; at = 0
    for f, n in enumerate(counts):
        start = r.uniform(0, DAYS * 0.3) ; span = r.uniform(DAYS * 0.2, DAYS - start)
        for i in range(n):
            jobs.append((f, i, n, start + span * i / n, span / n, sizes[at])); at += 1
    return jobs
def one(job):
    f, i, n, start, slice_days, size = job
    d = f"{OUT}/family{f:02d}"; os.makedirs(d, exist_ok=True)
    r = random.Random(f * 100003 + i)
    est = max(1, size // 143); step = slice_days * 86400.0 / est; t = T0 + dt.timedelta(days=start)
    us = dt.timedelta(seconds=step); buf = []; wrote = 0; lines = 0; pick = r.choice; ri = r.randrange
    with open(f"{d}/app.{i:05d}.log", "w") as o:
        while wrote < size:
            l = f"{t:%Y-%m-%d %H:%M:%S},{t.microsecond // 1000:03d} [{ri(40)}] {pick(LV):<5} Comp{f}.Mod{ri(8)} - {pick(W)} {pick(W)} order={ri(1000003)} took {ri(4096)} ms host=h{f % 16}\n"
            buf.append(l); wrote += len(l); lines += 1; t += us
            if len(buf) >= 4096: o.write("".join(buf)); buf.clear()
        o.write("".join(buf))
    return lines
if __name__ == "__main__":
    jobs = plan()
    with ProcessPoolExecutor(4) as e: total = sum(e.map(one, jobs, chunksize=32))
    print({"files": len(jobs), "lines": total, "bytes_asked": BYTES})
