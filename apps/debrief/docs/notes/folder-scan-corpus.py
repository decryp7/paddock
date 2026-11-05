#!/usr/bin/env python3
"""Invented corpus for the folder scan measurement of ADR 0067: 14,000 files and about 7.6 GiB in the mix of a large
diagnostic bundle. Every name and byte is made up. Files above 1.2 MiB have 1.2 MiB of real content and are sparse beyond it:
the scan never reads further. Usage: folder-scan-corpus.py OUT [seed]"""
import os, random, sys, zipfile

OUT = sys.argv[1]
rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 7)
POOL = rng.randbytes(1 << 21)
REAL = int(1.2 * (1 << 20))

def binary(n):
    at = rng.randrange(len(POOL) - min(n, 1 << 20) - 1)
    return POOL[at:at + n] if n <= 1 << 20 else (POOL[at:at + (1 << 20)] * (n // (1 << 20) + 1))[:n]

def repeat(make, n):
    out, size, i = [], 0, 0
    while size < n:
        line = make(i); out.append(line); size += len(line); i += 1
    return "".join(out)

def app_csv(i):  return f'2026/10/14,10:{i//60%60:02d}:{i%60:02d}.{i%1000:03d}+09:00,INFO,Worker{i%7},"Task {i} finished__next step {i+1}"\n'
def hist_csv(i): return f'2026/10/14 10:{i//60%60:02d}:{i%60:02d} +09:00,User{i%9},Changed setting {i} to café\n'
def local(i):    return f'2026-10-14 10:{i//60%60:02d}:{i%60:02d} INFO service tick {i}\n'
def xml(i):      return f'  <item id="{i}" name="Panel{i}" visible="true"/>\n'
def ini(i):      return f'Left={i}\r\nTop={i*2}\r\n'
def ctime(i):    return f'Wed Oct 14 10:{i//60%60:02d}:{i%60:02d} 2026;0x{i:04X};Information;Svc{i%5};Message {i}\n'
def monthfirst(i): return f'10/14/2026 10:{i//60%60:02d}:{i%60:02d} Scan {i}\n'

# (folder pattern, name pattern, count, mean bytes, content kind)
FAMILIES = [
    ("Bundle/Logs/AppA",        "AppA_{i:04d}.log",      610,   240_000, "app"),
    ("Bundle/Logs/Lifecycle",   "lifecycle_{i:05d}.log", 1340,  17_000,  "app"),
    ("Bundle/Logs/History/All", "all_{i:04d}.csv",       420,   28_000,  "hist"),
    ("Bundle/Logs/History/Summary", "summary_{i:04d}.csv", 293, 28_000,  "hist"),
    ("Bundle/Logs/Service",     "svc_{i:03d}.log",       60,    1_750_000, "local"),
    ("Bundle/Project/Windows/W{g:03d}", "win_{i:05d}.bin", 5100, 330_000, "bin"),
    ("Bundle/Project/Windows/W{g:03d}", "win_{i:05d}.xaml", 1100, 1_050_000, "utf16xml"),
    ("Bundle/Project/Data",     "tag_{i:05d}.dat",       1769,  105_000, "bin"),
    ("Bundle/Project/Images",   "pic_{i:04d}.bmp",       560,   300_000, "bmp"),
    ("Bundle/State/PerWindow",  "state_{i:05d}.ini",     1100,  370,     "ini"),
    ("Bundle/State/Trace",      "wm_{i:04d}.txt",        480,   700,     "utf16"),
    ("Bundle/Config",           "cfg_{i:04d}.xml",       900,   6_000,   "xml"),
    ("Bundle/Tools",            "scan_{i:03d}.txt",      190,   110_000, "monthfirst"),
    ("Bundle/Empty",            "none_{i:03d}.tmp",      46,    0,       "empty"),
    ("Bundle/System/Exports",   "Export{i}.txt",         3,     160_000_000, "ctime"),
    ("Bundle/System",           "Event{i}.evtx",         3,     1_060_000_000, "evtx"),
    ("Bundle/Dumps",            "crash{i}.dmp",          3,     310_000_000, "mdmp"),
    ("Bundle/Dumps/Small",      "mini{i:02d}.dmp",       19,    450_000, "mdmp"),
    ("Bundle/Archives",         "pack{i}.zip",           4,     200_000, "zip"),
]

def content(kind, n, i):
    n = min(n, REAL)
    if kind == "empty": return b""
    if kind == "app":   return repeat(app_csv, n).encode()[:n]
    if kind == "hist":  return repeat(hist_csv, n).encode("cp1252")[:n]
    if kind == "local": return repeat(local, n).encode()[:n]
    if kind == "bin":   return binary(n)
    if kind == "bmp":   return b"BM" + binary(max(0, n - 2))
    if kind == "utf16xml": return b"\xff\xfe" + repeat(xml, n // 2).encode("utf-16-le")[:n]
    if kind == "utf16": return b"\xff\xfe" + repeat(ini, n // 2).encode("utf-16-le")[:n]
    if kind == "xml":   return ('<?xml version="1.0"?>\n<root>\n' + repeat(xml, n)).encode()[:n]
    if kind == "ini":   return ("[Window]\r\n" + repeat(ini, n)).encode()[:n]
    if kind == "ctime": return repeat(ctime, n).encode()[:n]
    if kind == "monthfirst": return repeat(monthfirst, n).encode()[:n]
    if kind == "evtx":  return b"ElfFile\x00" + binary(n - 8)
    if kind == "mdmp":  return b"MDMP\x93\xa7" + binary(n - 6)
    raise ValueError(kind)

total = files = 0
for folder, name, count, mean, kind in FAMILIES:
    for i in range(count):
        d = os.path.join(OUT, folder.format(g=i // 40))
        os.makedirs(d, exist_ok=True)
        p = os.path.join(d, name.format(i=i))
        size = 0 if mean == 0 else int(mean * rng.uniform(0.4, 1.6)) if count > 3 else mean
        if kind == "zip":
            with zipfile.ZipFile(p, "w", zipfile.ZIP_DEFLATED) as z:
                for m in range(40):
                    z.writestr(f"logs/member_{m:02d}.log", repeat(local, 5000))
                    z.writestr(f"data/member_{m:02d}.bin", binary(2000))
            size = os.path.getsize(p)
        else:
            with open(p, "wb") as f:
                f.write(content(kind, size, i))
                if size > REAL: f.truncate(size)
        total += size; files += 1
print(f"{files} files, {total} bytes ({total / (1 << 30):.2f} GiB apparent)")
