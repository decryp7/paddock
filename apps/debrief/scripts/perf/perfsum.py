"""Summarises a procsample.py CSV: per process the peak physical footprint, peak RSS and CPU seconds; with a log that
holds `PHASE <name> <unix ms>` lines, the same per phase (CPU seconds, mean CPU %, wake-ups per second).

usage: python3 -I perfsum.py SAMPLES_CSV [PHASE_LOG]
"""
import csv
import sys
from collections import defaultdict

MB = 1 << 20


def load(path):
    rows = defaultdict(list)
    with open(path) as f:
        for r in csv.DictReader(f):
            rows[(r["pid"], r["name"])].append({k: (int(v) if k not in ("name",) else v) for k, v in r.items()})
    return rows


def phases(path):
    out = []
    with open(path) as f:
        for line in f:
            if line.startswith("PHASE "):
                _, name, ms = line.split()
                out.append((name, int(ms)))
    return out


def window(samples, a, b):
    s = [x for x in samples if a <= x["t_ms"] <= b]
    if len(s) < 2:
        return None
    secs = (s[-1]["t_ms"] - s[0]["t_ms"]) / 1000
    cpu = (s[-1]["cpu_ns"] - s[0]["cpu_ns"]) / 1e9
    wk = (s[-1]["idle_wkups"] + s[-1]["intr_wkups"] - s[0]["idle_wkups"] - s[0]["intr_wkups"])
    return {
        "secs": secs,
        "cpu": cpu,
        "pct": 100 * cpu / secs if secs else 0,
        "wk_s": wk / secs if secs else 0,
        "fp_peak": max(x["footprint"] for x in s) / MB,
        "fp_end": s[-1]["footprint"] / MB,
        "rss_peak": max(x["resident"] for x in s) / MB,
    }


def main():
    rows = load(sys.argv[1])
    print("process | samples | wall s | CPU s | peak footprint MB | lifetime peak footprint MB | peak RSS MB | end footprint MB")
    for (pid, name), s in rows.items():
        wall = (s[-1]["t_ms"] - s[0]["t_ms"]) / 1000
        print(f"{name}({pid}) | {len(s)} | {wall:.1f} | {s[-1]['cpu_ns'] / 1e9:.1f} | "
              f"{max(x['footprint'] for x in s) / MB:.0f} | {max(x['max_footprint'] for x in s) / MB:.0f} | "
              f"{max(x['resident'] for x in s) / MB:.0f} | {s[-1]['footprint'] / MB:.0f}")
    if len(sys.argv) > 2:
        ph = phases(sys.argv[2])
        print("\nphase | process | s | CPU s | CPU % | wake-ups/s | peak footprint MB | end footprint MB | peak RSS MB")
        for i, (name, a) in enumerate(ph):
            if name.endswith("-end"):
                continue
            b = ph[i + 1][1] if i + 1 < len(ph) else 10 ** 15
            for (pid, pname), s in rows.items():
                w = window(s, a, b)
                if w:
                    print(f"{name} | {pname} | {w['secs']:.1f} | {w['cpu']:.2f} | {w['pct']:.1f} | {w['wk_s']:.1f} | "
                          f"{w['fp_peak']:.0f} | {w['fp_end']:.0f} | {w['rss_peak']:.0f}")


main()
