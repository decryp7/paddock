# Performance record

Pacenotes' benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
Pacenotes has no code in the tree yet. Its measurements come from the embedding research behind
[ADR 0001](adr/0001-embeddings.md).

## Benchmarks

| Benchmark | Measures | Corpus | Budget | CI |
|---|---|---|---|---|
| Embedding comparison ([ADR 0001](adr/0001-embeddings.md)) | Model load, chunks/s on 1 and 13 threads, peak RSS, binary size, retrieval quality (recall@5, hit@5) | 19,462 function chunks from 25 Paddock crates (10.9 MB, median 320 characters); 332 labelled queries (300 doc, 32 concept) | Quality bar: concept hit@5 ≥ 0.75 and doc recall@5 +25% over BM25 | No |
| Fast-ingest micro-benchmarks ([research note](research/2026-11-04-fast-ingest.md), [ADR 0012](adr/0012-fast-ingest.md)) | tree-sitter parse and tags throughput on 1, 2 and 4 threads; incremental re-parse; content hashes; graph queries (direct, three levels, unlimited depth, path A to B) on CSR (warm, cold mapped) and SQLite; metadata stores (manifest and fact-key loads, reads during writes, lookups, small commits, physical footprint); git change detection | a Paddock worktree's Rust and TypeScript (6,419 files, 50.9 MiB); an invented 1M-node, 5M-edge call graph; 1M invented manifest rows and hash keys | none yet (ADR 0012's IG-B packages set them) | No |

## Budgets

The [proposal](proposal.md) §21.4 sets memory targets to be measured: about 2 GB in total on a 16 GB machine, and 6–8 GB
on a 64 GB machine.

## Gaps

- The measurement plan on real repositories (re-rank latency, indexing time, peak memory and index size per 1M lines) is
  pending.
- The harness is not in the tree, so the numbers cannot be reproduced from `main`.

## Results (newest first)

### 2026-11-04, fast-ingest rerun after review: one query set, physical footprint

Quoted by [ADR 0012](adr/0012-fast-ingest.md) and the [research note](research/2026-11-04-fast-ingest.md). It replaces the first
run's graph comparison and every memory figure of that run (which were RSS). Indications for design choices, not budgets.

- **Commit:** branch `docs/pacenotes-fast-ingest`; harness [research/2026-11-04-fast-ingest-bench.rs.txt](research/2026-11-04-fast-ingest-bench.rs.txt)
  (`graph2` and `kv` subcommands, reworked after the review)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, shared with other agents (a Dyno release was running)
- **Load (1-minute):** 11.6 to 19.8 during the graph run; 7.3, 4.9 and 8.5 at the ends of the three kv rounds. Everything at
  `nice -n 19` through the machine's semaphore
- **Build:** release, `lto = "thin"`, system allocator; rustc 1.99.1; rusqlite 0.40.2 bundled, redb 2.6.4, fjall 2.11.2, heed 0.22.1,
  blake3 1.8.7
- **Corpus:** invented. The graph has 1,000,000 nodes and 5,000,000 call edges from a seeded generator, with callees skewed so a few
  hubs have many callers. The stores get 1,000,000 rows of two kinds:
  - *manifest*: 40-byte path keys in walk order, 64-byte values;
  - *fact_keys*: uniform random 32-byte BLAKE3 keys, 8-byte values (a rowid), the shape of ADR 0012's `fact_keys` index.
- **Command:** `run-r2.sh` (in the raw output file): `graph2 1000000 5000000 <scratch>/g 10`, then three interleaved rounds over
  the stores of `KV_MODE=<manifest|factkeys> KV_READER=<1|0> kv <store> 1000000 <scratch>/kvr`
- **Runs:** graph once; stores three interleaved rounds, reported as median (range)
- **Memory measure:** physical footprint (`proc_pid_rusage`, `ri_phys_footprint` and `ri_lifetime_max_phys_footprint`, rule 26)
- **Raw output:** [research/2026-11-04-fast-ingest-raw-rerun.txt](research/2026-11-04-fast-ingest-raw-rerun.txt)
- **Graph queries.** One query set for both backends:
  - 200 random start nodes for "direct" and "3 levels";
  - 10 for unlimited depth;
  - 200 random pairs for paths.

  Every traversal is the same BFS with a visited stamp; only the neighbour lookup differs. SQLite has indexes on `(dst, src)` and
  `(src, dst)`, `cache_size` 512 MiB (larger than its 222 MiB file) and a warmed cache. "Cold mapped" is a file written with
  `F_NOCACHE`, mapped, and queried for the first time. Time per query (time per node reached):

  | Query | CSR in memory | CSR mapped, first pass | CSR mapped, second pass | SQLite, BFS over indexed look-ups | SQLite recursive CTE (`UNION` on the node) |
  |---|---|---|---|---|---|
  | callers, direct (avg 6) | 0.004 ms | 0.518 ms (84 µs a node: page faults) | 0.000 ms | 0.002 ms (0.40 µs) | |
  | callers, 3 levels (avg 201) | 0.005 ms (0.023 µs) | 0.760 ms (3.8 µs) | 0.004 ms (0.018 µs) | 0.075 ms (0.373 µs) | |
  | callers, unlimited (avg 496,497) | 20.4 ms (0.041 µs) | 23.2 ms (0.047 µs) | 21.0 ms (0.042 µs) | 882 ms (1.78 µs) | 1,323 ms (2.66 µs) |
  | callees, direct (avg 4) | 0.000 ms | 0.704 ms | 0.000 ms | 0.002 ms (0.52 µs) | |
  | callees, 3 levels (avg 146) | 0.004 ms (0.029 µs) | 0.511 ms (3.5 µs) | 0.004 ms (0.027 µs) | 0.053 ms (0.360 µs) | |
  | callees, unlimited (avg 588,448) | 28.3 ms (0.048 µs) | 29.9 ms (0.051 µs) | 35.0 ms (0.060 µs) | 1,043 ms (1.77 µs) | 1,494 ms (2.54 µs) |
  | path A to B, bidirectional (125 of 200 connected, 1,282 visited avg) | 0.032 ms (0.025 µs) | 0.030 ms | 0.030 ms | 0.474 ms (0.370 µs) | |

  - **Builds and sizes:** both CSR directions built from the in-memory edge list in 161 ms, 45.8 MiB. SQLite: insert 1.79 s,
    indexes 6.26 s, file 222 MiB.
  - **Footprint:** 86 MiB after building the CSR (edge list and both directions in memory), and 102 to 103 MiB before and after
    the mapped-file queries, so mapping added about 1 MiB (clean pages). The process peak was 696 MiB, mostly SQLite's 512 MiB cache.
- **Metadata stores** (median of three, range in brackets; durability as each store runs by default in this harness: redb fsync per
  commit, fjall `persist(SyncAll)` per commit, LMDB sync per commit, SQLite WAL `NORMAL` **without** an fsync per commit, only at
  checkpoints, and SQLite WAL `FULL` with one):

  | Store | Rows | Load, k rows/s | Reads during load, k/s (worst) | Look-up | 100-row commit | Disk | Footprint peak |
  |---|---|---|---|---|---|---|---|
  | SQLite `NORMAL` | manifest | 307 (282–322) | 297 (22 ms) | 2.56 µs | 20.5 ms | 125 MiB | **9 MiB** |
  | SQLite `NORMAL` | fact_keys | 25 (22–26) | 93 (143 ms; 617 ms once) | 2.17 µs | 17.1 ms | 81–193 MiB (WAL) | 8 MiB |
  | SQLite `NORMAL` | fact_keys, no reader | 21 (21–27) | | 2.13 µs | 17.0 ms | 81 MiB | 5 MiB |
  | SQLite `FULL` | fact_keys | 21 (18–22) | 176 (71 ms; 1,344 ms once) | 2.12 µs | 18.1 ms | 81 MiB | 8 MiB |
  | redb | manifest | 157 (157–168) | 972 (5.7 ms) | 1.10 µs | 68.6 ms | 258 MiB | 186 MiB |
  | redb | fact_keys | 20 (15–23) | 956 (5.3 ms) | 1.14 µs | 40.1 ms | 129 MiB | 191 MiB |
  | redb | fact_keys, no reader | 18 (17–20) | | 1.10 µs | 53.3 ms | 129 MiB | 185 MiB |
  | LMDB | manifest | 382 (248–442) | 1,102 (0.1 ms) | 1.05 µs | 48.4 ms | 108 MiB | **5 MiB** |
  | LMDB | fact_keys | 24 (24–27) | 1,108 (3.3 ms) | 1.04 µs | 48.7 ms | 185 MiB | 63 MiB |
  | LMDB | fact_keys, no reader | 29 (24–31) | | 1.00 µs | 49.0 ms | 185 MiB | 63 MiB |
  | fjall | manifest | 170 (167–201) | 580 (73 ms) | 4.83 µs | 21.1 ms | 45 MiB | 90 MiB |
  | fjall | fact_keys | **314** (223–328) | 759 (56 ms) | 2.38 µs | 13.7 ms | 62 MiB | 122 MiB |
  | fjall | fact_keys, no reader | 246 (220–253) | | 2.46 µs | 15.9 ms | 62 MiB | 103 MiB |

  The no-reader control changes little, so the slow random-key loads are the B-tree stores' own cost, not reader contention.
- **Notes:** one machine, under load; the stores were run at their versions of the first run (redb 4.4, fjall 3.1 and heed 0.23 are
  newer). The graph is random apart from the skew, so real call graphs are likely sparser; IG-B3 measures the Linux tier-2 graph.

### 2026-11-04, fast-ingest research: parse, hash and graph micro-benchmarks

Quoted by [research/2026-11-04-fast-ingest.md](research/2026-11-04-fast-ingest.md). Indications for a design choice, not budgets.

- **Commit:** branch `docs/pacenotes-fast-ingest` on c1bba3f9f; harness
  [research/2026-11-04-fast-ingest-bench.rs.txt](research/2026-11-04-fast-ingest-bench.rs.txt) (a scratch crate outside the
  repository; no workspace builds it)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2, shared with other agents (a Dyno release was running)
- **Load (1-minute):**
  - parse run: 7.6 at the start, 6.4 at the end;
  - graph run: 21.9 at the start, 13.5 at the end;
  - metadata-store runs: 13.6 down to 3.9 (random keys) and 7.4 to 13.3 (sorted keys);
  - git runs: about 6 to 14.

  All runs were at `nice -n 19`. Everything except the read-only git commands went through the machine's semaphore.
- **Build:** release, `lto = "thin"`, system allocator; rustc 1.99.1; tree-sitter 0.27.1, tree-sitter-rust 0.24.2,
  tree-sitter-typescript 0.23.2 (the versions `pacenotes-parse` pins), sha2 0.10.9 (default features), blake3 1.8.7, twox-hash
  2.1.5, rayon 1.12.0, rusqlite 0.40.2 bundled
- **Corpus:** parse: every `*.rs` and `*.ts` (not `.d.ts`) of a Paddock worktree, skipping `node_modules`, `target`, `dist`,
  `.git`, `.angular` and `fixtures`: 6,419 files (2,989 Rust, 3,430 TypeScript), 50.9 MiB, 1,290,893 lines; read into memory
  first, so no I/O is timed. Graph: invented, 1,000,000 nodes and 5,000,000 call edges from a seeded generator; callees skewed
  (cube of a uniform draw) so a few hubs have many callers
- **Command:** `ingest-bench parse <worktree> 2 4`; `[REACH=1] ingest-bench graph 1000000 5000000 <scratch>/graph.db`;
  `[KV_SORTED=1] [KV_SQLITE_CACHE_KIB=65536] ingest-bench kv <redb|fjall|lmdb|sqlite|sqlite-full> 1000000 <scratch>/kv`;
  git: `/usr/bin/time -p` around `GIT_OPTIONAL_LOCKS=0 git status --porcelain=v2 --untracked-files=all`, `git ls-files -s`,
  `git ls-tree -r HEAD`, `git diff --name-status [--no-renames] <a> <b>`, a private index (`GIT_INDEX_FILE=<scratch>/x.idx git read-tree
  HEAD`, then `git update-index -q --refresh`), and a shadow repository (`git -c core.fsmonitor=false --git-dir=<scratch>/shadow.git
  --work-tree=<worktree> add -A`, then `status`, `gc`)
- **Runs:** one run each; hashes averaged over 3 passes; graph latencies averaged over 1,000 queries (200 for the recursive CTE)
- **Numbers, parse and hash:**

  | Measure | Result |
  |---|---|
  | tree-sitter parse only, 1 thread | 3.44 s, 14.8 MiB/s, 375k lines/s (Rust 14.0, TypeScript 15.3 MiB/s) |
  | parse plus the grammar's `TAGS_QUERY`, 1 thread | 4.93 s, 10.3 MiB/s, 262k lines/s, 917,172 captures |
  | parse plus tags, 2 / 4 threads (one parser per thread, in process) | 17.8 / 36.5 MiB/s (1.7x / 3.5x) |
  | re-parse after inserting one line in the middle, 20 largest files (1,457 KiB) | full 87.7 ms, incremental with the edited old tree 2.2 ms (39x) |
  | SHA-256 / BLAKE3 / xxh3-64, 1 thread | 526 / 1,675 / 31,986 MiB/s (SHA-256: sha2 0.10.9 software rounds; its aarch64 hardware path needs the `asm` feature) |
  | peak RSS of the parse run | 96 MiB |

- **Numbers, graph** (1,000 queries on CSR, 200 for the recursive CTE; the samples differ, so the "reached" averages are not
  comparable, the per-query times are):

  | Measure | CSR (in memory, mmap-able layout) | SQLite (`edges(src, dst)`, indexes on `(dst, src)` and `(src, dst)`) |
  |---|---|---|
  | build | 83 ms (sort included) | insert 1.57 s (3.19M rows/s, prepared, one transaction) + indexes 4.71 s |
  | size | 22.9 MiB (reverse direction) | 222 MiB file |
  | direct callers | 0.006 µs (offset pair; count only), avg 8 | 4.1 µs, avg 8 |
  | callers 3 levels | 7.9 µs (BFS, visited stamp), avg 284 reached | 87 µs (recursive CTE), avg 123 reached |
  | peak RSS of the run | 74 MiB (both) | |

- **Numbers, unlimited depth and paths** (second run of the same graph with `REACH=1`, 1-minute load 22.9 at the start and 12.3 at
  the end; the first run's rows repeated within run-to-run spread: callers 3 levels 5.0 µs on CSR and 93 µs in SQLite, direct callers
  0.066 µs and 2.3 µs). BFS with a visited stamp, each node at most once, so cycles end the walk. Starting nodes are random; in this
  skewed graph a node either has no callers (reach 0) or reaches the giant component (about 993,000 nodes), so the averages are
  bimodal:

  | Query | CSR | SQLite (recursive CTE with `UNION`, which deduplicates nodes) |
  |---|---|---|
  | forward CSR build (callees) | 85 ms | |
  | all callers, unlimited depth | 24.7 ms a query, reached avg 595,796 (min 0, max 992,995), 100 queries | 3,638 ms a query, the same reach, 5 queries |
  | all callees, unlimited depth | 21.2 ms a query, reached avg 582,564 (max 588,451), 100 queries | not run |
  | path A to B | bidirectional BFS: 26.6 µs a query, 592 of 1,000 pairs connected, average length 8.9, visited avg 1,277 nodes (max 6,692) | SQL has no early exit or two-sided search: forward reach from A, then a test for B, 2,742 ms a query, 5 queries |
  | peak RSS of the run | 150 MiB (both directions of CSR, the SQLite run included) | |

  No graph from a real repository was measured: the harness has no extractor; IG-B3 adds one for the Linux checkout.

- **Numbers, metadata stores** (1,000,000 rows: 40-byte path key, 64-byte value; commits of 10,000 with fsync, a second thread
  doing point lookups during the load; then 200,000 point lookups; then 100 commits of 100 upserts). redb 2.6.4, fjall 2.11.2
  (`persist(SyncAll)` after each commit), heed 0.22.1 (LMDB, 8 GiB map), SQLite WAL through rusqlite 0.40.2 with one connection
  per thread:

  | Store | Load, path order | Load, random order | Reads during load (worst) sorted / random | Lookup | 100-row commit | Disk sorted / random | Peak RSS sorted / random |
  |---|---|---|---|---|---|---|---|
  | SQLite `NORMAL` | 359k/s, 2.79 s | 9k/s, 111 s; with `cache_size` 64 MB 12k/s, 85.7 s | 305k/s (21 ms) / 120k/s (51 ms); 64 MB cache 407k/s | 2.6 µs | 20 to 22 ms | 122 / 264 MiB (WAL included; 129 MiB after checkpoint) | 11 / 11 MiB; 175 MiB with the 64 MB cache |
  | SQLite `FULL` | not run | 9k/s, 116 s | 195k/s (45 ms) | 2.8 µs | 15 ms | 243 MiB | 12 MiB |
  | redb | 122k/s, 8.18 s | 7k/s, 141 s | 1.13M/s (6 ms) / 1.0M/s (9 ms) | 1.1 µs | 63 to 89 ms | 258 / 258 MiB | 312 / 381 MiB |
  | LMDB | 345k/s, 2.90 s | 15k/s, 65 s | 1.19M/s (0.7 ms) / 1.03M/s (3.6 ms) | 1.1 µs | 69 to 89 ms | 108 / 323 MiB | 114 / 428 MiB |
  | fjall | 206k/s, 4.85 s | 195k/s, 5.12 s | 461k/s (98 ms) / 379k/s (99 ms) | 5.0 to 6.0 µs | 18 to 40 ms | 45 / 53 MiB | 122 / 149 MiB |

  Compile time of each store crate in this build (cargo `--timings`, 2 jobs, nice 19): redb about 5 s, fjall with `lsm-tree` about
  5 s, heed with LMDB about 2 s; SQLite's bundled C was built earlier and not timed.

- **Numbers, git change detection** (git 2.56.0; this worktree: 11,867 tracked files; a public open-source checkout on the
  machine: 27,390 tracked files, 27 submodules, 3,704 commits, its index stat data stale for 27,363 entries):

  | Measure | 11,867 files | 27,390 files |
  |---|---|---|
  | `status --porcelain=v2 -uall`, `GIT_OPTIONAL_LOCKS=0` | 0.34 s first, 0.07 to 0.08 s after | 32.8 s first, 5.9 s every later call (stale stat data, index not rewritten) |
  | the same, `-uno`, submodules ignored | | 5.9 s |
  | private index: `read-tree HEAD` / first `update-index --refresh` / later refresh / `status -uno` / `status -uall` | | 0.03 s / 5.9 s / 0.03 s / 0.03 s / 0.07 s; index file 3.3 MB |
  | `ls-files -s` | 0.01 s | 0.01 s |
  | `ls-tree -r HEAD` | 0.17 s | 0.12 s |
  | `diff --name-status`, 200 first-parent commits apart (3,368 paths) | 0.82 s with renames, 0.01 s `--no-renames` | |
  | `diff --name-status`, 2,000 commits apart (11,086 paths) | 0.05 s (rename detection skipped by `renameLimit`), 0.02 s `--no-renames` | |
  | shadow repository: `add -A` / `status` / `status` after `touch` / `gc` | 34.3 s / 0.06 to 0.07 s / 0.07 s / 15.7 s; 11,158 loose objects, 398 MB, 371 MB after `gc` (486 MiB of tracked files, images included) | |

- **Memory measure:** RSS (`getrusage`), **not rule-26 evidence**. Superseded for memory by the rerun above, which measures physical
  footprint
- **Notes:** the parse numbers exclude the worker process, IPC, extraction into facts and SQLite, which the index pays (see
  2026-10-30 below); C and C++ grammars are not measured. The SQLite numbers use one prepared statement and one transaction, unlike
  stage 1's per-row `execute`. Raw output was kept in the session's scratch area only.

### 2026-10-25 (amended 2026-10-28), embedding engines

- **Machine:** Mac mini M4 Pro, 14 cores, shared with other agents
- **Load:** given per number (1.9–42); one run per configuration
- **Build:** release (`lto = "thin"`, `strip`)
- **Corpus:** as above (synthetic in the sense that it is Paddock's own code, no user data)
- **Numbers:**

  | Engine | Model file | Load time | Chunks/s, 1 thread | Chunks/s, 13 threads | Peak RSS |
  |---|---|---|---|---|---|
  | potion-base-8M | 29 MB | 15 ms | 9,581 | 65,843 | 131 MB |
  | potion-code-16M-v2 | 32 MB | 34 ms | 6,365 | 55,499 | 200 MB |
  | potion-retrieval-32M | 125 MB | 44 ms | 8,001 | 57,239 | 357 MB |
  | BGE-small fp32 | 133 MB | 0.5–0.6 s | 17.1 | 14.3–15.8 | 2.6–3.0 GB (batch 1) |
  | Qwen3-Embedding-0.6B Q8_0, Metal | 610 MB | about 2 s | — | 29.6 (GPU) | 1.9 GB |

- **Quality, doc recall@5:** BM25 0.250; potion-code 0.330; BM25 + potion-code 0.320; BGE 0.447; Qwen 0.773.
- **Verdict:** BM25 + potion-code reaches 0.750 concept hit@5 and +28% doc recall, so it meets the bar.
- **Notes:** about 8 s per 1M lines with Model2Vec, against about 50 min with BGE. Source: [ADR 0001](adr/0001-embeddings.md).

## 2026-10-30, `pacenotes-git` open: the symbolic-link walk of the git folder

Rule 48/51; helper: `PN_WALK_REPO=<repo> cargo test -p pacenotes-git --test it -- --ignored --nocapture measure_open`.
Machine: the development Mac mini under normal load; one run each, second run shown where it differs. Repository: the Paddock
monorepo's main checkout (about 92,000 packed objects in 3 packs, 5,203 loose objects, many linked worktrees).

| Variant | `open()` time |
|---|---|
| walk everything under `.git` (first version) | 698 ms |
| walk only `objects/`, `refs/` and `info/`, refuse a link at any other top-level name, skip hooks/logs/worktrees/modules | 25 to 28 ms |

The folders skipped are never opened by gix. Debrief's checkout (2 packs, 3 loose objects), full walk: 143 ms. The cost is
dominated by loose-object files and linked worktrees; a repository with hundreds of thousands of loose objects would still
pay one `lstat` each, bounded by the 1,000,000-entry cap.

## 2026-10-30, index run: parser pool, size and mtime shortcut, `.gitignore` walk, and a per-file wait found by measuring

Rule 48/51. Helpers (ignored tests, run by hand): `PN_MEASURE_REPO=<folder> cargo test --release -p pacenotes-worker --test it --
--ignored --nocapture measure_pool_and_shortcut` and the same with `-p pacenotes-engine ... measure_ignore_walk`. Machine: the
development Mac mini under normal load (5-minute load 15 to 22 during the runs), release build. Corpus: a Paddock worktree
(20,596 files without ignore rules: 6,550 parsed, 14,046 listed only, 184 over the size limit; no user data). One run per row.

**Bug found by this measurement.** The first pool run took 1,125 s for one worker (55 ms a file). Each parse call waited for its
monitor thread, which sampled the worker (a `ps` process on macOS) before looking at whether the call had ended, so every file cost a
`ps` run, and a beat (a second by default) whenever the call finished before the monitor first waited. Fixed: the monitor looks at
`done` first, takes no sample until its first beat and does not hold the lock while sampling; regression test
`a_call_that_finishes_does_not_wait_for_the_monitor` (a one-hour beat).

| Parse workers | First run (full index) | Speed-up |
|---|---|---|
| 1 | 20.6 s | 1.0x |
| 2 | 12.6 s | 1.6x |
| 4 | 9.8 s | 2.1x |

The read pass used 4 threads in all three. The gain flattens because the apply and SQLite steps are serial and the machine was
loaded; the Auto default (cores minus one, at most 8) stays, to be re-measured on an idle machine.

**The shortcut.** A second run over the unchanged tree of 20,596 files (2.3 GB on disk, build output included): 0.26 to 0.29 s, with 20,596 of 20,596 files not read (all shortcut hits),
against 9.8 to 20.6 s for the first run, so about 35x to 80x less. That is the cost of the folder watcher waking for an event that
changed nothing (an event inside `.git`, for instance).

**`.gitignore` walk** (second walk with the shared walker, `RepoFilesIgnore`; three rounds): the index's own walk without ignore
rules 220 to 230 ms (20,780 files; the first round 790 ms, cold cache); the shared walk 83 to 115 ms; the index's walk with the
ignore rules 120 to 128 ms (11,470 files). So honouring `.gitignore` costs about 205 ms against 225 ms without it, that is nothing
extra, and keeps 9,310 build-output files from being read and parsed.

**Cancellable "Finding files" (same day).** The shared `paddock_repo_files::walk` (the crate becomes `paddock-walk` after the shared-code freeze) cannot be cancelled and reports nothing until it
ends (7.5 s for 120,000 files on the reviewer's machine), so the engine runs it on its own thread and, meanwhile, a parallel
counting walk (`par_walk`, 4 workers) feeds the live count; a cancel returns at once and abandons the shared walk. On the 20,780-file
tree: shared walk alone 80 to 85 ms, shared walk plus counting walk 92 to 98 ms wall, that is 8 to 13 ms for the live count and the
cancel. Run it with `-p pacenotes-engine ... measure_ignore_walk`.

**Three walks per run.** A run walks the tree three times, and only the third is the index's own: (1) the shared
`paddock_repo_files::walk` for the `.gitignore` rules, single-threaded, on a thread of its own (80 to 85 ms on 20,780 files); (2) a
parallel counting walk beside it, for the live count and the cancel (about 8 to 13 ms more wall time; it ends before (1) does);
(3) the index's own walk, which applies the denylist, counts links and bounds depth and entries (110 to 130 ms with the ignore rules,
220 to 230 ms without them, because it then enters the 9,300 ignored files). Together about 200 to 230 ms on 20,780 files, against
220 to 230 ms for the one walk without ignore rules, so the three cost the same as one unfiltered walk and keep 9,300 files from
being read. At most one (1) runs at a time. Walks (1) and (2) become one, and walk (1) cancellable, when the shared walker is
unfrozen (ADR 0006).

## 2026-10-30, `pacenotes-rules` matcher (rule 48)

Helper: `cargo test --release -p pacenotes-rules --test it -- --ignored --nocapture measure_matcher`. Release build, the development Mac
mini at 5-minute load 16; one run. Corpus: a generated Rust file of 2,000 functions (161,780 bytes), invented code.

| Case | Time | Matches |
|---|---|---|
| typical rule `$X.unwrap()` | 25 ms | 2,000 |
| `$A + $B` | 23 ms | 2,000 |
| run placeholder `g($$$A)` | 25 ms | 2,000 |
| hostile: `f($$$A, $$$B, $$$C, $$$D, x)` against 40 arguments | 4.7 ms (ends by its step budget, no match) | 0 |
| hostile: 2,000-deep nesting | 2.4 ms | 0 |

So about 150 microseconds per kilobyte per rule, and each rule parses the file again (the parse is most of the 25 ms). With the
rules of a pack run one after another that is one parse per rule per file; sharing the index's parse tree across rules (take a `Tree`
in `find_all`) is the follow-up, measured when the engine wires the rules. The waiver glob is bounded by steps, not seconds: the
reviewer's case (`*a` fourteen times against 80 characters, which took over ten minutes) is checked as at most
`pattern length * path length * 2` steps in a unit test.

## 2026-10-30, blame and gix's similarity rename tracking (`Rewrites::default()`), parallelism and cost (ER-20, rule 48)

Helper: `cargo test --release -p pacenotes-git --test it -- --ignored --nocapture measure_blame`. Machine: the development Mac mini (Apple M4
Pro, 14 cores, 64 GB), 5-minute load 21 to 29 during the runs (other agents were building), release build, two runs per row. Repositories:
**invented and built in the scratch area** by the test (never a real repository): one file of 2,000 lines, 300 commits that each edit a
few lines of it; in the second repository the file is also renamed (`git mv`) every tenth commit (30 renames). The whole file is blamed
at the last commit.

| History | Rename tracking | Time | Hunks | Objects read |
|---|---|---|---|---|
| edits only | similarity (`Rewrites::default()`, what ships) | 278, 271 ms | 1,994 | 3,597 |
| edits only | none | 249, 274 ms | 1,994 | 3,597 |
| a rename every 10th commit | similarity (what ships) | 295, 264 ms | 1,994 | 3,747 |
| a rename every 10th commit | none | 1.2, 2.5 ms | **1** | 19 |

**Cheaper operation (ER-20).** Turning rename tracking off is not a saving: without it a renamed file's history ends at the rename, the
whole file goes to the commit that renamed it (one hunk, 19 objects, a millisecond), and the answer is wrong. With tracking on, the
extra cost over "edits only" is about 150 more objects (4%) and no measurable time at this size; it is the rename detection of the commits
that touch the file, not a search over the repository. So the default stays on, and the bounds that matter are the ones every blame
has: the object budget (default 2,000,000), the blob cap (2 MiB), `since`, and the line range. **Parallelism:** gix's blame walks one
history sequentially; the parallelism available is across files (one blame per file on a job pool), bounded by the object-database
reads (disk), a few at a time on an SSD, one or two on a hard disk; that belongs to the ownership view that calls it and is not
measured here. **Not measured:** a repository with hundreds of thousands of commits or very wide renames; the object budget stops those
with a limit error that says to set `since`.
