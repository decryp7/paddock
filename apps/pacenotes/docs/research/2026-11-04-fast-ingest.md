# Fast ingest: from a repository to a form every feature can query

Research note, 2026-11-04. Status: proposal for the user; nothing here is decided. Branch `docs/pacenotes-fast-ingest`, based on
`steward/pacenotes-stage1` at c1bba3f9f.

> **Superseded by [ADR 0012](../adr/0012-fast-ingest.md) where they differ.** The user decided on 2026-11-04 (section 10). Where
> this note and the ADR disagree, the ADR wins: change detection is the manifest alone (no Git accelerator), the fact cache is
> per repository identity and keyed by a hash of the raw bytes, and the review of 2026-11-04 added leases, Windows enumeration and
> logging. The note keeps the research and the measurements.
>
> | This note | ADR 0012 |
> |---|---|
> | D1 (the user's Git question) | D1 (manifest only, changed by the user) |
> | D2 (snapshot for serving) | D5 |
> | D3 (fact blobs per content key) | D3 |
> | D4 (cache across worktrees and clones) | D3 and D4 (per repository identity) |
> | D5 (DuckDB and DuckLake export only) | D8 |
> | D6 (text search without an index) | D6 |
> | D7 (memory budgets) | D7 |
> | D8 (rules in the worker) | D9 |
> | D9 (submodules on request) | D10 |
> | not in the note | D2 (pipeline), D6a (leases and GC), D11 (logging) |

**Question.** Pacenotes reads a repository and builds a graph. How do we make that ingest as fast as possible, and which intermediate
representation lets every later function (search, call graph, references, diagrams, call traces, MCP answers, review rules, coverage
mapping) answer quickly without parsing again?

**Short answer.** Make the parse result of a file a **content-addressed, immutable fact blob** (keyed by the file's content hash plus
the extractor, grammar and pack versions), so a file that any worktree, branch or earlier run has seen is never parsed again; find
what changed with **Pacenotes' own stat manifest for every folder** (decided: no Git fast path, never `git init`, never the git
binary); keep the catalog and the blobs in **SQLite** (one row per file instead of dozens); and serve every
query from a **derived, memory-mapped snapshot** per generation (interned strings, `u32` ids, sorted arrays, CSR adjacency, a name
FST). Memory is bounded by streaming and mmap (target, estimate: under 500 MB peak for a 100k-file first index, under 100 MB idle).
DuckDB and DuckLake fit analytics, not the interactive path. Section 7 has the recommendation per layer, section 9 the benchmark
that decides between the top two, and section 10 the questions, starting with the user's Git question. Every number measured here is
recorded in [performance.md](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks).

Sources were read on 2026-11-04 unless a date says otherwise. "Verified" means read in the primary source named; "estimate" means a
number I could not source and derived myself; "unverified" means a secondary source or a claim I could not confirm.

## 1. How stage 1 ingests today

Read from `origin/steward/pacenotes-stage1` (c1bba3f9f): ADRs 0002, 0003, 0005, 0006 and the crates `pacenotes-index`,
`pacenotes-parse`, `pacenotes-worker`, `pacenotes-engine`, `pacenotes-core`.

| Step | What it does | Where |
|---|---|---|
| Walk | three walks per run: the shared `.gitignore` walk (`paddock-repo-files`, single-threaded, not cancellable), a parallel counting walk for live progress, and the index's own walk (denylist, link refusal, bounds) | ADR 0006 "`.gitignore`"; `crates/pacenotes-engine/src/ignore.rs`, `crates/pacenotes-index/src/fs.rs` |
| Stamp shortcut | a file whose size and mtime match the stored row, and whose mtime is at least 2 s older than the read, is not read (git's racy rule) | ADR 0006 "Pool and the shortcut"; `crates/pacenotes-index/src/indexer.rs` |
| Read and fingerprint | SHA-256 over the content with `\r\n` and `\r` normalised to `\n`, on `par_map_ordered`, 4 threads on an SSD | `crates/pacenotes-core/src/ids.rs` (`Fingerprint::of_content`), ADR 0006 |
| Plan | added, modified, deleted, renamed (rename by fingerprint keeps the `FileId` and so every `SymbolId`) | ADR 0005 "Change plan"; `crates/pacenotes-index/src/plan.rs` |
| Parse | one `ParseRequest { path, bytes }` and one `ParseReply { facts }` per file over `paddock-ipc` (postcard), in a pool of worker processes (Auto: cores minus one, at most 8); the worker keeps no tree after the reply | ADR 0006 "The worker and its protocol"; `crates/pacenotes-worker/src/protocol.rs` |
| Extract | one `TreeCursor` walk per file, driven by the per-language table in `spec.rs` (not tree-sitter tags queries), bounded by 500,000 nodes and 2 MiB | `crates/pacenotes-parse/src/extract.rs`, `spec.rs` |
| Store | all results of the run are collected in one `Batch` in memory, then one SQLite transaction deletes and re-inserts every changed file's rows (`files`, `symbols`, `imports`, `calls`, `chunks`) with `tx.execute` per row | `crates/pacenotes-index/src/store.rs` (`commit`, `write_file`) |
| Query | `symbols_in_file` by index; `search` is `lower(name) LIKE '%q%'` over `files` and `symbols`; calls are stored as `caller TEXT, callee TEXT` with an index on `file_id` only | `store.rs` (`SCHEMA`, `search`) |

What it already does well: the stamp shortcut (a no-change re-run of 20,596 files took 0.26 to 0.29 s against 9.8 to 20.6 s for the
first run, [performance.md, 2026-10-30](../performance.md#2026-10-30-index-run-parser-pool-size-and-mtime-shortcut-gitignore-walk-and-a-per-file-wait-found-by-measuring)),
crash isolation of the grammars, deterministic ordering, "build new, then swap", and stable ids across renames.

### Where the bottlenecks would be

1. **The parse pass is far slower than the parser.** The recorded first run parsed 6,550 files in 9.8 s with 4 workers and 20.6 s with 1
   (performance.md, 2026-10-30), about 3 ms a file for one worker. Tree-sitter itself parses this repository's Rust and TypeScript at
   14.8 MiB/s on one core (about 1,870 files/s; [measurement](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks)),
   with the tags query about 1,300 files/s, 0.8 ms a file, so most of the 3 ms is not parsing (different corpora of the same repository; B1 measures both on one). The candidates, to be confirmed by benchmark B1: one IPC round trip and one copy of the bytes
   per file; the serial `apply` and SQLite step (the 2026-10-30 record says the gain flattens because of it); and the per-row
   `tx.execute`, which prepares the SQL again for every row (rusqlite caches statements only through `prepare_cached`), for about
   20 to 100 rows a file.
2. **Memory grows with the repository on a first index.** `par_map_ordered` returns every file's facts and `Batch` holds them all until the
   one commit. At Linux scale (108,158 files and about 43 million lines in Linux 7.2,
   [Phoronix](https://www.phoronix.com/news/Linux-7.2-43-Million-Lines), unverified secondary source) that is every symbol, call and
   chunk of the tree in RAM at once, as owned `String`s: estimate 2 to 6 GB, against the proposal's budget of about 2 GB in total on a
   16 GB machine (proposal 21.4).
3. **Facts are keyed by `FileId`, not by content.** A branch switch, a `git checkout` back and forth, a new worktree of the same
   repository (this family keeps many), or a vendored copy of a file all parse again, because the rows belong to a file id and are
   deleted when the content changes. A new worktree also defeats the stamp shortcut: every mtime is new.
4. **The queries the features need have no index.** "Find references" and "callers N levels" join `calls.callee` (a name, no index) to
   `symbols.name`: a full scan of `calls` per hop. `search` is a `LIKE '%q%'` over `lower()`, a full scan of both tables. Strings
   (qualified names, callee names, hex fingerprints, 16-byte blob ids) are repeated in every row, so the file is large and scans are
   slow. There is no text index yet (tier 1 search, `pacenotes-search`, is planned with FTS5, proposal 14).
5. **Every consumer parses again.** The rules engine parses the file once per rule (25 ms of a 25 ms match on a 160 KB file is mostly the
   parse; performance.md, 2026-10-30, "`pacenotes-rules` matcher"); the diagram and coverage readers will need spans and symbols per
   file; the worker keeps no tree, so ADR 0003's incremental re-parse (`Tree::edit` with the old tree) cannot happen.
6. **Three walks per run** (ADR 0006), about 200 to 230 ms on 20,780 files; known, and merged into one when `paddock-walk` gets a cancel
   token and a counter. Not the bottleneck, but it is per run and grows linearly.

## 2. How fast code indexers work (survey)

| System | What it stores | How it stays fast and incremental | Status, licence | Source |
|---|---|---|---|---|
| GitHub stack graphs | per-file graphs of definitions and references built from tree-sitter by a DSL; name resolution is a path search with a symbol stack at query time | each file is analysed alone, so unchanged files' results are reused; per-file graphs are merged at query time. No build, no CI job | **archived 2025-09-09**, read-only; MIT or Apache-2.0 | [GitHub blog, 2021-12-09, updated 2024-07-23](https://github.blog/open-source/introducing-stack-graphs/); [repository](https://github.com/github/stack-graphs) |
| GitHub code search (Blackbird) | sparse-gram index (variable-length n-grams chosen by bigram weights, fewer false positives than trigrams), shards keyed by Git blob id | **dedup by blob id**: 115 TB of code became about 28 TB of unique content; delta indexing crawls only blobs not in the parent repository, more than halving the documents crawled; about 120,000 documents/s ingest; per-shard p99 about 100 ms; index about 25 TB, a quarter of the raw corpus | proprietary | [GitHub blog, 2023-02-06](https://github.blog/engineering/architecture-optimization/the-technology-behind-githubs-new-code-search/) |
| Google Code Search | document-level trigram posting lists; a regex becomes an AND/OR query of trigrams, candidates are verified with the real regex | index about 20% of the source (420 MB of Linux 3.1.3 gave a 77 MB index); a literal query narrowed 36,972 files to 25, 0.01 s against 1.96 s brute force | `codesearch` BSD-3 (Go) | [Russ Cox, 2012-01](https://swtch.com/~rsc/regexp/regexp4.html) |
| zoekt | positional trigrams (offsets), so two trigrams at the right distance confirm a substring; shards of at most about 1 GB of content, mmap-friendly, versioned | index about 3x the corpus (offsets 2x, content 1x), a shard about 3.5x; needs about 1.2x the corpus in RAM; goal sub-50 ms on Android or Chrome (no measured figures in the doc); incremental by reindexing changed repositories and merging shards | Apache-2.0 | [zoekt design.md](https://github.com/sourcegraph/zoekt/blob/main/doc/design.md) |
| SCIP | protobuf `Index` of `Document`s, each with `Occurrence`s (range as 3 or 4 `int32`, a symbol string, a role bitset: definition, import, read, write, test...) and `SymbolInformation` with relationships (reference, implementation, type definition) | per document; human-readable symbol strings instead of LSIF's global numeric ids; LSIF was about 4x larger gzipped, 5x uncompressed; scip-typescript 10x faster than lsif-node in CI (not only from the format); Glean: SCIP "8x smaller, 3x faster to process" | Apache-2.0 | [scip.proto](https://github.com/sourcegraph/scip/blob/main/scip.proto); [announcement, 2022-06-08](https://sourcegraph.com/blog/announcing-scip); [repository](https://github.com/sourcegraph/scip) |
| LSIF | a graph of vertices and edges with global numeric ids, streamed as JSON lines | global ids make incremental and cross-repository indexing hard (Sourcegraph's reason for SCIP) | superseded by SCIP at Sourcegraph | [announcement](https://sourcegraph.com/blog/announcing-scip) |
| Meta Glean | immutable facts with user-defined schemas, forming a DAG, de-duplicated by the store; Angle, a Datalog-like query language; RocksDB | **stacked immutable databases**: an increment hides the *units* (files) that changed in the base and adds their new facts; ownership sets cost about 7% of DB size; indexing overhead 2 to 3%; typical queries under 10% slower, search-heavy ones about 3x | open source (Haskell and C++ server) | [introduction](https://glean.software/docs/introduction/); [incremental indexing, 2022-12-01](https://glean.software/blog/incremental/) |
| rust-analyzer | salsa: inputs (file texts) and derived queries, recomputed lazily on a revision bump | syntax trees are per-file values parsed in parallel; `ItemTree` summarises a file so that editing a function body never invalidates global derived data; everything in memory, nothing persisted | MIT or Apache-2.0 | [architecture](https://rust-analyzer.github.io/book/contributing/architecture.html) |
| Kythe | entries (source, kind, target, fact name, value) keyed by VNames (corpus, root, path, language, signature) | the storage format is explicitly not for serving: it is post-processed into serving tables; query efficiency is a non-goal of the store | Apache-2.0 (maintenance not confirmed, unverified) | [Kythe storage model](https://kythe.io/docs/kythe-storage.html) |
| clangd | dynamic index (open files), background index (all files, in a thread pool), optional static index; symbols, refs, relations, by `SymbolID` | one `*.idx` shard per file in `.cache/clangd/index/`, loaded at start so an unchanged project is not indexed again; LLVM or Chromium scale takes hours and several GB of RAM | Apache-2.0 with LLVM exception | [clangd indexing design](https://clangd.llvm.org/design/indexing) |
| IntelliJ stub indexes | a stub tree: the compact, serialised subset of a file's tree that other files need (declarations, names, modifiers), strings written once through `writeName` | a stub depends only on its own file; a format change bumps the stub version and rebuilds; other files resolve declarations from stubs without parsing | Apache-2.0 platform | [stub indexes](https://plugins.jetbrains.com/docs/intellij/stub-indexes.html) |
| tree-sitter tags | `queries/tags.scm` per grammar with `@definition.*`, `@reference.*`, `@name`, `@doc` captures; `tree-sitter tags` | the base of GitHub's search-based code navigation | MIT | [tree-sitter code navigation](https://tree-sitter.github.io/tree-sitter/4-code-navigation.html) |
| universal-ctags | one line per tag (name, file, pattern, fields such as kind, scope, signature, roles); some reference tags with roles | a hand-written or regex parser per language; references only for some languages | GPL-2.0 (licence from memory, unverified; not usable as a library under the family's licence policy anyway) | [ctags(1)](https://docs.ctags.io/en/latest/man/ctags.1.html) |
| ripgrep / grep-searcher | no index | parallel directory walk with `.gitignore`; a read buffer for many small files, mmap only for few large ones (mmap per file was 2.5 to 5x slower on the Linux tree); `PM_RESUME` over a built Linux checkout in 0.156 s on an i7-6900K (2016) | MIT or Unlicense | [ripgrep benchmarks, 2016-09-23](https://burntsushi.net/ripgrep/) |
| tantivy | Lucene-style inverted index, mmap directory | multithreaded indexing (English Wikipedia in under 3 minutes, the author's claim), start-up under 10 ms (claim) | MIT | [repository](https://github.com/quickwit-oss/tantivy) |
| SQLite FTS5 trigram | substring index usable by `LIKE` and `GLOB` (case-insensitive by default; at least 3 characters, or a linear scan); contentless and external-content tables | in the same SQLite file and transaction | public domain | [FTS5](https://www.sqlite.org/fts5.html) |

**What the fast ones have in common.**

1. **The unit of work and of invalidation is the file, and its output depends only on the file** (stack graphs, clangd shards,
   IntelliJ stubs, SCIP documents, Glean units). Cross-file meaning (which `parse` a call means) is computed at query time or in a
   cheap global link step, never stored inside a file's output.
2. **Content addressing** removes repeated work: Blackbird's blob-id dedup cut the corpus by 76%; clangd and IntelliJ reload per-file
   results instead of re-indexing.
3. **Storage is split from serving.** Kythe and Glean store facts and serve from derived tables; zoekt and codesearch serve from
   immutable, mmap'd files; nobody serves "callers 3 levels" from a general-purpose row store.
4. **Increments are layered, not edited in place** (Glean's stacked DBs, zoekt's shard replacement), so a reader always sees one
   consistent generation and a crash leaves the previous one intact.
5. **Strings are interned and ids are small** (IntelliJ `writeName`, SCIP's per-document symbol table, zoekt's `uint32` offsets).

## 3. Throughput

| Measure | Number | Source |
|---|---|---|
| tree-sitter parse only, 1 core, this repository's Rust and TypeScript | 14.8 MiB/s, 375k lines/s (Rust 14.0, TypeScript 15.3 MiB/s) | [measured](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks), 1-minute load 6 to 8 |
| tree-sitter parse plus the grammar's tags query, 1 core | 10.3 MiB/s | measured |
| the same on 2 and 4 threads (in process, one parser per thread) | 17.8 and 36.5 MiB/s | measured |
| incremental re-parse after a one-line edit (20 largest files) | 39x faster than a full re-parse (2.2 against 87.7 ms) | measured |
| tree-sitter, other grammars | about 5 to 12 MB/s per core | secondary benchmarks, unverified ([example](https://docs.rs/crate/weavatrix-parse/0.1.0/source/docs/comparison.md)) |
| content hash, 1 core: SHA-256 / BLAKE3 / xxh3-64 | 526 / 1,675 / 31,986 MiB/s (SHA-256 is sha2 0.10.9's software rounds: on aarch64 its hardware path needs the `asm` feature, which Pacenotes does not enable; sha2 0.11 detects the instructions at run time) | measured; source of `sha2` 0.10.9 `src/sha256.rs` |
| stage 1 parse pass end to end | 6,550 files in 20.6 s (1 worker), 9.8 s (4 workers) | [performance.md, 2026-10-30](../performance.md#2026-10-30-index-run-parser-pool-size-and-mtime-shortcut-gitignore-walk-and-a-per-file-wait-found-by-measuring) |
| stage 1 no-change re-run | 20,596 files in 0.26 to 0.29 s | same |
| Model2Vec potion-code-16M-v2 embedding | 6,365 chunks/s on 1 thread, 55,499 on 13; about 8 s per 1M lines | [performance.md, 2026-10-25](../performance.md#2026-10-25-amended-2026-10-28-embedding-engines) |
| Debrief's write paths (for comparison) | 8 Parquet writers 4.38M events/s; 1 DuckDB Appender 200k/s, 4 Appenders 355k/s | `apps/debrief/docs/research/2026-10-29-duckdb-write-paths.md` (origin/main) |

**What this means at scale (estimates from the rows above).** Linux 7.2 is about 108,000 files and 43 million lines (Phoronix,
unverified); at about 30 bytes a line that is roughly 1.3 GB of text (estimate). At 10.3 MiB/s per core, parsing it takes about
2 minutes on one core and roughly 10 to 20 s on 13 cores (estimate: 2 and 4 threads gave 1.7x and 3.5x here), if the rest of the
pipeline keeps up and the C grammar is as fast as the Rust one (not measured; B1 adds a C corpus). Stage 1's measured rate (about
670 files/s with 4 workers) would take about 160 s for the files alone and hold all facts in RAM. Chromium (with `third_party`) is
several times larger again (estimate; no primary count found). So the parse is not the limit; the per-file overheads and the store
are.

**mmap.** ripgrep's measurements say mmap per file is slower for many small files and faster for a few large ones. Source files are
small, so the reader keeps `read` into a reused buffer; mmap belongs to the **snapshot** (one large file, many random reads).

## 4. Change detection: "only prepare what changed"

> **Decided 2026-11-04 (user, D1 in section 10):** Pacenotes' own manifest for every folder, with no Git fast path. The Git
> measurements below stay as the evidence; the `GitAccelerator` of 4.3 is not adopted.

The user's input: *if the code is under Git, only prepare changes; if not, initialise Git to track changes.* This section measures
both halves and proposes one mechanism with a Git accelerator. Measurements: [performance.md](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks).

### 4.1 Under Git

| Question | Measured or found | Source |
|---|---|---|
| `git status --porcelain=v2 -uall` on this repository (this branch's worktree at c1bba3f9f, 11,867 tracked files), git index fresh | 0.07 to 0.08 s warm, 0.34 s the first time | measured |
| the same on a local mirror of the public repository github.com/UberGuidoZ/Flipper at commit 9bec07fa2 (2024-12-19; 27,390 tracked files, 27 submodules) whose index stat data is **stale** (27,363 of 27,390 entries; the checkout had been copied, so every inode and ctime differs) | **5.9 s every time**, and 33 s cold: git re-reads and re-hashes every stat-dirty file, and with `GIT_OPTIONAL_LOCKS=0` (what a read-only tool must use) it cannot write the refreshed index, so it pays again on every call | measured; the mechanism is git's own ([racy-git](https://git-scm.com/docs/racy-git): 2.22 s against 0.14 s on a 20,000-file Linux tree once refreshed) |
| a **private index file** (`GIT_INDEX_FILE` in Pacenotes' data folder): `read-tree HEAD`, then `update-index --refresh` once | read-tree 0.03 s; first refresh 5.9 s (hashes everything once); afterwards status 0.03 s (`-uno`) and 0.07 s (untracked too); index file 3.3 MB | measured |
| `git ls-files -s` (path, mode, blob id per entry) | 0.01 s for 11,867 and for 27,390 entries | measured |
| `git ls-tree -r HEAD` (blob ids of the commit) | 0.17 s (11,867) and 0.12 s (27,390) | measured |
| `git diff --name-status <indexed> HEAD` | 200 first-parent commits apart (3,368 paths): 0.82 s with rename detection, 0.01 s without; 2,000 commits apart (11,086 paths): 0.05 s, rename detection skipped by git's `renameLimit` | measured |

**What this shows.**

1. **Git's change detection is only as fresh as its index, and keeping it fresh means writing into the user's `.git`.** A read-only
   caller of `git status` on a repository whose index is stale pays a full content scan on every call (5.9 s for 27k files here).
   Pacenotes may not write into the repository (proposal 1, "the one hard rule"; read-only towards repositories). A private index
   outside the repository solves it (0.03 to 0.07 s after one refresh), but that is the shadow-repository idea in another form.
2. **Shelling out to `git` runs what the repository and the user configure.** `git status` calls `core.fsmonitor` and the clean
   filters (`filter.lfs.clean` is set globally wherever Git LFS is installed) on stat-dirty files. ADR 0004 already decided that
   Pacenotes never runs the git binary and opens repositories through gix *isolated*, with fsmonitor forced off and no filter
   pipeline. So `git status` and `git diff` as subprocesses are out; the same operations through gix are in, where ADR 0004 allows them.
3. **Blob ids are the free content keys.** The git index stores a blob id per path (`ls-files -s`, 0.01 s for 27k entries), and the
   HEAD tree has one per path (`ls-tree -r`, 0.12 s). When a file's stat matches its index entry, its blob id names its content without
   reading it. A blob id seen before (another branch, another worktree, a revert) maps to a fact blob that is already parsed.
4. **Tree-to-tree diffs are cheap without rename detection** (0.01 to 0.05 s) and slow with it (0.82 s for 3,368 paths). Pacenotes
   detects renames itself, by content key (ADR 0005), so it never needs git's similarity renames for indexing.

**gix against the git binary.**

| | gix (gitoxide) | git binary |
|---|---|---|
| Licence | MIT or Apache-2.0 ([repository](https://github.com/GitoxideLabs/gitoxide)) | GPL-2.0, run as a separate program (licence from memory, unverified) |
| Already in Pacenotes | yes, `gix` 0.90 in `pacenotes-git` (ADR 0004), features `sha1`, `revision`, `parallel`, `blame` | no; ADR 0004 forbids running it |
| Runs repository-configured programs | not when opened isolated with fsmonitor off and no filter pipeline (ADR 0004 layers 1 and 2) | yes: fsmonitor, clean filters, `include.path`, unless every one is overridden on each call |
| Index reading (blob ids, stat) | `gix-index`: V2 to V4, extensions TREE, UNTR, FSMN, `sdir` (sparse), "usable" tier ([crate-status](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md)) | yes |
| Status (index against worktree, untracked) | `gix-status` and `gix-dir`: listed with untracked files, sparse index and fsmonitor; submodules detected but not walked into (crate-status) | yes |
| Tree diff | `gix-diff`, renames by identity and similarity; no find-by-basename (crate-status) | yes |
| Speed | no published comparison found; the project's goal is to be the fastest implementation (claim) | measured above |
| Windows | tested in CI; Windows failures block releases (repository README) | yes |
| A missing git | works without any git installed | needs git on `PATH`, and its version varies |

So gix, read-only. The parts used here are reading the index file **as data** (blob id and stat per path) and diffing trees. ADR 0004
today refuses the index loaders in `pacenotes-git` because the danger it guards against is the status and filter pipeline, not the
parse of a file. Reading the index needs a short amendment to ADR 0004: the index is read as data, the worktree stat comparison is
Pacenotes' own (section 4.3), and gix's `status` is still never called.

**Edge cases under Git.**

- **`.gitignore`.** The walk already applies it (ADR 0006, `paddock-repo-files`). Tracked files that are also ignored stay tracked
  in git's sense; Pacenotes follows the walk, which lists only what is on disk and not ignored, so the two agree on what is indexed.
- **Submodules.** A gitlink (mode 160000) is a commit id, not content. Each submodule is its own repository: indexed as its own
  repository identity with its own cache keys, and joined into the parent's view on the snapshot level. Default: listed as a boundary
  with an "Index submodules" action (the measured 27k-file checkout has 27 of them; walking into them silently could double the work).
  gix detects but does not walk into submodules (crate-status), which matches this.
- **Git LFS pointers.** In the object database an LFS file is a pointer (under 1024 bytes, first line
  `version https://git-lfs.github.com/spec/v1`, then `oid sha256:` and `size`, [spec](https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md)).
  Its blob id names the pointer, not the content on disk. Rule: a path whose blob is a pointer never uses the blob id as its content
  key; the working-tree file is read and hashed like a non-git file (and is usually a binary asset that stays at tier 1). Pacenotes
  never runs the LFS smudge filter.
- **Sparse checkouts.** Entries with the skip-worktree bit, or `sdir` sparse-directory entries, are not on disk. Default: index what
  is on disk (the walk decides). Option: index the rest from the object database at HEAD (blobs read as stored, as ADR 0004 already
  does for history), marked "not checked out".
- **Line endings.** `Fingerprint` normalises CRLF; a blob id hashes the content as stored. So the cache maps blob id to
  fingerprint once, when a blob is first read, and never assumes they agree (`core.autocrlf` and `.gitattributes eol` differ per
  repository).
- **Linked worktrees** read the common git folder (ADR 0004 "A linked worktree is analysed through its main checkout"), so all
  worktrees of a repository share blob ids, which is what makes the shared fact cache (section 7.3) pay off.

### 4.2 Not under Git: do not `git init` in the user's folder

`git init` in the user's folder writes `.git` into their files, can be committed by their other tools by accident, clashes with SVN,
Perforce and Mercurial working copies and their ignore rules, fails on read-only folders and network shares, and breaks the hard rule
(read-only towards repositories). Two alternatives:

| | (a) Pacenotes' own manifest | (b) a shadow repository outside the folder |
|---|---|---|
| What | a table in Pacenotes' data folder: path, size, mtime (ns), ctime, inode or Windows file id, device, content key (BLAKE3 or SHA-256 of normalised bytes); git's stat rule: same stat means skip, otherwise hash, re-parse only on a new hash | `git --git-dir=<data>/<id>.git --work-tree=<folder> add -A`, then `status` |
| First run, this repository (11,778 files, 486 MiB tracked incl. images) | one read and hash of every file: the read is the cost (I/O bound; hashing at 1.7 GB/s BLAKE3 or 0.5 GB/s SHA-256 is minor) | **34.3 s**: git compresses (zlib) and writes every file as an object, 11,158 loose objects |
| No-change run | stage 1 measured: 20,596 files in 0.26 to 0.29 s (three walks and an `lstat` each) | 0.06 to 0.07 s for `status` (one walk, its own `lstat`s, untracked cache off) |
| After `touch` (stat changes, content does not) | re-hash that one file, keep its key | 0.07 s (git re-checks and rewrites its private index) |
| Disk | about 100 to 150 bytes a file: about 1.5 MB for 12k files, 15 MB for 100k (estimate) | **a second, compressed copy of the whole folder**: 398 MB loose, 371 MB after a 15.7 s `gc` |
| Executes programs | no | git runs global filters (LFS) and `core.fsmonitor` unless each is overridden; the git binary must be installed (ADR 0004 says never) |
| Renames | by content key (ADR 0005) | git's similarity detection, a second rename model |
| mtime granularity (FAT 2 s, SMB, NTFS 100 ns) | racy rule: a file whose mtime is not at least 2 s older than the read is re-checked by content (ADR 0006); FAT and some network shares get a "re-check by content" setting | the same racy rule, inside git |
| Clock skew (network drives, VMs) | compare stat values for equality, never "newer than"; a skewed future mtime only causes a re-hash | the same |
| Network drives | `lstat` latency dominates; the watcher may not fire (SMB, NFS); a periodic stat pass at low priority with a Setting | the same, plus object writes over the network if the data folder is remote (it is not: local app data) |
| Windows | file id from `GetFileInformationByHandle` instead of inode; mtime and size as above | needs git for Windows installed |

**Recommendation: (a), the manifest, for every folder.** It is what stage 1 already has in `files` (size, `modified_ns`,
`read_at_ns`, fingerprint), plus inode or file id and ctime. (b) costs a full compressed copy of the code on disk and a
compression pass on first open, needs the git binary, and runs configured filters. The one thing (b) gives that (a) does not is
history of a non-git folder, which Pacenotes does not need for indexing.

### 4.3 One interface for both (as proposed; the `GitAccelerator` part was not adopted, ADR 0012 D1)

Both paths produce the same thing for the indexer, so it never knows whether Git was involved:

```rust
/// What changed since the last generation, and the content key of everything present (pure data; the indexer's input).
pub struct ChangeSet {
    pub generation_base: Generation,
    pub entries: Vec<Entry>,          // sorted by path
}
pub struct Entry {
    pub path: RepoPath,
    pub stat: Stat,                   // size, mtime_ns, ctime_ns, file id
    pub key: ContentKey,              // Known(Fingerprint) | GitBlob(ObjectId) | Unknown (must read and hash)
    pub change: Change,               // Unchanged | Added | Modified | Deleted | Renamed { from }
}
```

A `ChangeSource` port has two adapters: `ManifestSource` (walk plus stat against the manifest; every folder) and `GitAccelerator`,
which only fills `key` for files that `ManifestSource` would otherwise mark `Unknown`: when a file's stat matches its entry in the
git index (read as data through gix), its key is the blob id, and the blob-id-to-fingerprint map turns it into a fact-cache hit with
no read. That covers the expensive cases (a new worktree, a fresh clone, a checkout of another branch) where every mtime is new and
the manifest alone would read everything. A stale git index (the 27k-file case) simply gives no keys, and the manifest path reads and
hashes, once.

For "index as of commit X" (diagrams of a release, review of a range) the `GitAccelerator` also offers `tree_keys(commit)`: the blob
id of every path at that commit (0.12 s for 27k entries), with blobs read from the object database for misses. Combined with the
content-addressed fact cache, a snapshot for another commit costs only the blobs never seen before.

### 4.4 The watcher (ADR 0006)

The watcher stays as ADR 0006 has it: OS notifications through `paddock-watch`, debounced (quiet 1 s, at most 5 s), one run at a
time. A run is now `ChangeSource` then the pipeline: one walk and `lstat` per file against the manifest (the no-change case is the
measured 0.26 s on 20k files), reads only for stat changes, parses only for new content keys. Two additions: (1) when
`paddock-watch` carries paths (its known limit today), the run stats only those paths and their folders instead of the whole tree;
(2) (not adopted, ADR 0012 D1) a change of `.git/HEAD` or `.git/index` (a checkout, a pull) is a hint to run the `GitAccelerator` before the walk, so a branch
switch of thousands of files turns into blob-id cache hits rather than reads. Events inside `.git` still start a cheap run, as today.

## 5. Stores compared

### 5.1 The metadata store (manifest, content key to fact blob, versions)

Workloads, from the [rerun after review](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint)
(three interleaved rounds, median and range, physical footprint; the raw output is linked there). Both use 1,000,000 rows:

- *manifest*: 40-byte path keys in walk order, 64-byte values, loaded in commits of 10,000 while a second thread does point lookups;
- *fact_keys*: uniform random 32-byte hash keys with 8-byte values, the shape of ADR 0012's key index, with and without the reader
  thread as a control.

After the load come 200,000 point lookups and 100 commits of 100 upserts. Durability is each store's default here: redb, fjall and LMDB
sync every commit. SQLite WAL `NORMAL` does **not** fsync on commit, only at checkpoints; `FULL` does.

| Store | Manifest load | fact_keys load (no reader) | Reads during load (worst) | Look-up | 100-row commit | Disk (manifest / fact_keys) | Footprint peak |
|---|---|---|---|---|---|---|---|
| SQLite WAL `NORMAL` (rusqlite 0.40.2) | 307k/s | 25k/s (21k/s) | 297k/s (22 ms) | 2.2 to 2.6 µs | 17 to 21 ms | 125 / 81 MiB | **5 to 9 MiB** |
| SQLite WAL `FULL` | | 21k/s | 176k/s (71 ms; once 1.3 s) | 2.1 µs | 18 ms | / 81 MiB | 8 MiB |
| redb 2.6.4 | 157k/s | 20k/s (18k/s) | 956k to 972k/s (6 ms) | 1.1 µs | 40 to 69 ms | 258 / 129 MiB | 185 to 191 MiB |
| LMDB, heed 0.22.1 | 382k/s | 24k/s (29k/s) | 1.1M/s (under 4 ms) | 1.0 µs | 48 ms | 108 / 185 MiB | 5 to 63 MiB |
| fjall 2.11.2 | 170k/s | **314k/s** (246k/s) | 580k to 759k/s (56 to 73 ms) | 2.4 to 4.8 µs | 14 to 21 ms | 45 / 62 MiB | 90 to 122 MiB |

What the table shows:

1. **Key order decides B-tree ingest speed.** Every B-tree store (SQLite, redb, LMDB) loads walk-ordered keys 6 to 15x faster than
   uniform random hash keys. The no-reader control barely changes that, so it is the stores' own cost: random commits dirty most leaf
   pages. The manifest is the fast case. A table keyed by a hash is the slow one, which is why ADR 0012 appends blobs in insertion
   order and keeps the hash in a narrow index. fjall, an LSM tree, does not care about key order.
2. **A 100-row commit costs 14 to 69 ms** whatever the store, even SQLite `NORMAL` without an fsync, because random upserts into a
   million-row tree read pages from disk. A watcher run should therefore be one commit.
3. **Physical footprint separates the stores:**
   - SQLite stays at 5 to 9 MiB with its default page cache.
   - LMDB's mapped pages are mostly clean, so its footprint is 5 MiB for the manifest and 63 MiB for the hash keys.
   - fjall holds 90 to 122 MiB and redb 185 to 191 MiB.
4. **Point look-ups take 1 to 5 µs everywhere**, so none of the stores is the bottleneck for look-ups.

The first run's RSS figures, measured with path keys in random order, are kept in the older block of performance.md as history. They
are not rule-26 evidence.

| Store | Crash safety | Readers during a write | Several processes on one store | Windows and macOS | Licence | Build |
|---|---|---|---|---|---|---|
| **SQLite** WAL (rusqlite, bundled) | ACID; WAL; `synchronous=NORMAL` does not fsync on commit (only at checkpoints), so a power loss can lose the last commits, never consistency; `FULL` fsyncs each commit | yes, readers see the last commit (WAL) | **yes**: readers and one writer across processes on a local disk; not on network file systems | same on both; the family's most used store | public domain; rusqlite MIT | C compile of the amalgamation, already in the family's builds (Dyno, Pitwall, Spotter, Pacenotes) |
| **redb** | ACID, copy-on-write B-tree, 1PC+C (one fsync) by default ([design](https://github.com/cberner/redb/blob/master/docs/design.md)) | yes (MVCC, one writer) | modes for one exclusive writer, single writer plus read-only processes, or several writers, through file range locks (design doc) | pure Rust | MIT or Apache-2.0; 4.4.0 (2026-10-13) | pure Rust, about 5 s here |
| **fjall** | LSM with a journal; a commit is durable only after `persist(SyncAll)` (README) | yes | **no**: "must not be opened from separate processes" (README) | pure Rust | MIT or Apache-2.0; 3.1.13 (2026-10-25) | pure Rust, about 5 s here (with `lsm-tree`) |
| **LMDB** (heed) | ACID, copy-on-write B-tree, mmap | yes, readers never block | yes (mmap and a lock file) | the map size is fixed up front; on Windows the file is grown to it (unverified) | OpenLDAP licence (permissive); heed MIT; 0.23.1 (2026-10-26) | small C library, about 2 s here |
| **RocksDB** (rust-rocksdb) | WAL plus LSM | yes | one writer process; others as read-only or secondary instances (unverified) | C++ toolchain on every target; long compile (estimate several minutes at 2 jobs) | Apache-2.0 or GPL-2.0 dual; crate 0.25.0 (2026-08-16) | not built here: cost of the C++ build is the reason |
| **DuckDB** | ACID, WAL, checkpoint | yes, within the process (MVCC) | **only read-only** across processes; one read-write process ([concurrency](https://duckdb.org/docs/2.0/connect/concurrency.html)) | macOS and Windows; prebuilt `libduckdb` per platform | MIT; duckdb-rs 1.20000.1 (2026-10-28) | large; Debrief links a prebuilt library |
| **sled** | beta; on-disk format will change before 1.0 and need manual migration; main branch is a rewrite ([README](https://github.com/spacejam/sled)) | | | | MIT or Apache-2.0; last release 0.34.7 (crates.io 2024-10-11) | **not a candidate** |

**DuckDB as a metadata store.** It is columnar: excellent for scans and aggregates, weak for point lookups and frequent small writes.
Family numbers: one in-process Appender writes 200k rows/s and native DuckDB tables wrote 36.5k events/s in Debrief's R2b, against
Parquet's 0.97 to 1.7M; a page of 500 rows over a Parquet view takes 13 ms warm, over native tables 64 to 99 ms
(`apps/debrief/docs/research/2026-10-29-duckdb-write-paths.md`, `2026-10-28-duckdb-server-mode.md`); the vendor's own small-transaction
figure over Quack is 1,038 transactions/s on one thread (same note). Against 2.6 µs for a point lookup in SQLite
(table above) a DuckDB query is milliseconds. And `memory_limit` defaults to 80% of RAM with one thread per core
([configuration](https://duckdb.org/docs/2.0/configuration/overview.html)), and Debrief found the process grows past
`memory_limit` (it covers only the buffer manager). Not for metadata.

**Recommendation: SQLite (WAL, rusqlite bundled).** It already is the family's metadata store (ER-22: Dyno, Pitwall, Spotter and
Pacenotes use rusqlite 0.40 bundled; Paddock has no shared key-value crate, `paddock-storage` is about clearing app folders, not a
store), it is fast enough for the manifest (about 300k rows/s loaded in walk order, 2 to 3 µs point lookups) **and** it is the only candidate that lets the MCP server process (`pacenotes serve
--stdio`, proposal 14) read while the app writes, it is crash-safe, its physical footprint stayed at 5 to 9 MiB, and its default page cache is about 2 MB per connection (SQLite's
default `cache_size` of -2000 KiB; from the SQLite docs, not re-read today). redb is the fallback if a measurement ever shows SQLite as
the bottleneck for metadata: pure Rust, ACID, multi-process modes, 1.1 µs lookups and the best reads during a write, but 185 to 191 MiB of physical footprint and slow random-order loads.

**The one weak spot is a table keyed by a content hash** (uniform random keys): 18k to 29k rows/s at a million rows, in every B-tree store.
So the fact blobs are not stored keyed by their hash. They go into an append-ordered table (`facts(rowid INTEGER PRIMARY KEY, blob)`,
written in insertion order, the fast case) with a narrow index `fact_keys(fact_key BLOB PRIMARY KEY, rowid) WITHOUT ROWID` (40-byte
rows, many to a page; measured as the fact_keys workload above, 21k to 27k rows/s in SQLite); or into append-only segment files with the same narrow index (the
contender in B9). If the narrow index is still the bottleneck at Linux scale, fjall (246k to 314k random rows/s, 62 MiB on disk, about 100 to 120 MiB footprint) can hold just that
index, owned by the indexer process (fjall cannot be shared between processes, and nothing but the indexer needs it).

### 5.2 Bulk facts, graph and search: the candidates

The four queries: **refs** (find references), **callers 3** (callers three levels up), **in file** (symbols in a file), **path**
(a path between A and B). "Measured" numbers are the synthetic graph (1,000,000 nodes, 5,000,000 call edges; invented, skewed so a
few hubs have many callers) in [performance.md](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks).
Versions and dates from crates.io and the repositories, checked 2026-11-04.

| Candidate | State (checked 2026-11-04) | Licence | refs | callers 3 | in file | path | One file changed | Memory | Share with other apps |
|---|---|---|---|---|---|---|---|---|---|
| **Custom: mmap'd CSR snapshot + Arrow IPC or postcard arrays** (baseline) | our code | ours | **4 µs** for direct callers in memory (0.5 ms on a cold mapped file) | **5 µs** (BFS with a visited stamp, 0.023 µs a node) | a range in a per-file array (sub-µs, estimate) | bidirectional BFS: **32 µs** (1,282 nodes visited on average) | rebuild or a delta overlay (B4 measures the real link; both CSR directions build from an in-memory edge list in 161 ms) | **45.8 MiB** for 5M edges, both directions; mmap, so resident only what is touched | answers over MCP; the format is private |
| **SQLite, normalised tables** (interned names, integer ids, two edge indexes) | 3.5x; family store | public domain | **2 µs** (cache sized to the data) | **75 µs** (the same BFS over indexed look-ups, 0.373 µs a node) | indexed, µs | **474 µs** (the same bidirectional BFS over look-ups) | delete and insert the file's rows; one transaction | file **222 MiB** for the same edges (measured: table plus two indexes); page cache bounded by `cache_size` | the family's store; a second process can read it |
| **DuckLake** (DuckDB extension; catalog in SQLite, PostgreSQL, MySQL or DuckDB; data as Parquet) | **1.0, 2026-04-13**, "production-ready" with guaranteed backward compatibility ([ducklake.select](https://ducklake.select/)) | MIT | a Parquet scan with zone maps: milliseconds (Debrief: 13 ms for a 500-row page over Parquet) | recursive CTE over Parquet, milliseconds to seconds (estimate) | partition prune, ms | recursive CTE | **each insert writes new Parquet files** unless data inlining is used; small files need `ducklake_merge_adjacent_files` ([compaction](https://ducklake.select/docs/stable/duckdb/maintenance/merge_adjacent_files)) | DuckDB's buffer manager: `memory_limit` default 80% of RAM, must be capped; grows past it (Debrief) | Debrief's trial is queued (memory note); multi-process writes need a PostgreSQL catalog or Quack |
| **Lance** (lancedb format, Rust) | `lance` 14.0.2 (2026-10-29); very active; stable storage versions are a compatibility contract (README) | Apache-2.0 | random access by row id is its strength ("100x faster than Parquet", vendor claim); by key needs a scalar index (unverified for our shape) | not native: application BFS over row reads (estimate: tens of µs per hop) | scan with filter | not native | versioned appends and deletes (fragments, deletion files); compaction needed | Arrow and DataFusion runtime; caps not documented in the pages read (unverified) | could hold both facts and vectors; heavy dependency set; Windows not stated in the README |
| **Delta Lake** (delta-rs) | `deltalake` 1.1.2 (2026-10-17) | Apache-2.0 | scan of Parquet via DataFusion, ms | not native | scan | not native | a new Parquet file and a log entry per commit; small-file compaction | DataFusion | built for data lakes and object stores; **overkill locally** |
| **Apache Iceberg** (iceberg-rust) | `iceberg` 0.11.0 (2026-11-03), pre-1.0; feature status on a separate page | Apache-2.0 | as Delta | not native | scan | not native | manifests and snapshots per commit | as Delta | catalog servers (REST, Glue, HMS); **overkill locally** |
| **Kùzu** (embedded property graph) | **archived 2025-10-10**, last release 0.11.3 ([repository](https://github.com/kuzudb/kuzu)) | MIT | fast | native Cypher, fast | fast | native | per edge | own buffer pool | **not a candidate**: unmaintained |
| **CozoDB** (Datalog, backends SQLite, RocksDB, sled, memory) | `cozo` 0.7.6, last published **2023-12-11** | **MPL-2.0** (weak copyleft, allowed by the family policy at file level) | ms | recursive Datalog; vendor: two-hop under 1 ms on 1.6M vertices and 31M edges (2020 Mac mini, RocksDB, [README](https://github.com/cozodb/cozo)) | ms | native | per fact | its backend's | **not a candidate**: no release in almost three years |
| **Oxigraph** (RDF triple store, SPARQL) | 0.5.13 (2026-10-22), active | MIT or Apache-2.0 | ms (SPARQL) | property paths | ms | property paths | per triple, RocksDB underneath | RocksDB's | not relevant: RDF adds URIs and triples to every fact for no gain here |
| **Glean-style fact store over RocksDB** | Glean is Haskell and C++ with a Thrift server; the idea needs our own fact layer over rust-rocksdb | Glean BSD (unverified); RocksDB Apache-2.0 or GPL-2.0 | fast (indexed facts) | Angle is non-recursive (Glean docs): N levels is N queries | fast | not native | stacked increments that hide changed units; about 7% size, under 10% query overhead, search-heavy queries about 3x ([Glean blog](https://glean.software/blog/incremental/)) | RocksDB block cache and memtables | adopt the ideas (units, stacking), not the system: a C++ build and a second engine |
| **Text: none** (grep-searcher) | ripgrep crates, active | MIT or Unlicense | a literal over a Linux checkout in 0.16 to 0.33 s warm (2016) | | | | zero | the read buffer | MCP tool |
| **Text: document trigram index** (codesearch design) | our code | ours | candidates in ms, then verify (Cox: 0.01 s against 1.96 s) | | | | posting lists per file | index about 20% of the source; mmap | MCP tool |
| **Text: positional trigrams** (zoekt design) | Go reference | Apache-2.0 | faster verification | | | | per shard | about 3.5x the source; 1.2x the corpus in RAM | |

**Depth is unlimited by design.** "Callers 3 levels" above is one sample depth, not a limit. Callers, callees and paths are a BFS
with a visited set: every node is touched at most once, so cycles (recursion, mutual calls) end the walk and the cost is bounded by
the reachable part of the graph. Measured on the same 1M-node, 5M-edge graph ([rerun](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint)):

| Query (rerun on one query set; time per node in brackets) | CSR | SQLite, BFS over indexed look-ups | SQLite, recursive CTE (`UNION` on the node) |
|---|---|---|---|
| all callers, unlimited depth (10 queries, avg 496,497 reached) | **20.4 ms** (0.041 µs) | 882 ms (1.78 µs) | 1,323 ms (2.66 µs) |
| all callees, unlimited depth (avg 588,448 reached) | **28.3 ms** (0.048 µs) | 1,043 ms (1.77 µs) | 1,494 ms (2.54 µs) |
| path A to B, bidirectional BFS (200 pairs, 125 connected, 1,282 visited on average) | **32 µs** | 474 µs | no early exit or two-sided search in SQL |

So unlimited depth is affordable on CSR (tens of ms for the whole reachable graph, and a two-sided path search touches about a
thousand nodes) and about 40x slower per node in SQLite (about a second for the whole reachable graph). A real call graph is sparser than this random one, so the reach is likely smaller;
IG-B3 measures it on Linux. **UI guidance:** the view shows levels progressively (one, two, three, more on "Show next level"), with
counts of what is hidden, and "All callers" (or callees) is an explicit action that runs as a cancellable query with a live count of
nodes reached, because its answer can be most of the repository.

**DuckLake, specifically.**

- *Catalog and data.* A SQL catalog (SQLite, PostgreSQL, MySQL or DuckDB) holds tables, snapshots and file lists; data is Parquet. The
  catalog in SQLite would sit next to Pacenotes' own SQLite, but it is DuckDB's schema, written through the DuckDB extension, not ours.
- *Snapshots and time travel.* Every commit is a snapshot; queries can read `AT (VERSION => n)`; DuckLake 1.0 claims unlimited
  snapshots "without expensive compaction steps". Small inserts make small files; `ducklake_merge_adjacent_files` compacts them while
  keeping time travel (docs above).
- *"Index as of commit X".* A DuckLake snapshot is a catalog transaction, not a git commit. Mapping commit X to snapshot n is a row in a
  table of ours; the snapshot only exists if commit X was ingested. The content-addressed fact cache gives the same thing more cheaply:
  the index for commit X is the set of content keys of the files at X (read at that revision through `History`, ADR 0004), and only
  content the repository's cache has not seen is parsed.
- *Latency.* Point queries are DuckDB queries over Parquet: milliseconds (Debrief measured 13 ms warm for a 500-row page), against
  2 µs (SQLite, indexed) and 4 to 5 µs (CSR, direct and three levels) in the rerun here. Three orders of magnitude
  for an MCP tool that walks a graph hop by hop.
- *Fit for Pacenotes' writes.* The ingest is many small per-file results, rewritten by the watcher every few seconds. That is the
  small-file pattern DuckLake has to inline or compact; Debrief's import (millions of rows in big batches, rarely rewritten) is the
  pattern it is built for.

**One shared approach with Debrief? (ER-22).** Partly. Shared: the idea of immutable, content-addressed segments plus a small SQL
catalog, the Parquet/Arrow tooling (Debrief already has arrow-rs 58 and parquet 58), and the memory discipline (caps, spill). Not
shared: the query layer. Debrief's queries are scans, facets and histograms over millions of events, which DuckDB is built for;
Pacenotes' are point lookups and graph walks, which it is not. If Debrief's DuckLake trial succeeds, Pacenotes can *export* its
facts as Parquet into a DuckLake catalog for analytics (hot spots over history, coverage joins), sharing Debrief's conventions,
without putting DuckDB on its interactive path. The rule of three applies to a shared segment-and-catalog crate: two apps is not yet
enough to extract one.

## 6. Memory as a first-class criterion

> The measured memory figures in this section are the first run's RSS, superseded by the physical-footprint
> [rerun](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint) and by ADR 0012 D7, whose budgets
> and enforcement are the decision. The estimates below are unchanged.

**How each part bounds memory.**

| Mechanism | Where | Effect |
|---|---|---|
| Streaming parse: one tree per worker, dropped after extraction | parse worker | a worker holds one file's tree; measured peak RSS (not footprint) of an in-process parse of 6,419 files with all 51 MiB of source in RAM: 96 MiB, so the parse state itself is small |
| Bounded channels with backpressure, bounded **by bytes** not by count | read pool to workers to writer | in-flight source at most a budget (default 64 MiB, estimate) whatever the file sizes |
| Write blobs as they arrive, swap only the small `files` map | writer | replaces stage 1's whole-run `Batch` (O(repository)) with O(one batch) |
| Interned strings and `u32` ids | blob string table, snapshot arena | names stored once per file and once per snapshot |
| mmap instead of heap | snapshot, trigram index | the OS page cache holds what queries touch; clean pages are reclaimable and are not the app's private footprint |
| Columnar or compressed blobs | fact blobs (postcard, optional zstd), Parquet export | smaller on disk and in the page cache |
| Spill to disk | link step | external sort of `(name, id)` runs when the edge list exceeds the budget |
| No whole graph in RAM | serving | only the mmap'd CSR; never a pointer graph of nodes |

**Estimates per repository size** (to be replaced by B7; private footprint, not RSS; Linux-kernel size taken from Phoronix as above):

| | Medium (~10k files, ~1.3M lines, this repository) | Large (~100k files) | Huge (Linux, ~108k files, ~43M lines) |
|---|---|---|---|
| Peak during first ingest, recommended pipeline | about 200 to 300 MB: 8 workers at 20 to 40 MB each (estimate from the 96 MiB in-process run), 64 MiB in flight, SQLite cache | about 300 to 450 MB: the same, plus the link step's sort buffer (128 MB budget) | about 350 to 500 MB with spill: workers and buffers as before; the link step spills (30M+ edges at 8 bytes each is 240 MB before spilling) |
| Peak during first ingest, stage 1 as built | facts of every file held until commit (estimate 0.3 to 1 GB) | estimate 2 to 4 GB | estimate 3 to 6 GB |
| Idle | under 50 MB private plus clean mmap pages | the same | the same |
| Querying | private: per-query BFS state (a visited stamp array: 4 bytes per node, 4 MB per million nodes); shared: the pages touched | the same, plus a few MB | the same |
| Snapshot on disk (estimate) | 10 to 30 MB | 100 to 300 MB | 300 to 800 MB |

**Budget target (estimate, to be confirmed by B7):** under **500 MB peak** private footprint for a 100k-file first index, and under
**100 MB** steady while idle or querying, on the 16 GB "Low memory" preset (proposal 21.4's 2 GB total leaves room for one language
server and the embeddings).

**Worker count against memory.** Parse workers = `min(cores − 1, memory budget / per-worker peak, 8)`, where the per-worker peak is
measured at start-up by the worker's self-test on a reference file and refreshed from observed peaks (ADR 0006 already caps a worker at
1 GiB). Two Settings with "Auto" defaults (ER-20): **Index threads** and **Index memory budget** (Auto: 500 MB on 16 GB machines or less,
1 GB above, estimate).

**Engine memory and caps.**

| Engine | Default | Cap |
|---|---|---|
| SQLite | page cache about 2 MB per connection (`cache_size` -2000 KiB; docs, not re-read today); `mmap_size` 0 | `PRAGMA cache_size`, `PRAGMA mmap_size` |
| redb, fjall, LMDB | measured peak physical footprint at 1M rows: redb 185 to 191 MiB, fjall 90 to 122 MiB, LMDB 5 to 63 MiB, SQLite 5 to 9 MiB | redb and fjall have cache-size settings in their builders (unverified defaults); LMDB uses the page cache through mmap |
| DuckDB (and DuckLake) | `memory_limit` 80% of RAM, `threads` = cores ([configuration](https://duckdb.org/docs/2.0/configuration/overview.html)) | `SET memory_limit`, `SET threads`, `temp_directory`; the process still grows past the limit (Debrief) |
| Lance | not documented in the pages read (unverified) | index and metadata cache sizes in its dataset options (unverified) |
| tree-sitter workers | one tree per worker | ADR 0006 memory cap, default 1 GiB |

**Trade-offs, stated.** (1) Fewer workers save memory and lose speed linearly (measured 1.7x on 2 threads and 3.5x on 4). (2) A
smaller in-flight budget makes the pipeline stall on large files; 64 MiB is the default, a Setting through the memory budget. (3)
Spilling the link sort to disk costs one more write and read of the edge runs (seconds at Linux scale, estimate). (4) Keeping trees in
the worker for open files makes edits 39x cheaper to re-parse and costs one tree per open file (cap: the files open in the editor, at
most 16, estimate). (5) mmap keeps private memory small but competes for the page cache with Pitwall's model; macOS reclaims clean
pages first, so this is the right side of the trade.

## 7. Recommendation

### 7.1 Per layer

| Layer | Store | Why |
|---|---|---|
| **Catalog and metadata** (manifest, generations, content key to fact blob, quarantine, versions, user state) | **SQLite** (WAL, rusqlite bundled) | family store; multi-process readers for MCP; crash-safe; about 300k rows/s in walk order, 2 to 3 µs look-ups, 5 to 9 MiB footprint; 2 MB default cache |
| **Bulk facts** (per-file parse results) | **content-addressed fact blobs** (postcard, optional zstd) in the same SQLite file, appended in insertion order with a narrow content-key index (section 5.1); **top-2 contender**: append-only segment files (Arrow IPC or postcard packs) per ingest batch with the same index in SQLite | one row per file instead of dozens; shared across worktrees and branches; writes early, swaps only the map; avoids the random-key B-tree cost measured in 5.1 |
| **Graph and search** (refs, callers, in file, path, names, text) | **derived mmap snapshot**: string arena, name FST, symbol and occurrence arrays, CSR forward and reverse; text by grep-searcher first, a document trigram index only if B6 asks | on one query set: callers three levels 5 against 75 µs, unlimited depth 20 against 882 ms, paths 32 against 474 µs, CSR against SQLite with its cache sized to the data; 45.8 against 222 MiB for 5M edges both ways; near-instant re-open; private memory small |
| **Embeddings** | as decided: the `VectorIndex` port with its spike (hannoy, libSQL, usearch, proposal 21.3), vectors cached by chunk text hash and model id (ADR 0001); Lance stays behind LanceDB as the fallback named there | no new decision needed; Lance would bring Arrow and DataFusion for one layer |
| **Analytics** (optional, later) | Parquet export of the snapshot for DuckDB, in a DuckLake catalog if Debrief adopts it | shares Debrief's tooling without DuckDB on the interactive path |

### 7.2 The pipeline

```
open      -> map the last snapshot (usable at once; the refresh runs as a job, ER-17 to ER-19)
change    -> ChangeSource: walk + lstat against the manifest (decided: no Git accelerator, section 10 D1)
read+hash -> only keys still unknown; read into a reused buffer           I/O pool: 4 on SSD, 1-2 on HDD
cache     -> FactKey = H(content key, extractor schema, grammar version and ABI, query pack hash, limits); hit: reuse the blob
parse     -> worker processes, batched requests (up to 64 files or 4 MiB per round trip), one compact fact blob per file
             workers = min(cores - 1, memory budget / per-worker peak, 8), QoS utility / nice 10 (rule 52: bulk work)
write     -> one writer, streaming: INSERT OR IGNORE blobs in batches of ~1,000 (content-addressed, so writing early is safe);
             the files -> fact key map of the new generation is swapped in one commit
link      -> build the snapshot from the generation's blobs: intern, u32 ids, tier-2 edges by name with confidence, tier-3
             (SCIP/LSP) edges overlaid, sort (spill above budget), CSR, FST; write aside, fsync, rename
serve     -> queries read the snapshot (and one blob for detail); SQLite for user state, findings, links
```

**Why each step.**

- **Content-addressed fact blobs** (clangd shards, IntelliJ stubs, Blackbird's blob dedup, which cut 115 TB to 28 TB). A blob
  depends only on the bytes and the extractor's versions, so every worktree, branch, revert and vendored copy shares it. The key
  includes the grammar crate version and ABI, the extractor schema and the pack hash, so a grammar upgrade invalidates exactly the files
  of that language (IntelliJ's rule: bump the stub version). `FileId` and `SymbolId` stay as ADR 0002 defines them; the ids are
  derived when a blob is bound to a file, not stored in it.
- **One row per file.** Stage 1 writes about 20 to 100 rows a file, each through rusqlite's `execute`, which prepares the statement
  again on every call (`Connection::execute` is `self.prepare(sql)?.execute(params)` in rusqlite 0.40.2). One prepared insert of a
  blob replaces them; the measured SQLite insert rate with a prepared statement is 3.2M small rows/s, so the writer stops being the
  bottleneck.
- **Batched IPC** keeps the crash isolation of ADR 0003 and 0006 and pays the round trip once per batch. When a worker dies in a
  batch, the host replays the batch one file at a time to find the culprit and quarantines only it.
- **A derived, mmap'd snapshot for serving** (codesearch, zoekt, Kythe's serving tables). Missing, stale or damaged means rebuild,
  never a user-visible error, so it needs no migrations, only a version in its header, like the vector index (proposal 21.3).
- **Overlay, then compaction** (Glean's stacked databases) only if B4 shows the full link is too slow for a watcher run. The 83 ms
  measured here is a CSR build from an in-memory edge list, not the real link from blobs; B4 measures the real link before anything
  relies on it.
- **Parse once, serve every consumer.** The worker emits symbols, occurrences with a role bitset (as SCIP does), imports, call
  candidates, chunk spans with a text hash, string literals and log-call sites (Debrief bridge). Review rules run **inside the worker on
  the same tree** (the parse is most of a rule's 25 ms) and are cached under the same key plus the rule pack's hash.
- **The fact format resembles SCIP on purpose,** so tier-3 facts (SCIP imports, LSP answers) merge in the same link step, and a SCIP
  export is a writer.

### 7.3 Formats (sketch, for the ADR)

- **Fact blob:** header (schema version, language, `partial`, `has_errors`), file-local string table, arrays of symbols (kind, name,
  container, span as four `u32`), occurrences (name, role bits, span), imports, call candidates (caller, callee name, span), chunks
  (span, text hash). postcard (the family's IPC encoding), optional zstd. Tables `facts(rowid INTEGER PRIMARY KEY, blob BLOB)` (append order) and
  `fact_keys(fact_key BLOB PRIMARY KEY, rowid INTEGER) WITHOUT ROWID`.
- **Snapshot** `<data>/<repo-id>/snapshot-<generation>.pns`: magic, format version, generation, digest of its key set, section table
  with checksums; sections: string arena with offsets, name FST (`fst` crate, MIT or Unlicense; its author indexed 1.6 billion keys in
  20% of their size, [2015](https://burntsushi.net/transducers/)), files, symbols (structure of arrays), occurrences sorted by symbol,
  call edges as CSR forward and reverse with tier and confidence, import graph as CSR. Little-endian fixed-width arrays (`bytemuck` or
  Arrow IPC buffers; the ADR picks one). Written aside, fsynced, renamed; old snapshots deleted when no reader holds them (Windows cannot
  delete a mapped file).
- **Cache sharing (as decided, ADR 0012 D3 and D4).** One fact store per **repository identity** (the root commits plus the `origin`
  URL for a Git folder; its own random identity for any other folder): worktrees and clones of one repository share it, different
  repositories do not. Each folder has its own catalog, which attaches its repository's fact store; snapshots stay per folder. The
  key is a BLAKE3 hash of the raw bytes plus the language, grammar identity, build-derived extractor id and limits.

### 7.4 Ranked options, fallback and risks

1. **Recommended:** SQLite catalog + SQLite fact blobs + mmap snapshot + no text index first.
2. **Contender (decided by B9):** SQLite catalog + Arrow IPC segment files for the facts + the same snapshot.
3. **Fallback:** SQLite only, normalised (interned names, integer ids, edge indexes, prepared statements, streaming commits), with the
   same content-addressed blobs. One file; traversals as BFS over indexed look-ups (75 µs for callers 3, 882 ms for all callers, in the rerun).
4. Not recommended: DuckDB or DuckLake on the interactive path (milliseconds per point query, small-file writes, memory defaults),
   Lance (one more engine for one layer), Delta and Iceberg (built for object-store data lakes), Kùzu (archived), CozoDB (no release
   since 2023), Oxigraph (RDF adds nothing), a Glean-style store over RocksDB (C++ build, second engine), sled (beta, format changes),
   fjall for metadata (no multi-process access).

**Risks.** (1) A second, derived format to keep correct: derive it only from blobs, and test "rebuild from blobs gives the same
snapshot" as a property. (2) Name-based tier-2 resolution makes the link step global: edges are resolved at link time, never stored in
a blob. (3) mmap of a replaced file: never modify in place; write aside and rename. (4) Windows: a mapped file cannot be deleted, so old
snapshots go lazily. (5) Leases and garbage collection across processes (ADR 0012 D6a) must be right, or a reader loses its generation: a two-process test
guards it. (6) Windows needs the directory enumeration in `paddock-walk` for cheap stat values (ADR 0012 D1).

## 8. Parallelism and cost (rule 51, ER-20)

| Stage | Kind of work | Bound (Auto default; each a Setting) | Cheaper operation |
|---|---|---|---|
| change | metadata I/O | 4 threads on SSD, 1 to 2 on HDD | one walk instead of three (after `paddock-walk` unfreezes); the stat comparison; watcher paths |
| read and hash | I/O then CPU | 4 on SSD | skip by stat and by git key; SHA-256 is 526 MiB/s, BLAKE3 1,675 (switch only if B2 shows the read pass hash-bound) |
| parse | CPU, a process each | `min(cores − 1, memory budget / per-worker peak, 8)` | the fact cache; batched requests; incremental re-parse of open files (39x) |
| write | one writer | 1 (SQLite has one writer) | one blob row per file, prepared once, batches of ~1,000 |
| link | CPU and memory | parallel sort (cores − 1), CSR build single-threaded | B4 measures the real link (83 ms was a CSR build from an in-memory list); overlay only if needed; spill above the budget |
| embed | CPU | cores − 1 | cache by chunk text hash and model id (ADR 0001) |
| serve | memory reads | per query | mmap; no work at open |

Priority: bulk index work runs at `QOS_CLASS_UTILITY` / nice 10 / `THREAD_PRIORITY_BELOW_NORMAL` (rule 52), never at a starvable
level, because the user waits for the first index. Results stay deterministic: blobs are content-addressed and the link step sorts.

## 9. Benchmark plan (rule 48)

Every run is recorded first in `apps/pacenotes/docs/performance.md` with the family template, then linked. Corpora: this repository
(medium), a generated tree of 100k files of invented Rust, TypeScript and C with realistic size and call distributions (large), and a
public Linux checkout cloned into the scratch area (huge; public code, not user data). Release builds, `CARGO_BUILD_JOBS=2`, through the
machine's semaphore, load recorded, interleaved A/B runs, **private footprint (macOS `ri_phys_footprint`) and RSS both recorded** for
memory, sampled every 100 ms, peak and steady.

| # | Question | Measure | Decides |
|---|---|---|---|
| B1 | Where do stage 1's 3 ms a file go? | per phase, 1 and 8 workers: read, IPC, parse, extract, apply, SQLite; then batched IPC and `prepare_cached`; add a C corpus | batch size |
| B2 | Hash choice | SHA-256, BLAKE3, xxh3-128 over the read pass of the large tree | keep SHA-256 unless hash-bound |
| B3 | Query latency at scale | normalised SQLite against the snapshot for refs, callers 1 to 5 levels **and unlimited depth** (time and nodes visited), callees unlimited, in file, path A to B (bidirectional BFS on CSR); 1M, 5M, 20M synthetic edges **and the tier-2 graph of the Linux checkout**; p50, p99 | snapshot or the SQLite-only fallback |
| B4 | Link cost | full snapshot build from blobs at 100k, 1M, 10M symbols; time and peak footprint, with and without spill | overlay needed or rebuild per run |
| B5 | Re-open and change detection | open to first answer; a new worktree of an indexed repository; a branch switch of 10% of files; the no-change run on macOS, Linux and Windows (directory-enumeration stat) | the Auto I/O threads and the shared per-repository cache |
| B6 | Text search | grep-searcher against a document trigram index against FTS5 trigram: p50/p99 warm and cold, index size, build time | whether a text index is built |
| B7 | Memory at three sizes | peak and steady private footprint and RSS during first ingest, idle, querying; workers 2, 4, 8; in-flight budgets 32, 64, 128 MiB | the Auto defaults and the 500 MB target |
| B8 | Rules on the worker's tree | rule pack time per file, one parse against one parse per rule | moving rules into the worker |
| **B9** | **Top two for the bulk facts: SQLite append-ordered blobs with a narrow key index, against append-only segment files (Arrow IPC) with the same index** | on medium, large and huge: ingest throughput (files/s) with the key-index insert timed separately, peak footprint and RSS, disk size, re-open time, one-file update cost (watcher run), crash test (kill during write, then open), a branch switch of 10%, and garbage sweep time | the bulk-facts layer; and whether the key index moves to fjall |

**B9 decision rule.** Take SQLite blobs unless Arrow IPC segments are at least 1.5x faster at ingest **and** not worse on one-file
update cost, crash recovery and peak footprint (each within 10%). Ties go to SQLite: one file, one engine, the family's store.

## 10. Decisions (user, 2026-11-04)

The user accepted all nine recommendations on 2026-11-04, with one change to D1. They are recorded as
[ADR 0012](../adr/0012-fast-ingest.md), with amendments to ADRs 0004, 0005, 0006 and 0007.

| # | Decision (user, 2026-11-04) |
|---|---|
| D1 | **Changed from the recommendation.** Change detection uses Pacenotes' own manifest **everywhere**, for consistency: one code path, with no gix blob-id fast path under Git. Size, mtime, ctime and file id are compared, then a content hash is taken when any of them changed. `.gitignore` and the other ignore files are respected through the `ignore` crate. Never `git init`, never a shadow repository. Sections 4.1 and 4.3 above (the `GitAccelerator`) are therefore **not adopted**. The price: a new worktree or a fresh clone reads every file once, because its mtimes are new, but it parses only content the cache has not seen. |
| D2 | Graph queries are served from the derived mmap snapshot (CSR plus FST); SQLite is the only source of truth. |
| D3 | Parse results are stored once per content key (fact blobs, schema v2); B9 decides between SQLite blobs and Arrow IPC segments. |
| D4 | One fact cache is shared by all worktrees and clones **of a repository** (per repository identity); different repositories do not share. |
| D5 | DuckDB and DuckLake stay off the interactive path; they are used only for a later Parquet export. |
| D6 | Text search starts without an index; a trigram index only if B6 asks for one. |
| D7 | Budgets: under 500 MB peak for a 100k-file first index and under 100 MB idle (estimates for B7 to confirm); Index threads, I/O threads and memory budget are Settings with Auto defaults. |
| D8 | Review rules run in the parse worker on its tree. |
| D9 | Submodules and nested repositories are indexed on request. |

## 10a. The questions as they were asked

> Kept as asked, with the first run's numbers. Superseded by section 10, ADR 0012 and the
> [rerun](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint) where they differ.

1. **The user's question: "If code is under Git, only prepare changes; if not, initialise Git to track changes."**
   Recommendation: yes to the first half, no to the second. Under Git, use the git index and trees (read as data through gix, never
   the git binary) to skip reading and parsing anything whose blob was seen before; this needs a small ADR 0004 amendment. Not under
   Git, do **not** run `git init` in the folder (it writes into the user's files, clashes with SVN, Perforce or Mercurial, fails on
   read-only folders) and do not keep a shadow repository either (measured: a second 371 to 398 MB compressed copy of this repository
   and 34 s to create); keep Pacenotes' own manifest (path, size, mtime, ctime, file id, content key) for every folder, which stage 1
   already half has. Agree?
2. **Snapshot or SQLite only?** Accept the derived mmap snapshot for serving, with SQLite the only source of truth, or stay with a
   normalised SQLite and accept slower traversals (87 against 7.9 µs for callers 3; 222 against 23 MiB)?
3. **Fact blobs as rows.** Accept one fact blob per content key in SQLite instead of schema v1's per-row tables (a schema v2 and a
   rebuild, fine before v1)? B9 compares it with Arrow IPC segments first.
4. **Cache across worktrees.** Share the fact-blob cache between all worktrees and clones of a repository on the machine?
5. **DuckLake.** Keep DuckDB and DuckLake off Pacenotes' interactive path and use them only for an optional analytics export once
   Debrief's trial decides? (Recommended.)
6. **Text index.** Start without one (brute-force search) and add a trigram index only if B6 asks for it?
7. **Scale and memory target.** Which repository sets the budgets (the family monorepo, a 100k-file tree, Linux), and is "under 500 MB
   peak for 100k files, under 100 MB idle" the right target?
8. **Rules in the worker.** Run review rules in the parser worker on the tree it already has, accepting that a rule-engine crash then
   costs a worker like a grammar crash does?
9. **Submodules.** Index submodules only on request (default), or always?

## Sources

Read on 2026-11-04 unless dated otherwise. crates.io versions and dates were read from `https://crates.io/api/v1/crates/<name>` the
same day.

- GitHub, "Introducing stack graphs" (2021-12-09, updated 2024-07-23): https://github.blog/open-source/introducing-stack-graphs/
- github/stack-graphs (archived 2025-09-09): https://github.com/github/stack-graphs
- GitHub, "The technology behind GitHub's new code search" (2023-02-06): https://github.blog/engineering/architecture-optimization/the-technology-behind-githubs-new-code-search/
- Russ Cox, "Regular Expression Matching with a Trigram Index" (2012-01): https://swtch.com/~rsc/regexp/regexp4.html
- zoekt design: https://github.com/sourcegraph/zoekt/blob/main/doc/design.md
- SCIP schema: https://github.com/sourcegraph/scip/blob/main/scip.proto; repository: https://github.com/sourcegraph/scip
- Sourcegraph, "SCIP, a better code indexing format than LSIF" (2022-06-08): https://sourcegraph.com/blog/announcing-scip
- Glean introduction: https://glean.software/docs/introduction/; incremental indexing (2022-12-01): https://glean.software/blog/incremental/
- rust-analyzer architecture: https://rust-analyzer.github.io/book/contributing/architecture.html
- Kythe storage model: https://kythe.io/docs/kythe-storage.html
- clangd indexing design: https://clangd.llvm.org/design/indexing
- IntelliJ Platform stub indexes: https://plugins.jetbrains.com/docs/intellij/stub-indexes.html
- tree-sitter code navigation: https://tree-sitter.github.io/tree-sitter/4-code-navigation.html; incremental parsing: https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html
- universal-ctags manual: https://docs.ctags.io/en/latest/man/ctags.1.html
- ripgrep benchmarks (2016-09-23): https://burntsushi.net/ripgrep/
- tantivy: https://github.com/quickwit-oss/tantivy
- SQLite FTS5: https://www.sqlite.org/fts5.html
- git, racy-git: https://git-scm.com/docs/racy-git
- gitoxide: https://github.com/GitoxideLabs/gitoxide; crate status: https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md
- Git LFS pointer specification: https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md
- DuckDB 2.0: `WITH` and `USING KEY` https://duckdb.org/docs/2.0/sql/query_syntax/with.html; concurrency https://duckdb.org/docs/2.0/connect/concurrency.html; configuration https://duckdb.org/docs/2.0/configuration/overview.html
- DuckLake: https://ducklake.select/; compaction: https://ducklake.select/docs/stable/duckdb/maintenance/merge_adjacent_files
- Lance: https://github.com/lancedb/lance
- iceberg-rust: https://github.com/apache/iceberg-rust
- redb: https://github.com/cberner/redb; design: https://github.com/cberner/redb/blob/master/docs/design.md
- fjall: https://github.com/fjall-rs/fjall
- sled: https://github.com/spacejam/sled
- Kùzu (archived 2025-10-10): https://github.com/kuzudb/kuzu
- CozoDB: https://github.com/cozodb/cozo
- fst: https://github.com/BurntSushi/fst; "Index 1,600,000,000 keys with automata and Rust" (2015-11-11): https://burntsushi.net/transducers/
- Phoronix, Linux 7.2 line count (secondary, unverified): https://www.phoronix.com/news/Linux-7.2-43-Million-Lines
- tree-sitter throughput, third-party (unverified): https://docs.rs/crate/weavatrix-parse/0.1.0/source/docs/comparison.md
- Family records: `apps/pacenotes/docs/performance.md`; `apps/debrief/docs/research/2026-10-29-duckdb-write-paths.md` and
  `2026-10-28-duckdb-server-mode.md` (origin/main)
