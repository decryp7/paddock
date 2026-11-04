# 12. Fast ingest: the manifest, the per-content fact cache, the SQLite catalog and the derived mmap graph

Status: accepted (user, 2026-11-04). Proposed the same day in
[research/2026-11-04-fast-ingest.md](../research/2026-11-04-fast-ingest.md), whose recommendations D1 to D9 the user accepted, with
one change to D1: change detection uses Pacenotes' own manifest everywhere, with no Git fast path. Amends ADRs 0004, 0005, 0006 and
0007 (each carries an amendment section that points here).

## Context

Stage 1 (ADRs 0005 and 0006) indexes correctly but is slow per file and holds a whole run in memory. Its first run took about 3 ms a
file, while tree-sitter with the tags query needs about 0.8 ms (research note section 1). A run keeps every file's facts in one
`Batch` until a single commit, which grows with the repository. Facts belong to a `FileId`, so a new worktree, a branch switch or a
revert parses everything again. "Find references" and "callers" scan an unindexed `calls` table. The rules engine parses each file
once per rule. The research note surveys how code indexers stay fast (stack graphs, Blackbird, zoekt, SCIP, Glean, clangd, IntelliJ
stubs), measures parse, hash, graph, metadata-store and change-detection costs (recorded in
[performance.md, 2026-11-04](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks)), and compares
the stores. This ADR fixes the design that follows from it.

## Decision

### D1. Change detection: Pacenotes' own manifest, one code path for every folder

- **One mechanism, Git or not.** Every opened folder has a **manifest** in Pacenotes' data folder, never in the repository: per path,
  the size, the mtime and ctime in nanoseconds, the file id (the inode and device on macOS and Linux, the volume serial and file index
  from `GetFileInformationByHandle` on Windows) and the content key. A run walks the folder and compares each file's stat with its row.
  If all four values are equal the file is unchanged and is not read. If any differs, the file is read and hashed. Only a new content
  key goes on to the fact cache.
- **The racy rule of ADR 0006 stays.** A file whose mtime is not at least 2 s older than when it was read is checked by content, so an
  edit within the timestamp granularity is never missed. Stat values are compared for equality, never as "newer than", so a skewed
  clock (a network drive, a VM) causes a re-hash, never a missed change. A Setting "Always check content" covers file systems with
  coarse or unreliable times (FAT, some network shares).
- **The content key** is the `Fingerprint` of ADR 0002: SHA-256 of the content with line endings normalised. Benchmark B2 can replace
  the hash only if it shows the read pass is bound by hashing (measured 526 MiB/s for SHA-256, 1,675 for BLAKE3 on one core).
- **Ignore files** (`.gitignore`, `.ignore`, nested files, negations) are respected through the `ignore` crate, which the shared
  `paddock-repo-files` walk already uses (ADR 0006). The denylist is checked first and still wins.
- **Git is not used for change detection.** Pacenotes does not read the git index, does not run `git status` or `git diff`, never
  runs `git init` in a folder and never keeps a shadow repository. Reasons:
  - one code path for all folders;
  - read-only `git status` on a stale index re-hashes everything on every call (5.9 s for 27k files, measured);
  - the git binary runs configured programs (ADR 0004);
  - a shadow repository is a second compressed copy of the code (371 to 398 MB for this repository, 34 s to create, measured);
  - `git init` writes into the user's files and clashes with other version-control tools.

  `pacenotes-git` stays the history adapter only (ADR 0004).
- **The interface.** A `ChangeSource` port yields a `ChangeSet`: the entries sorted by path, each with its stat, its content key
  (`Known(Fingerprint)` or `Unknown`, meaning read and hash) and its change (unchanged, added, modified, deleted, or renamed from a
  path). Renames are found by content key (ADR 0005). The indexer sees nothing else.

### D2. The pipeline

```
open      -> map the last snapshot (D5); the app answers at once, the refresh runs as a job (ER-17 to ER-19)
change    -> one walk (ignore files, denylist, bounds) + lstat against the manifest                   I/O: 4 on SSD, 1-2 on HDD
read+hash -> only files whose stat changed; read into a reused buffer                                  I/O pool
cache     -> FactKey = H(content key, extractor schema version, grammar crate version and ABI, query pack hash, limits)
             hit: reuse the stored fact blob; miss: parse
parse     -> worker processes (ADR 0006), requests batched by up to 64 files or 4 MiB; one fact blob per file;
             the review rules run on the same tree (D9)
write     -> one writer, streaming: fact blobs in transactions of about 1,000 as they arrive;
             the new generation's path -> fact key map is swapped in one commit
link      -> build the snapshot (D5) from the generation's blobs; write it aside, fsync, rename
serve     -> queries read the snapshot, plus one blob for detail; SQLite holds user state, findings and links
```

The channels between stages are **bounded by bytes in flight** (default 64 MiB, part of the memory budget, D7), not by a file count.
A full channel blocks the stage before it (backpressure). Nothing collects the whole run: stage 1's `Batch` goes away.

### D3. The per-content fact cache

- **A fact blob** holds one file's parse result, keyed by `FactKey`, and depends only on the file's bytes and the extractor's
  versions. It contains:
  - a header (blob schema version, language, `partial`, `has_errors`);
  - a file-local string table;
  - symbols (kind, name, container, span as four `u32`);
  - occurrences (name, role bits for definition, reference, import, read, write and test, span), as SCIP models them;
  - imports;
  - call candidates (caller, callee name, span);
  - chunks (span, text hash, for the embedding cache of ADR 0001);
  - string literals and log-call sites (Debrief bridge).

  It is encoded with postcard, the family's IPC encoding, with optional zstd. `FileId` and `SymbolId` (ADR 0002) are not stored in
  the blob: they are derived when the blob is bound to a path.
- **One cache per user, not per repository.** Content keys are content-addressed, so one cache in the app's data folder serves every
  worktree, clone, branch, revert and vendored copy on the machine (D4 of the research note). A blob that no live generation of any
  repository names is garbage, and a sweep job removes it. The cache is a "cache" category in `paddock-storage`: clearable and
  rebuilt on demand.
- **A grammar or extractor upgrade** changes the `FactKey` of exactly the files it affects (IntelliJ's stub-version rule); their blobs
  are parsed again and the old ones are swept.
- **Where blobs live is decided by benchmark B9.** The default is SQLite in the catalog file: `facts(rowid INTEGER PRIMARY KEY,
  blob)` appended in insertion order, plus a narrow index `fact_keys(fact_key BLOB PRIMARY KEY, rowid) WITHOUT ROWID`. The blobs are
  not keyed by the hash itself, because random-order keys load 15 to 40x slower in every B-tree store (measured). The contender is
  append-only segment files (Arrow IPC) per ingest batch, with the same index in SQLite. B9's rule: take SQLite unless segments are at
  least 1.5x faster at ingest and not worse (each within 10%) on one-file update cost, crash recovery and peak footprint. If the
  narrow index is still the bottleneck at Linux scale, it moves to fjall, owned by the indexer process alone.

### D4. The SQLite catalog (schema v2)

SQLite in WAL mode (rusqlite, bundled; the family's store) is the only source of truth. It holds:

- `meta` (schema version, generation);
- the manifest (D1);
- `generation_files(generation, path, file_id, fact_key)`, the map that a commit swaps;
- `fact_keys` and `facts` (D3);
- quarantine;
- findings, suppressions, requirements links and coverage runs (proposal 14);
- the blob-to-snapshot bookkeeping.

The MCP server process (`pacenotes serve`) reads the same file while the app writes, which only SQLite of the candidates offers
together with crash safety and small memory (11 MiB peak RSS in the 1M-row benchmark). The per-row tables of schema v1 (`symbols`,
`imports`, `calls`, `chunks`) go away: their content lives in the blobs and the snapshot. Schema v1 files are rebuilt under ADR 0005's
rule (no migration before v1).

### D5. The derived mmap graph: the snapshot

Each generation gets one read-only file, `<data>/<repository>/snapshot-<generation>.pns`. It contains:

- a header: magic, format version, generation, the digest of its key set, and a section table with checksums;
- a string arena with offsets;
- a name FST (the `fst` crate, MIT or Unlicense) for exact, prefix and fuzzy name search;
- files, each with its symbol range;
- symbols as a structure of arrays;
- occurrences sorted by symbol;
- call edges as CSR, forward and reverse, with tier and confidence per edge;
- the import graph as CSR.

The arrays are little-endian and fixed-width. The work package picks `bytemuck` casts or Arrow IPC buffers. The snapshot is built
from the generation's blobs by interning strings, assigning `u32` ids, resolving tier-2 edges by name with a confidence label,
overlaying tier-3 edges (SCIP, LSP) and sorting. Edges are resolved here, never stored in a blob, because name resolution is global.
The sort spills to disk above the memory budget.

- **Why.** Measured on 5M edges: callers three levels up take 7.9 µs against 87 µs in SQLite, direct callers take under 1 µs against
  4.1 µs, and the file is 22.9 MiB against 222 MiB; building it takes 83 ms. A re-open maps the file and is usable at once, and only
  the touched pages are resident.
- **Depth is unlimited by design.** Callers, callees and paths are a BFS with a visited set (a stamp array, 4 bytes a node): each node
  is touched at most once, cycles end the walk, and the cost is bounded by the reachable part of the graph. Measured on the synthetic
  graph: all callers at unlimited depth take 24.7 ms (about 596,000 nodes reached on average) against 3,638 ms in SQLite, and a path A
  to B by bidirectional BFS takes 26.6 µs (about 1,300 nodes visited) against 2,742 ms. **UI:** levels are shown progressively ("Show
  next level", with counts of what is hidden), and "All callers" or "All callees" is an explicit action that runs as a cancellable
  query with a live count, because its answer can be most of the repository. MCP tools take a depth (default a few levels) and a result
  limit, and report truncation.
- **Derived data**, like the vector index (proposal 21.3). A missing, stale, damaged or other-version snapshot is rebuilt, never shown
  as an error. A property test checks that rebuilding from the blobs gives a byte-identical snapshot.
- **Updates.** The snapshot is rebuilt per run while B4 shows that a full link fits a watcher run. Otherwise a delta overlay (files
  hidden from the base, plus their new facts, as Glean stacks databases) is merged at query time and compacted in the background.
- **Safety.** The file is never modified in place: it is written aside, fsynced and renamed. Old snapshots are deleted when no reader
  holds them, lazily on Windows, which cannot delete a mapped file.
- **Queries** (find references, callers N, symbols in a file, path between two symbols, name search) are pure functions over the
  snapshot's arrays behind the `Store` read port (ADR 0002), with a cancel token and a result limit.

### D6. Text search without an index first

Literal and regex search runs over the walked files with the ripgrep crates (`grep-searcher`, `grep-regex`; MIT or Unlicense), with
the denylist, limits and cancellation. No text index is built at ingest. A document trigram index (the codesearch design, about 20% of
the source, mmap'd) is added only if benchmark B6 shows the need. BM25 over chunks for hybrid search stays as ADR 0001 and proposal
21.3 have it.

### D7. Memory: budgets and Auto settings

- **Mechanisms.** Bounded memory comes from:
  - one tree per worker, dropped after extraction;
  - channels bounded by bytes;
  - blobs written as they arrive;
  - interned strings and `u32` ids;
  - the snapshot mmap'd, not on the heap;
  - the link sort spilling to disk;
  - no pointer graph in memory.
- **Targets (estimates, to be confirmed by B7).** Under 500 MB peak private footprint for a first index of 100k files, and under
  100 MB steady while idle or querying, on the 16 GB "Low memory" preset (proposal 21.4).
- **Settings, each with an "Auto" default (ER-20):**
  - **Index threads** (parse workers): `min(cores − 1, memory budget / per-worker peak, 8)`. The per-worker peak is measured by the
    worker's self-test at start and updated from observed peaks; ADR 0006's 1 GiB cap per worker stays.
  - **Index I/O threads**: 4 on an SSD, 1 to 2 on a hard disk.
  - **Index memory budget**: 500 MB on machines with 16 GB or less, 1 GB above (estimate). It splits into the worker share, the bytes in
    flight and the link sort buffer.
- **The trade-offs are explicit in the UI copy.** Fewer workers or a smaller budget use less memory and index more slowly; the
  measured scaling was 1.7x on 2 threads and 3.5x on 4.
- **Engines are capped.** SQLite keeps its default page cache (about 2 MB per connection). DuckDB, where used (D8), runs with an
  explicit `memory_limit` and `threads`; its defaults are 80% of RAM and every core.

### D8. DuckDB and DuckLake are for export only

The interactive path (UI, MCP, review) never runs on DuckDB or DuckLake. Their point queries take milliseconds and their small writes
make small files. An optional, later **Parquet export** of the snapshot and the catalog serves analytics (hot spots over history,
coverage joins). It writes into a DuckLake catalog if Debrief's queued DuckLake trial adopts one, so the two apps share conventions
and tooling. A shared crate waits for the rule of three (ER-22).

### D9. Review rules run in the parse worker

The worker runs the structural rules of ADR 0007 on the tree it has already parsed. Each rule costs a match, not a parse; the parse
was most of a rule's 25 ms (measured). The results are cached under the `FactKey` plus the rule pack's hash. A crash or a stall in the
rule engine is attributed to the file and handled like a grammar crash (ADR 0006: the file is quarantined until its content or the
rule pack changes). The rule engine's step budgets (ADR 0007) bound a rule; the worker's memory cap bounds the process.

### D10. Submodules and nested repositories are indexed on request

A folder below the root that contains a `.git` entry (a folder or a gitfile) is a **nested repository boundary**. It is listed with
its file count and an "Index submodules" action (a per-repository Setting, off by default), and it is not walked into until the user
turns it on. Once indexed, it has its own manifest and generation, and its facts join the parent's snapshot. The detection uses the
walk alone: Git is not read (D1).

## Parallelism and cost (rule 51)

| Stage | Kind of work | Bound (Auto default; each a Setting) | Cheaper operation |
|---|---|---|---|
| change | metadata I/O | 4 threads on SSD, 1 to 2 on HDD | one walk instead of three (after `paddock-walk` gains cancel and progress, ADR 0006's TODO); the stat comparison; paths from the watcher when `paddock-watch` carries them |
| read and hash | I/O, then CPU | 4 on SSD | only files whose stat changed; SHA-256 unless B2 shows the pass is hash-bound |
| parse and rules | CPU, one process per worker | `min(cores − 1, memory budget / per-worker peak, 8)` | the fact cache; batched requests; rules on the same tree; incremental re-parse of files open in an editor (39x measured), later |
| write | one writer | 1 (SQLite has one writer) | one blob row per file, statements prepared once, transactions of about 1,000 |
| link | CPU and memory | parallel sort (cores − 1), single-threaded CSR build | 83 ms per 5M edges measured; overlay only if B4 asks; spill above the budget |
| embed | CPU | cores − 1 | cache by chunk text hash and model id (ADR 0001) |
| serve | memory reads | per query | mmap; no work at open |

Bulk index work runs at `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL` (rule 52), never at a level that can be
starved, because the user waits for the first index. Results are deterministic: blobs are content-addressed, the writer applies in
path order and the link step sorts.

## Work packages

Sizes are agent-days, estimated from this design (the family's rule: a firm estimate once an ADR has work packages). The benchmark
packages come first, because B9, B4 and B7 decide open parameters. Every benchmark result is recorded in `docs/performance.md`
before anything quotes it (rule 48), with footprint and RSS for memory.

| WP | What | Depends on | Size |
|---|---|---|---|
| IG0 | Corpus generator: a tree of N files of invented Rust, TypeScript and C with realistic size and call distributions (medium 10k, large 100k), plus the scripted clone of a public Linux checkout into the scratch area; a memory sampler (footprint and RSS every 100 ms) | | 2 |
| IG-B1 | B1: per-phase cost of stage 1 with 1 and 8 workers (read, IPC, parse, extract, apply, SQLite), then batched IPC and prepared statements; adds a C corpus. B2: SHA-256, BLAKE3 and xxh3-128 over the read pass | IG0 | 2 |
| IG-B3 | B3 and B4: snapshot against normalised SQLite for the four queries at 1M, 5M and 20M synthetic edges and on the tier-2 graph of the Linux checkout (p50, p99), including callers and callees at unlimited depth (time and nodes visited) and path A to B by bidirectional BFS; link cost at 100k, 1M and 10M symbols, with and without spill | IG0 | 2.5 |
| IG-B6 | B6: brute-force search against a document trigram index against FTS5 trigram (warm and cold p50/p99, size, build time) | IG0 | 1.5 |
| IG-B7 | B7: memory at three sizes during first ingest, idle and querying; workers 2, 4 and 8; in-flight budgets 32, 64 and 128 MiB | IG2, IG3 | 2 |
| IG-B9 | B9: SQLite append-ordered blobs against Arrow IPC segments (ingest, key-index insert, footprint, disk, re-open, one-file update, crash test, branch switch, sweep); then the decision recorded as an amendment here | IG2 | 3 |
| IG1 | Manifest and `ChangeSource` (D1): stat with ctime and file id on all three platforms, racy rule, "Always check content" Setting, renames by content key; replaces the stamp shortcut of ADR 0006 | | 3 |
| IG2 | Fact blob format and schema v2 (D3, D4): blob encoding and version, `FactKey`, worker emits blobs, streaming writer with byte-bounded channels, generation swap, sweep job, `paddock-storage` category | IG-B1 | 6 |
| IG3 | Batched worker protocol (D2): up to 64 files or 4 MiB a request, replay one by one after a crash to find and quarantine the culprit; per-worker peak reported to the host | | 3 |
| IG4 | Snapshot builder and reader (D5): arena, FST, structure-of-arrays, CSR both ways, spill sort, header and checksums, write-aside and rename, lazy deletion on Windows; the rebuild-is-identical property test | IG2, IG-B3 | 7 |
| IG5 | Queries over the snapshot behind the `Store` port: references, callers and callees to any depth (progressive levels, "all" as an explicit cancellable query with a live count), symbols in a file, shortest path by bidirectional BFS, name search (exact, prefix, fuzzy), each with cancel and limits | IG4 | 4 |
| IG6 | Text search without an index (D6): grep-searcher over the walked set, denylist, limits, cancel, MCP tool | IG1 | 2 |
| IG7 | Rules in the worker (D9): `find_all` on the worker's tree, results cached by `FactKey` and pack hash, crash attribution | IG2, IG3 | 3 |
| IG8 | Memory budget and Auto settings (D7): Index threads, I/O threads and memory budget; the budget split; UI copy for the trade-off | IG3, IG-B7 | 2 |
| IG9 | Nested repository boundaries (D10): detection in the walk, the listing, the per-repository Setting, a separate manifest and generation, the join into the parent's snapshot | IG1, IG4 | 2 |
| IG10 | Parquet export (D8), after Debrief's DuckLake trial decides | IG4 | 2.5, later |

Total: about 32 agent-days for IG1 to IG9 and 13 for the benchmarks, so about 45, or 47.5 with IG10, an
estimate with the usual uncertainty of a design-stage number.

## Consequences

- Ingest work becomes proportional to new content, not to the repository or the number of worktrees. A new worktree of an indexed
  repository still reads every file once (its mtimes are new), but parses only content the cache has not seen. That is the price of one
  code path without Git (D1).
- Two derived artefacts (the cache and the snapshot) must stay correct. Both are derived from content alone, and the rebuild property
  test guards the snapshot.
- Schema v1 files are rebuilt once.
- The UI and MCP query latencies move from milliseconds to microseconds for graph walks. The cost is a private file format with a
  version in its header.
- DuckDB stays out of Pacenotes' interactive path; analytics come later through an export.
- The Settings page gains three index Settings with Auto defaults, and the repository settings gain "Index submodules" and "Always check
  content".
