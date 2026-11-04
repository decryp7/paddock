# 12. Fast ingest: the manifest, the per-repository fact cache, the SQLite catalog and the derived mmap graph

Status: accepted (user, 2026-11-04). Proposed the same day in
[research/2026-11-04-fast-ingest.md](../research/2026-11-04-fast-ingest.md). The user accepted its recommendations D1 to D9 with one
change to D1: change detection uses Pacenotes' own manifest everywhere, with no Git fast path. The fact cache is shared by the
worktrees and clones of **one repository** (the user's D4). Revised after an independent review the same day. Amends ADRs 0004,
0005, 0006 and 0007; each carries an amendment section that points here.

## Context

Stage 1 (ADRs 0005 and 0006) indexes correctly but is slow per file and holds a whole run in memory.

- Its first run took about 3 ms a file, while tree-sitter with the tags query needs about 0.8 ms (research note section 1).
- A run keeps every file's facts in one `Batch` until a single commit, so memory grows with the repository.
- Facts belong to a `FileId`, so a new worktree, a branch switch or a revert parses everything again.
- "Find references" and "callers" scan an unindexed `calls` table.
- The rules engine parses each file once per rule.

The research note surveys how code indexers stay fast (stack graphs, Blackbird, zoekt, SCIP, Glean, clangd, IntelliJ stubs). It
measures parse, hash, graph, metadata-store and change-detection costs, recorded in
[performance.md, 2026-11-04](../performance.md#2026-11-04-fast-ingest-research-parse-hash-and-graph-micro-benchmarks), and compares
the stores. This ADR fixes the design that follows from it.

## Decision

### D1. Change detection: Pacenotes' own manifest, one code path for every folder

**One mechanism, Git or not.** Every opened folder has a **manifest** in its catalog (D4), never in the folder.

- **What a row holds**, per path:
  - the size;
  - the modification time and the change time, in nanoseconds;
  - the file id;
  - the raw content hash (D3).
- **How a run uses it.** A run walks the folder and compares each file's four stat values with its row. Equal values mean the file is
  unchanged and is not read. If any value differs, the file is read and its raw content hash taken; only a new hash goes on to the
  fact cache.
- **Where the values come from:**

| | macOS and Linux | Windows |
|---|---|---|
| Size | `st_size` | `EndOfFile` |
| Modification time | `st_mtime` (ns) | `LastWriteTime` |
| Change time | `st_ctime` (ns) | `ChangeTime` |
| File id | `st_ino` with `st_dev` | the 128-bit `FileId` (`FILE_ID_128`) with the volume serial number |

- **Windows reads everything from the directory enumeration**, never one handle per file. Each directory is opened once and read with
  `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` (or `NtQueryDirectoryFile`). That returns the 128-bit file id,
  `ChangeTime`, `LastWriteTime`, `EndOfFile` and the attributes; reparse points are skipped, which is the link refusal. The volume
  serial is read once per volume. Rust's `std` has no change time on Windows (its `MetadataExt` offers creation time and last write
  time), and Windows has no `ctime` in the Unix sense, so the NTFS/ReFS `ChangeTime` (the metadata change time) is used in its place.
  This is a **requirement on `paddock-walk`** (today `paddock-repo-files`): its enumeration yields these values on every platform
  instead of a bare path list. The change lands in the shared walk (rule 42), not in a Pacenotes copy.
- **The racy rule of ADR 0006 stays.** A file whose modification time is not at least 2 s older than when it was read is checked by
  content. Stat values are compared for equality, never as "newer than", so a skewed clock causes a re-hash rather than a skip.
- **Volumes with unreliable times are detected.** "Always check content" is turned on by default for:
  - remote volumes: macOS `statfs` type `smbfs`, `nfs`, `afpfs` or `webdav`; Windows `GetDriveType` `DRIVE_REMOTE`; Linux network
    magic numbers;
  - FAT-family volumes (FAT, FAT32, exFAT; 2 s modification times).

  On those volumes every file whose stat is not identical is hashed, and so is every file whose modification time falls within the
  racy window widened by the skew observed between the volume and the local clock. Users can set the option per repository.
- **Limits, stated rather than hidden.** A tool that rewrites a file while restoring its size, modification time, change time and file
  id defeats the check. On Unix the change time cannot be set by user tools, so this needs a file system that does not keep it.
  Settings > Index > "Re-index everything" is the answer.
- **Case and normalisation.** On case-insensitive volumes (the defaults on macOS and Windows), each row also holds `path_key`, the
  path case-folded and, on macOS, NFC-normalised. Look-ups and uniqueness use `path_key`, so a rename that only changes case is a
  rename, not a delete and an add. On case-sensitive volumes `path_key` equals the path.
- **Ignore files.** `.gitignore`, `.ignore`, nested files and negations are respected through the `ignore` crate, which the shared walk
  uses (ADR 0006). The denylist is checked first and still wins.
- **Git is not used for change detection** (the user's decision). Pacenotes:
  - does not read the git index;
  - does not run `git status` or `git diff`;
  - never runs `git init` in a folder;
  - never keeps a shadow repository.

  The reasons, measured in the note: one code path for all folders; read-only `git status` on a stale index re-hashes every file on
  every call (5.9 s for 27k files); the git binary runs configured programs (ADR 0004); a shadow repository is a second compressed copy
  of the code (371 to 398 MB for this repository, 34 s to create). `pacenotes-git` reads commits only for the repository identity
  (D4) and for history (ADR 0004).
- **The cost of one path.** A new worktree or a fresh clone has new modification times, so it reads and hashes every file once. It
  parses only content the repository's cache has not seen (D3).
- **The interface.** A `ChangeSource` port yields a `ChangeSet`: the entries sorted by `path_key`. Each entry has:
  - its stat;
  - its raw content hash, or `Unknown` (read and hash);
  - its change: unchanged, added, modified, deleted, or renamed from a path.

  Renames are matched by `Fingerprint` (ADR 0005), computed only when needed (D3).

### D2. The pipeline

```
open      -> map the current snapshot (D5); the app answers at once, the refresh runs as a job (ER-17 to ER-19)
change    -> one walk (ignore files, denylist, bounds) + stat compare against the manifest                  I/O pool (Auto, D7)
read+hash -> only files whose stat changed: raw content hash (D3); Fingerprint only for rename candidates  I/O pool
cache     -> FactKey (D3); hit: reuse the fact blob; miss: parse
parse     -> worker processes (ADR 0006), requests of up to 64 files or 4 MiB, one fact blob per file;
             the review rules run on the same tree (D9)
write     -> one writer (D4), streaming: blobs in transactions bounded to about 50 ms of work;
             the new generation's path -> fact key map is swapped in one commit
link      -> build a new snapshot (D5) from the generation's blobs; write it under a fresh name, fsync, rename
serve     -> queries read the snapshot, plus one blob for detail; SQLite holds user state, findings and links
```

The channels between stages are **bounded by bytes in flight**, not by a file count. The default is 64 MiB (an estimate, to be
confirmed by B7). It counts every file read and not yet parsed, the batched worker requests, and the replies not yet written. A full
channel blocks the stage before it (backpressure). Nothing collects the whole run: stage 1's `Batch` goes away.

### D3. The fact cache: keys, versions and blobs

**What a fact blob holds.** One file's parse result. It depends only on the file's raw bytes and on what extracted them. It contains:

- a header: magic, blob schema version, language, `partial`, `has_errors`, payload length and an xxh3-64 checksum of the payload;
- a file-local string table;
- symbols (kind, name, container, span);
- occurrences (name, role bits for definition, reference, import, read, write and test, span), as SCIP models them;
- imports;
- call candidates (caller, callee name, span);
- chunks (span, text hash, for the embedding cache of ADR 0001);
- string literals and log-call sites, for the Debrief bridge.

It is encoded with postcard, optionally compressed with zstd. `FileId` and `SymbolId` (ADR 0002) are not stored in the blob; they are
derived when the blob is bound to a path.

**The key.**

```
FactKey = BLAKE3( raw content hash (BLAKE3 of the file's bytes as read, no normalisation)
                ‖ ParseLanguage
                ‖ grammar identity (grammar crate name, version, ABI version)
                ‖ EXTRACTOR_BUILD_ID
                ‖ limits (max bytes, max nodes) )
```

Rule results are cached beside the blob under `FactKey ‖ rule pack hash` (D9).

- **Raw bytes, not normalised.** The parser reads the raw bytes, and every span refers to them. Two files that differ only in line
  endings are different content and get different blobs, so a span is never applied to the wrong bytes.
  - `Fingerprint` (SHA-256 of the line-ending-normalised text, ADR 0002) stays for **rename detection only**. It is computed lazily,
    for added files only when there are unmatched deleted files to pair them with (ADR 0005).
  - A test parses an LF, a CRLF and a lone-`\r` copy of the same source and checks that each one's spans slice its own bytes to the
    expected names.
- **The language is in the key**, so a `.h` file parsed as C and as C++ (a per-repository Setting can change that), or a file whose
  detected language changes, never shares a blob.
- **The grammar identity is in the key**, so a grammar upgrade invalidates exactly the files of its language (IntelliJ's stub-version
  rule).
- **`EXTRACTOR_BUILD_ID` is derived at build time.** A `build.rs` in `pacenotes-parse` hashes:
  - the crate's sources, its spec tables and every query file;
  - the versions and ABI numbers of the tree-sitter runtime and of each grammar crate, read from the workspace `Cargo.lock`;
  - the blob schema version.

  No one has to remember to bump a version.
- **A golden-corpus test** checks the build id against what the extractor actually produces:
  - a fixed set of source files is extracted and each blob's payload hash is compared with the recorded one;
  - the test fails when an output changes while `EXTRACTOR_BUILD_ID` stays the same (a behaviour change that the build hash did not
    see, for example in a dependency);
  - it also fails when the build id changes without a recorded reason (the expected-file update says why).
- **Separate cache folders for dev and test builds** (ER-14). A debug build, a test and an `ingest-bench` run use their own data
  folder: tests use a temporary folder, and debug builds use `<data>-dev`. They never write into the cache of the installed app, and
  the installed app never reads theirs.
- **A damaged blob is a miss.** A blob whose header checksum or postcard decode fails is treated as a miss: it is deleted, the file is
  parsed again, and the event is logged (D11) with the key and the reason.

**Where the blobs live** is decided by benchmark B9, behind a `FactStore` port that IG2 introduces, so the decision changes one adapter:

- **Default:** SQLite in the repository's fact store (D4). Blobs go in `facts(rowid INTEGER PRIMARY KEY, blob)`, appended in insertion
  order, with a narrow index `fact_keys(fact_key BLOB PRIMARY KEY, rowid) WITHOUT ROWID`. The blobs are not keyed by the hash itself,
  because uniform random 32-byte keys load at 18k to 29k rows/s in every B-tree store, against
  250k to 440k for keys in walk order ([measured](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint)).
- **Contender:** append-only segment files (Arrow IPC) per ingest batch, with the same index in SQLite.
- **B9's rule:** take SQLite unless segments are at least 1.5x faster at ingest and not worse, each within 10%, on one-file update cost,
  crash recovery and peak footprint. If the narrow index is still the bottleneck at Linux scale, it moves to fjall, owned by the writer
  process alone.

### D4. Files, repository identity, writers and the SQLite catalog

**Layout** in the app's data folder (`paddock-env`), never in a folder being indexed:

```
<data>/repos/<repository-id>/facts.sqlite              fact store: fact_keys, facts, rule results, run leases
<data>/folders/<folder-id>/catalog.sqlite              catalog: meta, manifest, generations, generation_files, leases,
                                                       quarantine, findings, suppressions, links, coverage runs
<data>/folders/<folder-id>/snapshot-<generation>-<nonce>.pns    the snapshots (D5)
```

**Catalog and fact store.**

- The catalog `ATTACH`es its repository's fact store, so one connection sees both.
- SQLite runs in WAL mode (rusqlite, bundled; the family's store, used by Dyno, Pitwall, Spotter and Pacenotes).
- The catalog is the only source of truth for its folder; the fact store and the snapshots are derived and can be rebuilt.
- `<folder-id>` is a hash of the folder's canonical path.

**Repository identity** decides which folders share a fact store.

- **A Git folder:** BLAKE3 of the sorted root commit ids, plus the normalised URL of the `origin` remote, or "no remote".
  - The root commits are the parentless commits reachable from `HEAD`. They are read through gix as data (ADR 0004 allows reading
    commits), computed once and stored in the catalog, and re-checked only when `HEAD` moves to a commit whose ancestry the stored
    roots do not cover.
  - So all worktrees and clones of a repository with the same origin share one fact store. A fork with another origin gets its own,
    and so does an unrelated repository.
  - A history too deep to walk within the walk budget of ADR 0004 falls back to a non-Git identity, with a log line.
- **A folder that is not under Git**, or whose repository cannot be read, gets **its own identity**: a random id created on first open
  and stored in its catalog. It shares its fact store with no one.

**One writer per file.**

- Each SQLite file has one writing process at a time: the process that holds its owner lock, `<file>.owner` (an exclusive OS lock:
  `flock` or `LockFileEx`). The lock file records the owner's `paddock_proc::ProcessId` (pid and start time, rule 53).
- Inside that process one writer thread does all writes (ADR 0005's single writer).
- Any number of processes read through WAL: the app, the MCP server (`pacenotes serve --stdio`) and the CLI.
- **When a second writer appears** (the CLI's `pacenotes index` while the app indexes the same folder, or two folders of one repository
  indexed by two processes):
  - it does not write;
  - an interactive caller shows "Waiting for Pacenotes (pid …) to finish writing" with Cancel, and waits on the lock;
  - the CLI refuses with exit code 2 and that message, unless it is given `--wait`;
  - neither times out (ER-19).

  First-release plan package H1 already asks for this behaviour. A lock whose recorded process no longer exists (pid gone, or a new
  start time) is stale and is taken over, with a log line.
- **`SQLITE_BUSY` handling:**
  - Writers seldom see it, because the OS lock already serialises them. When they do (a checkpoint against a long reader), the busy
    handler retries with backoff up to 1 s, then gives up that transaction and retries the batch, and logs it.
  - Readers that hit it (WAL recovery after a crash) retry with backoff while their query's cancel token is live.
  - No path turns `SQLITE_BUSY` into a user-visible error without a Retry.

**Schema v2.** The catalog holds:

- `meta` (schema version, generation, repository identity, current snapshot name);
- the manifest (D1);
- `generations` and `generation_files(generation, path, file_id, fact_key)`, the map that a commit swaps;
- `leases` (D6a);
- quarantine;
- findings, suppressions, requirements links and coverage runs (proposal 14).

The fact store holds `fact_keys`, `facts`, rule results and `run_leases`, and uses `auto_vacuum = INCREMENTAL`, so a sweep can return
space with `PRAGMA incremental_vacuum` in bounded steps.

Schema v1's per-row tables (`symbols`, `imports`, `calls`, `chunks`) go away; their content lives in the blobs and the snapshot. A
schema v1 file is rebuilt under ADR 0005's rule, with no migration before v1.

**Why SQLite.** It is the only candidate that combines:

- readers in other processes while one process writes (the MCP server reads while the app indexes); fjall cannot be opened by two
  processes, and DuckDB allows several processes only read-only;
- crash safety;
- being the family's store already (ER-22; Paddock has no shared key-value crate).

Its point lookups (about 2.6 µs) and path-ordered loads (hundreds of thousands of rows a second) are fast enough for the manifest.
Its memory, as physical footprint (rule 26), was 5 to 9 MiB at a million rows, against 5 to 63 MiB for LMDB, 90 to 122 MiB
for fjall and 185 to 191 MiB for redb ([rerun](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint)).

### D5. The derived mmap graph: the snapshot

**Contents.** Each generation gets one read-only file, `snapshot-<generation>-<nonce>.pns`, with:

- a header: magic, format version, generation, the digest of its key set, a section table and a header checksum;
- a string arena with offsets;
- a name FST (the `fst` crate, MIT or Unlicense) for exact, prefix and fuzzy name search;
- files, each with its symbol range;
- symbols as a structure of arrays;
- occurrences sorted by symbol;
- call edges as CSR, forward and reverse, with tier and confidence per edge;
- the import graph as CSR.

The arrays are little-endian and fixed-width; IG4 picks `bytemuck` casts or Arrow IPC buffers.

**How it is built.** From the generation's blobs:

- intern the strings and assign `u32` ids;
- resolve tier-2 edges by name, with a confidence label (IG11);
- overlay tier-3 edges (SCIP, LSP);
- sort.

Edges are resolved here, never stored in a blob, because name resolution is global. The interner and the sort both spill to disk
above their share of the memory budget (D7).

**Names and atomicity.**

- The `<nonce>` is random and a name is never reused. The catalog's `meta` holds the current snapshot's name.
- A new snapshot is written under its fresh name, fsynced and renamed into place; on Unix the directory is fsynced after the rename.
  Then the catalog's pointer is updated in one transaction.
- On Windows a rename that meets a sharing violation (an antivirus or indexer holding the file) is retried with backoff, as long as the
  job is not cancelled, and logged.
- At open, only the header checksum and the file length are checked, so opening stays instant. The section checksums are verified
  after each build and by "Verify index".
- An old snapshot is deleted only when no reader holds it (D6a). On Windows that is lazy, because a mapped file cannot be deleted.

**Why.** The measurements are in the
[rerun](../performance.md#2026-11-04-fast-ingest-rerun-after-review-one-query-set-physical-footprint): the same query sets on both sides,
both directions, time per node reached, SQLite's page cache sized to the data, and a cold mapped CSR case. In short:
on 1M nodes and 5M edges, CSR against SQLite with its cache sized to the data:

| Query | CSR | SQLite | Per node reached |
|---|---|---|---|
| callers, three levels (200 queries) | 5 µs | 75 µs | 0.023 against 0.373 µs, 16x |
| callers at unlimited depth (10 queries, about 500,000 nodes) | 20 ms | 882 ms (a recursive CTE: 1,323 ms) | 0.041 against 1.78 µs, 43x |
| path A to B by bidirectional BFS (200 pairs, about 1,300 nodes visited) | 32 µs | 474 µs | 15x |

- A cold mapped file costs about 0.5 to 0.8 ms for the first small queries (page faults), then matches memory.
- Both CSR directions take 45.8 MiB against a 222 MiB SQLite file.
- Mapping and querying the file added about 1 MiB of physical footprint, because the mapped pages are clean.

The CSR build time measured there is a build **from an in-memory edge list**. The real link (blobs to snapshot) is what B4 measures,
and nothing here assumes its cost yet.

**Depth is unlimited by design.** Callers, callees and paths are a BFS with a visited set (a stamp array, 4 bytes a node). Each node is
touched at most once, cycles end the walk, and the cost is bounded by the reachable part of the graph. A path between two symbols is a
bidirectional BFS that expands the smaller frontier.

- **UI:** levels are shown progressively ("Show next level", with counts of what is hidden). "All callers" or "All callees" is an
  explicit action that runs as a cancellable query with a live count, because its answer can be most of the repository.
- **MCP:** tools take a depth (default a few levels) and a result limit, and report truncation.

**Derived data**, like the vector index (proposal 21.3). A missing, stale, damaged or other-version snapshot is rebuilt, never shown as
an error. A property test checks that rebuilding from the blobs gives a byte-identical snapshot.

**Updates.** Whether a watcher run rebuilds the whole snapshot or merges a delta overlay is decided by B4. A delta overlay holds the
files hidden from the base plus their new facts, as Glean stacks databases. It is merged at query time and compacted in the
background.

**Queries** are pure functions over the snapshot's arrays behind the `Store` read port (ADR 0002), each with a cancel token and a
result limit:

- find references;
- callers and callees to any depth;
- symbols in a file;
- path between two symbols;
- name search.

The consumers of schema v1 move to them (IG12): search, `symbols_in_file`, the MCP tools and the UI.

### D6. Text search without an index first

- Literal and regex search runs over the walked files with the ripgrep crates (`grep-searcher`, `grep-regex`; MIT or Unlicense), with
  the denylist, limits and cancellation.
- No text index is built at ingest.
- A document trigram index (the codesearch design, about 20% of the source, mmap'd) is added only if benchmark B6 shows the need.
- BM25 over chunks for hybrid search stays as ADR 0001 and proposal 21.3 have it.

### D6a. Leases and garbage collection

**Generation leases.** A reader that maps a snapshot holds a **shared OS lock** on that snapshot file (`flock(LOCK_SH)` or `LockFileEx`
shared) for as long as it is mapped. A reader in another process that only reads the catalog for a generation (the MCP server
answering from blobs) writes a **lease row**: generation, `ProcessId` and a purpose.

**Run leases.** An indexing run writes a run lease to the fact store before its first blob: run id and `ProcessId`. Its blobs are
tagged with the run id until the generation swap clears the tag. Blobs from a run that crashed or was cancelled therefore stay
protected only while their lease is live.

**Stale leases.** A lease or lock whose process no longer exists, or exists with another start time, is stale (rule 53: identity
checked right before acting). The sweep removes it and logs it.

**Mark and sweep** runs as a job after a generation swap and on "Clean up the index":

- It works in chunks of candidate blobs. Each chunk is **one SQLite write transaction** that deletes only blobs that, inside that same
  transaction, are named by:
  - no current generation of any folder of the repository (their catalogs record their current generation in the fact store's
    `generation_refs` table at each swap);
  - no leased generation;
  - no live run lease.
- Because the check and the delete are in one transaction, a run that names a blob concurrently either commits first (and the blob
  stays) or waits.
- Snapshots are deleted when an exclusive non-blocking lock on them succeeds (no reader) and they are not current.
- The sweep then calls `PRAGMA incremental_vacuum(N)` in bounded steps.

**A two-process test** is part of IG13: process A maps generation N while process B indexes, swaps to N+1 and sweeps. A keeps answering
from N, and N's blobs and snapshot survive until A exits. Killing B mid-run leaves its blobs protected until the lease turns stale, and
the next sweep then removes them.

### D7. Memory: budgets and Auto settings

**What the budget covers.** The engine process (walk, readers, writer, link, queries) and its parse workers. It does not cover language
servers (their own caps, proposal 21.4), Pitwall, or the webview.

**Mechanisms:**

- one tree per worker, dropped after extraction;
- channels bounded by bytes (D2);
- blobs written as they arrive;
- interned strings and `u32` ids, with the interner and the arena spilling to disk above their share;
- the snapshot mmap'd, not on the heap;
- the link sort spilling to disk;
- no pointer graph in memory.

**Targets (estimates, to be confirmed by B7):** under 500 MB peak physical footprint for a first index of 100k files, and under 100 MB
steady while idle or querying, on the 16 GB "Low memory" preset (proposal 21.4).

**Settings**, each with an "Auto" default (ER-20):

- **Index memory budget.** Auto: 500 MB on machines with 16 GB or less, 1 GB above (estimate). It splits into:
  - the worker share;
  - the bytes in flight (64 MiB by default, an estimate);
  - the link share (interner, arena and sort buffer, which spill beyond it).
- **Index threads** (parse workers). Auto: `min(cores − 1, worker share / per-worker peak, 8)`.
  - The per-worker peak is measured by the worker's self-test at start and updated from the peak each worker reports after every batch.
  - **Enforcement:** when the observed footprints of the host plus its workers exceed the budget, the pool shrinks by one worker: that
    worker finishes its batch and is not given another. The pool does not grow again until the total stays under 80% of the budget.
  - **The per-worker cap** (ADR 0006's process memory cap) becomes `min(1 GiB, worker share / 2)`, so one worker always fits inside the
    budget. A file that needs more is quarantined, as a crash is.
- **Index I/O threads.** Auto by volume:
  - an SSD gets 4;
  - a rotational disk gets 1 to 2;
  - a network volume gets 2.

  The detection: macOS reads the IOKit "Medium Type" (solid state) of the volume's device; Windows uses `IOCTL_STORAGE_QUERY_PROPERTY`
  with `StorageDeviceSeekPenaltyProperty`; Linux reads `/sys/block/<device>/queue/rotational`. Unknown means SSD.

**The trade-off is stated in the UI copy.** Fewer workers or a smaller budget use less memory and index more slowly; the measured
scaling was 1.7x on 2 threads and 3.5x on 4.

**Engines are capped.** SQLite keeps its default page cache (about 2 MB per connection). The snapshot's pages are clean, file-backed,
and not in the process's footprint. DuckDB, where used (D8), runs with an explicit `memory_limit` and `threads`; its defaults are 80% of
RAM and every core.

### D8. DuckDB and DuckLake are for export only

The interactive path (UI, MCP, review) never runs on DuckDB or DuckLake: their point queries take milliseconds and their small writes
make small files. An optional, later **Parquet export** of the snapshot and the catalog serves analytics (hot spots over history,
coverage joins). It writes into a DuckLake catalog if Debrief's queued DuckLake trial adopts one, so the two apps share conventions and
tooling. A shared crate waits for the rule of three (ER-22).

### D9. Review rules run in the parse worker

- **On the parser's tree.** The worker runs the structural rules of ADR 0007 on the tree it has already parsed, so a rule costs its
  match and not a parse.
- **Why it matters.** At the measured parse rate (14.8 MiB/s), parsing the 160 KB rule benchmark file takes about 11 ms, about 44% of
  a typical rule's 25 ms. A pack of N rules therefore saves N − 1 parses per file. B8 measures the real saving.
- **Cache.** Rule results are cached under `FactKey ‖ rule pack hash`.
- **A crash or a stall in the rule engine**:
  - the host replays that file **without rules** in a fresh worker and keeps its facts (they are stored as usual);
  - it quarantines only the pair (file content, rule pack), with the reason;
  - the file keeps its symbols and graph edges;
  - it gets no rule results until its content or the pack changes;
  - the report names the file, the pack and the reason.

  A crash in the grammar is handled as in ADR 0006, which quarantines the file.

### D10. Submodules and nested repositories are indexed on request

- **The boundary.** A folder below the root that contains a `.git` entry (a folder or a gitfile) is a **nested repository boundary**.
- **By default** it is listed with its file count and an "Index submodules" action (a per-repository Setting, off by default), and it
  is not walked into until the user turns it on.
- **Once indexed**, it is a folder of its own: its own catalog, manifest and generation, and its own repository identity, so it shares
  facts with other clones of that submodule. Its facts join the parent's snapshot.
- **Detection** uses the walk alone.

### D11. Logging (ER-27)

Every run logs enough, with user content left out (paths only where the denylist allows them, never file text), for a diagnostic
bundle to show the root cause. All lines carry a **run id** (the correlation id) and the folder and repository ids.

- **Phases:** each phase's start, end, duration and counts:
  - files walked, unchanged, read, hashed;
  - cache hits and misses;
  - parsed, quarantined;
  - blobs written;
  - edges linked.
- **The cache:** hits, misses and the reason for each miss class: new content, new grammar identity, new `EXTRACTOR_BUILD_ID`, a damaged
  blob (with the key and whether the checksum or the decode failed).
- **Workers:**
  - start (pid and start time) and exit (code or signal);
  - quarantine with the cause: a crash, a stall (the judge's counts), the memory cap, or a rule crash with its pack;
  - the batch replay that found a culprit;
  - pool shrink and grow events with the footprints that caused them.
- **The writer:**
  - transactions with duration and row counts;
  - `SQLITE_BUSY` retries;
  - owner-lock waits and takeovers of stale locks.
- **Snapshots:**
  - each rebuild with its reason (new generation, missing, damaged header, other format version) and its duration, size and spill
    volume;
  - rename retries;
  - deletions.
- **Sweeps:** the leases found stale, and the blobs and snapshots removed, with the bytes returned.
- **Errors** carry their cause chain and the next step shown to the user.

## Parallelism and cost (rule 51)

| Stage | Kind of work | Bound (Auto default; each a Setting) | Cheaper operation |
|---|---|---|---|
| change | metadata I/O | Index I/O threads (D7) | one walk instead of three (after `paddock-walk` gains cancel, progress and the stat fields, ADR 0006's TODO and D1); the stat comparison; paths from the watcher when `paddock-watch` carries them |
| read and hash | I/O, then CPU | Index I/O threads | only files whose stat changed; BLAKE3 for the raw hash (1,675 MiB/s on one core, measured); `Fingerprint` only for rename candidates |
| parse and rules | CPU, one process per worker | `min(cores − 1, worker share / per-worker peak, 8)` | the fact cache; batched requests; rules on the same tree; incremental re-parse of files open in an editor (39x measured), later |
| write | one writer thread | 1 (SQLite has one writer) | one blob row per file, statements prepared once, transactions of about 50 ms of work |
| link | CPU and memory | parallel sort (cores − 1), single-threaded CSR build | overlay only if B4 asks; spill above the link share |
| embed | CPU | cores − 1 | cache by chunk text hash and model id (ADR 0001) |
| serve | memory reads | per query | mmap; no work at open |

**Priority (rule 52).** Parse workers, readers and the link step are bulk work: `QOS_CLASS_UTILITY`, nice 10 or
`THREAD_PRIORITY_BELOW_NORMAL`. They never run at a level that can be starved, because the user waits for the first index.

**The writer thread holds the SQLite write lock that live work waits on** (a suppression, a finding's triage, a link the user confirms,
all written through the same writer). So it runs at the normal level. It keeps each transaction to about 50 ms of work: it commits
after the rows that fit, so a live write waits at most about one transaction. Live writes are queued ahead of bulk ones.

Results are deterministic: blobs are content-addressed, the writer applies in path order, and the link step sorts.

## Work packages

Sizes are agent-days, estimated from this design (the family's rule: a firm estimate once an ADR has work packages). The benchmark
packages come before the choices they decide (B9 before IG2's adapter is final, B4 before IG4's update mode, B7 before IG8's
defaults). Every benchmark result is recorded in `docs/performance.md` before anything quotes it (rule 48), with physical footprint
for memory (rule 26) and the raw output linked.

| WP | What | Depends on | Size |
|---|---|---|---|
| IG0 | Corpus generator: N files of invented Rust, TypeScript and C with realistic size and call distributions (medium 10k, large 100k); the scripted clone of a public Linux checkout into the scratch area; a footprint sampler (every 100 ms, macOS `ri_phys_footprint`, Linux PSS, Windows private working set) | | 2 |
| IG-B1 | B1: per-phase cost of stage 1 with 1 and 8 workers (read, IPC, parse, extract, apply, SQLite), then batched IPC and prepared statements; a C corpus. B2: SHA-256, BLAKE3 and xxh3-128 over the read pass; first switch `sha2` to its hardware path (0.10.9 uses the software rounds on aarch64 unless the `asm` feature is on; 0.11 detects SHA-2 instructions at run time) | IG0 | 2 |
| IG-B3 | B3 and B4: snapshot against normalised SQLite for the four queries at 1M, 5M and 20M synthetic edges and on the tier-2 graph of the Linux checkout (p50, p99, time per node reached), including callers and callees at unlimited depth and path A to B; the real link cost (blobs to snapshot) at 100k, 1M and 10M symbols, with and without spill | IG0, IG4 | 2.5 |
| IG-B5 | B5: re-open (open to first answer), a new worktree of an indexed repository, a branch switch of 10% of the files, and the no-change run, on macOS, Linux **and Windows** (directory-enumeration stat, D1) | IG1, IG2, IG14 | 1.5 |
| IG-B6 | B6: brute-force search against a document trigram index against FTS5 trigram (warm and cold p50 and p99, size, build time) | IG0 | 1.5 |
| IG-B7 | B7: physical footprint at three sizes during first ingest, idle and querying; workers 2, 4 and 8; in-flight budgets 32, 64 and 128 MiB; the shrink rule exercised | IG2, IG3, IG4 | 2 |
| IG-B8 | B8: a rule pack's time per file with one shared parse against one parse per rule | IG7 | 1 |
| IG-B9 | B9: the two `FactStore` adapters (SQLite append-ordered blobs against Arrow IPC segments): ingest with the key-index insert timed separately, footprint, disk, re-open, one-file update, crash test, branch switch, sweep; the decision recorded as an amendment here | IG2 | 3 |
| IG1 | Manifest and `ChangeSource` (D1): the stat values on all three platforms, `path_key`, the racy rule, volume detection and "Always check content", renames by lazy `Fingerprint`; replaces the stamp shortcut of ADR 0006 | IG14 (Windows part) | 3 |
| IG2 | Fact cache and schema v2 (D3, D4): `FactStore` port and the SQLite adapter, blob format with header checksum, `FactKey`, the `EXTRACTOR_BUILD_ID` build script and golden-corpus test, the raw-bytes span test, the file layout, repository identity, owner locks and `SQLITE_BUSY` handling, dev and test data folders, the streaming writer with byte-bounded channels and 50 ms transactions, the generation swap, the `paddock-storage` category | IG-B1 | 8 |
| IG3 | Batched worker protocol (D2): up to 64 files or 4 MiB a request, the replay that finds and quarantines a culprit, per-worker peak reports | | 3 |
| IG4 | Snapshot builder and reader (D5): arena, FST, structure of arrays, CSR both ways, spilling interner and sort, header checksum and section checksums, fresh names, rename with retry and directory fsync, the rebuild-is-identical property test | IG2 | 7 |
| IG5 | Queries over the snapshot behind the `Store` port: references, callers and callees to any depth (progressive levels, "all" as an explicit cancellable query with a live count), symbols in a file, path by bidirectional BFS, name search (exact, prefix, fuzzy), each with cancel and limits | IG4 | 4 |
| IG6 | Text search without an index (D6): grep-searcher over the walked set, denylist, limits, cancel, the MCP tool | IG1 | 2 |
| IG7 | Rules in the worker (D9): `find_all` on the worker's tree, results cached by `FactKey ‖ pack hash`, the rule-crash replay without rules and the (file, pack) quarantine | IG2, IG3 | 3 |
| IG8 | Memory budget and Auto settings (D7): the three Settings, the budget split, footprint-driven pool shrink and grow, the per-worker cap, I/O-thread detection by volume, UI copy for the trade-off | IG3, IG-B7 | 3 |
| IG9 | Nested repository boundaries (D10): detection in the walk, the listing, the per-repository Setting, the separate catalog, the join into the parent's snapshot | IG1, IG4 | 2 |
| IG10 | Parquet export (D8), after Debrief's DuckLake trial decides | IG4 | 2.5, later |
| IG11 | Tier-2 name resolution in the link step: imports and scopes per language, candidate edges by name with a confidence label ("certain", "likely", "possible"), and tier-3 overlay | IG4 | 4 |
| IG12 | Moving schema v1's consumers to the snapshot: search, `symbols_in_file`, the MCP tools and the UI's queries; removing the v1 tables and their tests | IG5 | 3 |
| IG13 | Leases and garbage collection (D6a): snapshot locks, lease and run-lease rows, stale-lease removal by `ProcessId`, the chunked transactional sweep, incremental vacuum, and the two-process test | IG2, IG4 | 4 |
| IG14 | Windows enumeration in `paddock-walk` (shared, D1): `FileIdExtdDirectoryInfo` per directory, the volume serial once per volume, reparse points skipped, the stat fields on every platform, cancel and progress; its own Windows CI test | | 3 |
| IG15 | Logging (D11): the phase, cache, worker, writer, snapshot and sweep events with run ids, and a test that a diagnostic bundle of a failing fixture run names the cause | IG2, IG3, IG4 | 2 |

**Total.** IG1 to IG15 without IG10 come to 51 agent-days, and the benchmarks to 16.5, so about **67.5 agent-days**, or 70 with IG10.
That is above the reviewer's 55 to 60 because of the packages this revision adds:

- leases and GC;
- the Windows enumeration;
- logging;
- name resolution;
- the move of schema v1's consumers;
- B5 and B8;
- the build-id and golden-corpus work inside IG2.

It is an estimate with the usual uncertainty of a design-stage number (±30%). The packages are added to the first-release plan (section
4.3a there).

## Consequences

- Ingest work becomes proportional to new content, not to the repository or the number of worktrees.
  - A new worktree of an indexed repository still reads and hashes every file once (its modification times are new), but parses only
    content the repository's cache has not seen. That is the price of one code path without Git (D1).
  - Line-ending variants are separate content.
- Two derived artefacts (the fact store and the snapshot) must stay correct. Both derive from content alone, the build id is computed
  rather than remembered, and the rebuild property test and the golden corpus guard them.
- Schema v1 files are rebuilt once.
- UI and MCP graph walks become pure memory reads over a private, versioned file format.
- `paddock-walk` gains the stat fields and the Windows directory enumeration (a shared change, IG14).
- DuckDB stays out of Pacenotes' interactive path; analytics come later through an export.
- The Settings page gains three index Settings with Auto defaults. The repository settings gain "Index submodules" and "Always check
  content" (on by default for remote and FAT volumes).
