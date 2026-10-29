"""usage: python3 -I sampcat.py FILE.sample.txt -- self samples of a macOS sample(1) call graph by category (innermost matching
frame wins), per thread group, idle (waits) separated. Rust v0 symbols are matched on the names embedded in them."""
import re, sys, collections
IDLE = ("psynch_cvwait", "semaphore_wait", "semaphore_timedwait", "__workq_kernreturn", "kevent", "mach_msg", "__ulock_wait",
        "nanosleep", "__semwait_signal", "__psynch_mutexwait", "swtch_pri", "thread_switch", "__select")
RULES = [  # (category, substrings in a frame); checked from the innermost frame outwards
    ("zstd", ("ZSTD_", "zstd")), ("lz4/snappy", ("LZ4", "lz4", "snappy")),
    ("gzip inflate", ("inflate", "flate2", "miniz", "zlib_rs", "libdeflate", "Inflate")),
    ("file read (syscall)", ("(in libsystem_kernel.dylib)",)),  # refined below by name
    ("tantivy", ("tantivy",)), ("duckdb", ("libduckdb",)),
    ("parquet", ("7parquet", "parquet")),
    ("regex", ("regex_automata", "5regex", "aho_corasick", "memchr")),
    ("json parse", ("serde_json", "simd_json", "sonic")),
    ("entities extract", ("entities",)), ("catalogue", ("catalogue",)),
    ("arrow batch build", ("debrief_batch", "arrow_array", "arrow_buffer", "arrow_data", "EventBatchBuilder")),
    ("stats/promote", ("5stats", "7promote", "stats", "promote")),
    ("words (tokenizer)", ("5words",)),
    ("chunk/line reading", ("5chunk", "LineReader", "ReaderChunks", "8encoding")),
    ("batcher/pipeline", ("7batcher", "8pipeline", "RecordBatcher")),
    ("text/log parsing", ("importers", "6parser", "4text", "4json", "ingest", "5lines", "debrief_core")),
    ("alloc", ("mi_", "mimalloc", "_malloc", "free", "_realloc")),
    ("memcpy/memmove", ("_platform_mem",)),
]
def cat_of(path):
    leaf = path[-1]
    if any(k in leaf for k in IDLE): return "idle (wait)"
    for f in reversed(path):
        if "(in libsystem_kernel.dylib)" in f:
            name = f.split()[0]
            if name in ("read", "pread", "__pread_nocancel", "__read_nocancel"): return "file read (syscall)"
            if name in ("write", "pwrite", "__pwrite_nocancel", "__write_nocancel", "fsync", "fcntl", "rename", "open", "close", "__open", "unlink", "__unlink", "__open_nocancel", "close_nocancel", "__close_nocancel"): return "file write/open/rename (syscall)"
            if name.startswith("__mmap") or name.startswith("madvise") or name.startswith("__munmap") or name.startswith("mmap") or name.startswith("munmap"): return "vm (mmap/madvise)"
            continue
        for c, subs in RULES:
            if c.startswith("file read"): continue
            if any(s in f for s in subs): return c
    return "other"
def group(t):
    t = re.sub(r"Thread_\d+\s*", "", t).strip(": ")
    t = re.sub(r"DispatchQueue_\d+: ", "", t)
    return re.sub(r"-\d+$", "-N", re.sub(r"\d+$", "N", t)) or "(unnamed)"
lines = open(sys.argv[1], errors="replace").read().split("\n")
start = next(i for i, l in enumerate(lines) if l.startswith("Call graph:")) + 1
stack = []; thread = None; tot = collections.defaultdict(collections.Counter)
node_re = re.compile(r"^(\s*[+!:| ]*)(\d+) (.*)$")
nodes = []  # (thread, depth, count, sym)
for l in lines[start:]:
    if not l.strip(): break
    m = node_re.match(l)
    if not m: continue
    depth = len(m.group(1)); cnt = int(m.group(2)); sym = m.group(3)
    if "Thread_" in sym and depth == 4: thread = group(sym); stack = []; continue
    nodes.append((thread, depth, cnt, sym))
# self = count - sum(children counts)
for i, (th, d, c, s) in enumerate(nodes):
    while stack and stack[-1][0] >= d: stack.pop()
    stack.append((d, s))
    child = 0
    for th2, d2, c2, s2 in nodes[i + 1:]:
        if th2 != th or d2 <= d: break
        if child_depth := (d2 if child == 0 and 'cd' not in dir() else None): pass
    # children are the next nodes with the smallest depth > d until depth <= d
    j = i + 1; cd = None
    while j < len(nodes) and nodes[j][0] == th and nodes[j][1] > d:
        if cd is None: cd = nodes[j][1]
        if nodes[j][1] == cd: child += nodes[j][2]
        j += 1
    selfc = c - child
    if selfc > 0: tot[th][cat_of([x[1] for x in stack])] += selfc
want = sys.argv[2:] or None
allc = collections.Counter()
for th in sorted(tot, key=lambda t: -sum(tot[t].values())):
    busy = sum(v for k, v in tot[th].items() if k != "idle (wait)")
    if busy < 20: continue
    allc.update({k: v for k, v in tot[th].items() if k != "idle (wait)"})
    print(f"{th}: busy {busy}, idle {tot[th]['idle (wait)']}")
    for k, v in tot[th].most_common():
        if k != "idle (wait)" and v * 100 >= busy: print(f"    {k:34s} {v:7d} {100*v/busy:5.1f}%")
b = sum(allc.values()); print(f"ALL busy samples {b}")
for k, v in allc.most_common(): print(f"    {k:34s} {v:7d} {100*v/b:5.1f}%")
