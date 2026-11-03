# Synthetic disk-map tree in SQLite (ADR 0029): one row per node against packed 4 MiB chunks, raw and zlib level 1.
# Invented names and sizes only; no real paths. Usage: python3 -I disk_cache_sqlite_bench.py <nodes> <out-dir> (writes three .db files there).
import sqlite3, struct, random, time, os, sys, zlib
N = int(sys.argv[1]) if len(sys.argv) > 1 else 1_000_000
out = sys.argv[2]
random.seed(42)
# record: parent u32, kind u8, flags u8, name_len u16, bytes u64, logical u64, items u64, loose_bytes u64, loose_logical u64, loose_items u64, newest i64, dir_mtime i64
REC = struct.Struct('<IBBH8q')
names = []
recs = []
for i in range(N):
    parent = 0 if i == 0 else random.randrange(max(0, i - 5000), i)
    nm = ('folder-%x-%s' % (i, 'abcdefgh'[: i % 9])).encode()
    names.append(nm)
    recs.append((parent, i % 3, 0, len(nm), random.randrange(1 << 30), random.randrange(1 << 30), random.randrange(1000), 1, 2, 3, 1_700_000_000_000 + i, 1_700_000_000_000 - i))
def fresh(p):
    for s in ('', '-wal', '-shm'):
        try: os.remove(p + s)
        except FileNotFoundError: pass
    c = sqlite3.connect(p); c.execute('PRAGMA journal_mode=WAL'); c.execute('PRAGMA synchronous=NORMAL'); return c
res = {}
# A: one row per node
pa = os.path.join(out, 'rows.db'); c = fresh(pa)
c.execute('CREATE TABLE nodes (scan INTEGER, id INTEGER, parent INTEGER, kind INTEGER, flags INTEGER, name BLOB, bytes INTEGER, logical INTEGER, items INTEGER, lb INTEGER, ll INTEGER, li INTEGER, newest INTEGER, mtime INTEGER, PRIMARY KEY(scan,id)) WITHOUT ROWID')
t = time.perf_counter()
with c:
    c.executemany('INSERT INTO nodes VALUES (1,?,?,?,?,?,?,?,?,?,?,?,?,?)', ((i, r[0], r[1], r[2], names[i], *r[4:]) for i, r in enumerate(recs)))
res['rows_write_s'] = time.perf_counter() - t
c.execute('PRAGMA wal_checkpoint(TRUNCATE)'); c.close()
res['rows_file_mb'] = os.path.getsize(pa) / 1e6
c = sqlite3.connect(pa)
t = time.perf_counter(); n = 0
for row in c.execute('SELECT * FROM nodes WHERE scan=1 ORDER BY id'): n += 1
res['rows_read_s'] = time.perf_counter() - t; assert n == N; c.close()
# B: packed chunks (records then names), 4 MiB per chunk, optional zlib (stand-in for a real compressor)
def pack():
    buf = bytearray()
    for r in recs: buf += REC.pack(*r)
    for nm in names: buf += nm
    return bytes(buf)
for comp in (False, True):
    key = 'chunks_z' if comp else 'chunks'
    pb = os.path.join(out, key + '.db'); c = fresh(pb)
    c.execute('CREATE TABLE chunks (scan INTEGER, seq INTEGER, data BLOB NOT NULL, PRIMARY KEY(scan,seq)) WITHOUT ROWID')
    t = time.perf_counter(); blob = pack(); res[key + '_pack_s'] = time.perf_counter() - t
    t = time.perf_counter()
    CH = 4 << 20
    with c:
        for k in range(0, len(blob), CH):
            d = blob[k:k + CH]
            c.execute('INSERT INTO chunks VALUES (1,?,?)', (k // CH, zlib.compress(d, 1) if comp else d))
    res[key + '_write_s'] = time.perf_counter() - t
    c.execute('PRAGMA wal_checkpoint(TRUNCATE)'); c.close()
    res[key + '_file_mb'] = os.path.getsize(pb) / 1e6
    c = sqlite3.connect(pb)
    t = time.perf_counter()
    parts = [zlib.decompress(r[0]) if comp else r[0] for r in c.execute('SELECT data FROM chunks WHERE scan=1 ORDER BY seq')]
    whole = b''.join(parts)
    res[key + '_read_s'] = time.perf_counter() - t
    t = time.perf_counter(); m = sum(1 for _ in REC.iter_unpack(whole[:N * REC.size])); res[key + '_python_unpack_s'] = time.perf_counter() - t
    assert m == N and len(whole) == len(blob); c.close()
res['raw_mb'] = len(blob) / 1e6
res['sqlite'] = sqlite3.sqlite_version; res['nodes'] = N
for k, v in res.items(): print(f'{k}: {v:.3f}' if isinstance(v, float) else f'{k}: {v}')
