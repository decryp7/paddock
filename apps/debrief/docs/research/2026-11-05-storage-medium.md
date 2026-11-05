# Debrief's storage medium, by kind of data

Status: research note, for a decision. Date: 2026-11-05. Code read at `debrief/p0-idle` `b4bf8cb6d` (DuckDB 2.0, Tantivy 0.27,
Parquet 60). Nothing in the product changed. Results: [performance.md](../performance.md), "2026-11-05, storage medium at 1M to
1B events". Raw output and scripts: [2026-11-05-storage-medium/](2026-11-05-storage-medium/).

Questions: what should each kind of Debrief data be stored in, up to a billion events? Is a database easier to manage, and
can a client see it? Would our own format (FlatBuffers and so on) make sense? The user's requirement for any format: binary,
generic, and fast lookup without loading everything.

## 1. Answer

1. **Events: keep Parquet segments, a DuckDB catalogue and Tantivy. Change how they are read.** Today every query binds a
   view over every segment. At 1B events that costs 340 MB before any row is read, over the 300 MB query budget. A catalogue
   of each segment's time and source range, used to hand DuckDB only the files a query needs, made pages about 100× faster
   with a sixth of the memory (10.6 s to 0.10 s; 474 MB to 77 MB).
2. **Global positions (`ord`) are the next wall.** At 1B they take 15 minutes and 4.3 GB (section 3). Scroll by time and
   histogram counts instead of keeping an exact position for each event.
3. **DuckLake is the fallback, and a stronger one than expected.** It gave the same pruned speed with no pruning code of ours,
   and a 7 MB catalogue. It does not yet fit promoted columns, and a one-shot registration peaked at 4.2 GB. Keep the bounded
   trial (plan Y3) with the gate in section 8.
4. **A database file does not by itself give client access.** Measured: while a process holds `data.duckdb` read-write, a
   DuckDB client cannot open it, not even read-only. The Parquet files can be read at any moment. So events in Parquet are
   the more visible design. A small "Open in DuckDB" package closes the rest (section 5).
5. **Our own format: no for events, yes in a narrow form for heap graphs.** Keep samples in Parquet. Keep heap graphs as a
   memory-mapped cache of plain arrays, preferably in an Arrow IPC file, rebuilt from its source rather than migrated.
   FlatBuffers, Cap'n Proto and rkyv add cost and no benefit over plain arrays here (section 6).

## 2. Evidence

- **Measured here:** tables marked as such, all on generated data (ER-14) with the DuckDB 2.0.0 CLI on the Mac mini under load
  5 to 14. Each query ran twice in one process, single runs, so differences under about 20% are noise. "First" still had the OS
  page cache warm; nothing here is a truly cold read.
- **Read in the code:** paths are under `apps/debrief/crates/`.
- **Read in sources:** official pages, linked in section 12. Vendor numbers are marked as vendor numbers.
- **Not verified:** marked "unverified" where it matters. The main one: the corpus has short messages, about 25 bytes per event in
  Parquet against about 50 in the product's `large10` store. The probe's segments are also written by DuckDB, not arrow-rs, and
  have no promoted columns. So the shapes and ratios carry over better than the absolute numbers. The product path at 1B was not
  run (ST-5).

## 3. Measurements on events

Segments of 131,072 rows in row groups of 65,536, zstd; query settings as the store's (192 MB, 4 threads, 4 async threads). Second
run in the process, ms.

| Query | 1M | 10M | 100M | 1B, view today | 1B, files pruned | 1B, DuckLake |
|---|---|---|---|---|---|---|
| Bind, no rows | 1 | 1 | 14 | **145** (first 1,701) | | 1 |
| Count | 3 | 11 | 111 | 1,480 | | 3 |
| 1000-bucket timeline, scan | 8 | 58 | 544 | 6,403 | | 4,209 |
| 1000-bucket timeline, `histogram` | 6 | 35 | 61 | 70 | | |
| Page at a time, mid-data | 16 | 127 | 833 | **10,550** | **102** (144 files) | 98 |
| Filtered page (level, source) | 5 | 9 | 86 | 877 | 196 (120 files) | 205 |
| 200 rows by id | 30 | 122 | 167 | 1,003 | 137 (64 files) | |
| First page, no filter (top-N) | 17 | 155 | 1,484 | **26,220** | | |
| Ids at a position, `ord` | 1 | 0 | 1 | 3 | | |
| Position of an id, `ord` | 1 | 3 | 38 | 427 (first 7,620) | | |

| Cost | 1M | 10M | 100M | 1B |
|---|---|---|---|---|
| Segments: files, bytes | 8, 21 MB | 77, 257 MB | 763, 2.6 GB | 7,630, 25 GB |
| View statement | 842 B | 7 KB | 70 KB | 693 KB |
| Peak footprint, bind only | 12 MB | 14 MB | 46 MB | **340 MB** |
| Peak footprint, page, view | 57 MB | 71 MB | 128 MB | 474 MB |
| Peak footprint, page, pruned | 51 MB | 57 MB | 73 MB | 77 MB |
| `ord` rebuild, 768 MB limit | 0.11 s | 0.70 s | 20.7 s | **912 s** |
| `ord` rebuild, peak footprint | 58 MB | 304 MB | 905 MB | 1,567 MB |
| Catalogue with `ord` | 3.9 MB | 33 MB | 428 MB | **4.27 GB** |
| Reading all footers for zone maps | | 0.01 s | 0.06 s | 2.1 s, 300 MB |

DuckLake (DuckDB catalogue; 1B): registering all 7,630 files took 3.8 s at a peak of **4,216 MB** (452 MB at 100M), catalogue 7.4 MB,
count 3 ms, page peaks 76 to 89 MB, deleting a source 0.65 s. The product would register a few files at a time; that was not
measured.

What this means:

- Reading stays fine with data size only when the file list is pruned. From 10M to 1B a page in a time window grew under 2× when
  pruned and about 80× with the view. A filtered page grows with the files that hold its source (2 files at 10M, 120 at 1B).
- The top-N with no filter cannot prune here because every source starts at the same time. Keep it behind a time window.
- `ord` grows at 4 bytes an event and its rebuild is superlinear past memory: 44× the time for 10× the events. The import budget
  (ADR 0014 §2, 768 MB) is exceeded at 1B even though DuckDB's limit was set to it.
- Search index, not measured at 1B. The recorded rate is 56 to 60 bytes a document and about 330k documents/s
  (performance.md, 2026-10-28; ADR 0062). **Extrapolated**, unverified: about 60 GB and 50 minutes at 1B. That is more than the
  segments. ST-5 must measure it, with a per-source or on-demand index as the likely answer.

The native table was not re-run. Recorded results still rule it out for events: 200k events/s against 4.38M, 8.7× larger
([write paths](2026-10-29-duckdb-write-paths.md), ADR 0014). At 200k/s, 1B takes about 83 minutes. The DuckDB 2 columnar append test
the user asked for stays in plan Y3.

## 4. What fits each kind of data

Lookup column: how a single item or page is reached without loading everything. All rows here are binary and generic unless
noted.

| Data | Today (code) | Recommendation | Fast lookup by | Why |
|---|---|---|---|---|
| Events | Parquet, arrow-rs (`debrief-store-duck/src/segments.rs`) | **Keep**, add zone maps | Catalogue zone map to file, row group statistics inside the file; Parquet 60 writes the page index by default (read in `file/properties.rs`), but whether DuckDB uses it is unverified | Parallel writers, compression, SQL, every client |
| Segment list, sources, diagnostics | DuckDB catalogue (`schema.rs`) | **Keep** | Primary keys, zone maps | Small, transactional, edited |
| Histogram, field stats, lanes | DuckDB catalogue (`stats.rs`, `lanes.rs`) | **Keep** | Filter on bucket: 70 ms at 1B | Answers timelines without touching events |
| Positions | `ord` table (`pos.rs`) | **Replace at scale**: time keyset paging, histogram-based scroll | Keyset with zone maps: 102 ms at 1B | Section 3 |
| Word index | Tantivy, mmap (`fts.rs`) | **Keep**, measure at 1B | Term dictionary | Tokeniser matches our needs (ADR 0014). `fts` extension rejected earlier; SeekStorm has far less use (crates.io downloads 2.5k against 5.2M in 90 days) |
| Search hits | Parquet spill (`hits.rs`) | **Keep; the pattern to copy** | `id IN (read_parquet)` | Bounded, already streaming |
| Analysis results | JSON blobs in `analysis_cache` (`analysis.rs:586`) | **Change** (audit Q-2): rows, or a Parquet file per large result | Paged SQL | A blob must be read whole |
| Fallback scan matches | `Vec<u64>` of all ids (`read.rs:538`, `:658`) | **Change** (Q-1): keyset paging, or spill as `hits.rs` does | Keyset | Memory grows with matches. SQL `OFFSET` itself has gone from the store (grep); the offset indexes this list |
| Profile samples | In the UI process (`debrief-profile/src/model.rs:231`, `store.rs:87`) | **Parquet** in the project | Time filter, row groups | Section 6 |
| Frames, stack nodes | In the UI process | **Parquet**, read into a small cache | By id | Up to 1M and 4M rows (`limits.rs`) |
| Heap graph | In the UI process, already arrays (`heap/model.rs:22`) | **Memory-mapped cache**, Arrow IPC or plain arrays | Array index: 50 ns | Section 6 |
| Dump documents | In the UI process; the dump is read by ranges (ADR 0023) | **Keep the source file and range reads**; bound the parsed form (audit MEM-1, MEM-2) | File ranges | Dump formats are already indexed |
| Notes, reports, UI state | Versioned JSON, capped (`debrief-notes`, `services/project.rs`, 64 MB) | **Keep** | Whole small file | Small, edited, readable. FlexBuffers gives in-place reads, but no client can read it and nothing here is big enough to need them |
| Video frames, media | Files by content key (`debrief-video/src/record.rs`) | **Keep** | Path | Images are already files |
| Conversations, AI memory | JSON; SQLite proposed (`paddock-assistant`; [AI memory research](../../../../docs/research/2026-10-ai-memory.md)) | **Keep that plan** | SQLite FTS5 | Global scope, not log data |
| Template vectors | Planned (X55, brute-force cosine) | **Parquet or a catalogue table** | Whole set is small | No vector database needed at this size |
| Settings | JSON and TOML | **Keep** | | Must stay editable |

Formats named by the user and the coordinator against the lookup requirement:

| Format | Lookup without loading all | Verdict |
|---|---|---|
| Parquet with row group statistics and page index | Yes; measured 200 rows in 137 ms at 1B | **Events, samples, large results** |
| Arrow IPC with a memory map | Yes by batch and array index; zero-copy if aligned ([FileDecoder](https://docs.rs/arrow-ipc/latest/arrow_ipc/reader/struct.FileDecoder.html)); no statistics | **Heap graph container** |
| Plain versioned arrays | Yes; 50 ns | **Heap graph fallback** |
| Vortex | Yes; "100×" is a vendor figure an independent audit calls a benchmark artefact, about 1.3× on ClickBench ([audit, secondary](https://mohakchugh.is-a.dev/blog/compute-pushdown-compressed-data-vortex-columnar-format)) | No: DuckDB's extension has no Windows build (checked: 404; [docs](https://duckdb.org/docs/2.0/core_extensions/vortex)) |
| Lance | Yes; vendor "100×" | No: 74 to 93 MB extension, third party, and crate majors move fast (14.0.2 now, 4.0 in April) |
| FlexBuffers | Yes for keys, binary search ([docs](https://flatbuffers.dev/flexbuffers/)) | Not needed: JSON documents are small and big results belong in rows |

## 5. Options, with manageability

| Option | Write 1B | Read at 1B | Live client access | Licence, health | Fit | Rank |
|---|---|---|---|---|---|---|
| **Segments + catalogue + zone maps** | Today's writers | Measured best | Parquet: yes. Catalogue: copy or close (below) | MIT, Apache; active | No new dependency; about 3.5 days | **1** |
| DuckLake over the segments | Same writers, then register | Measured as good, without our code | DuckDB catalogue: as today. SQLite catalogue: a second process read while the first held it (measured). Clients need the extension | MIT, spec 1.1, [backward compatibility promised](https://ducklake.select/2026/04/13/ducklake-10/); DuckDB's [secondary tier](https://duckdb.org/docs/2.0/core_extensions/overview) | 13 to 17 MB per platform (compressed); promoted columns conflict; would own our files | **2, fallback** |
| Native tables, all events | About 83 min, 9× larger | 1 to 3× faster on 10M-era numbers | **No while Debrief runs** (measured) | MIT | Rejected in ADR 0014 | 3 |
| Hybrid: fan-out, then convert | Adds conversion | Not measured | No, for converted data | MIT | Costs disk and time twice; plan Y3 still measures it | 4 |
| Vortex, Lance | Own writers | Not measured | Vortex: no Windows reader | Apache | Section 4 | 5 |
| chDB | Unknown | Not measured | One data path per process | Apache; Rust binding "experimental", [no Windows stated](https://github.com/chdb-io/chdb) | Fails platform support | No |
| DataFusion over Parquet | Same | ADR 0010 spike only | Parquet yes | Apache | ~130 MB (ADR 0010); cooperative cancel only | ADR 0010's reserve |
| SQLite, `sqlite-columnar` | Row writes | Not measured | SQLite allows live readers | Public domain; extension Apache with 16 stars, new in 2026 | Not for 1B events; fine for small records | Not events |
| Own row format | Fast | No pruning, no SQL | None | Ours | Section 6 | No |
| fjall, redb, RocksDB for positions | Fast | Point lookups | None | Permissive | A second engine to save one table; keyset paging removes the need | No |
| Quack server | Rejected | | Would allow clients | MIT | ADR 0010; memory cost recorded | No |

**Is a database easier to manage? Can a client see it?** Measured with the DuckDB CLI on a store held open read-write by another
process (`mgmt` in the harness). DBeaver and Python were not run; they use the same engine, but their versions vary.

| What | Result |
|---|---|
| Open the catalogue read-only while held | **Refused**: "Could not set lock on file" ([DuckDB's rule](https://duckdb.org/docs/2.0/connect/concurrency)) |
| `ATTACH ... (READ_ONLY)` while held | **Refused**, same error |
| `read_parquet` on the segments while held | **Works** |
| Copy the catalogue file while held | Opens, but **misses what is still in the WAL**, and a copy of a live file is not guaranteed consistent |
| Copy file and WAL, open read-write | The change appears; the same consistency caveat applies. Ask the worker for a snapshot instead |
| Open read-only after Debrief closes | Works, views included |
| DuckDB 1.5.6 client on the pinned catalogue (`v1.5.0`, `lib.rs:241`) | Works |
| DuckDB 1.5.6 client on a default 2.0 file | Refused: version 69 |

So the honest comparison:

- **Viewing:** a database is easier only when Debrief is closed. Parquet works always and from more tools. One trap: a raw glob
  also shows removed sources and any old files waiting in `trash`, until compaction. The catalogue is what knows which rows count
  (`schema.rs`, `READY`).
- **Schema to a person:** events read well (`message`, `raw`, `extra`). `ts` is in ms, `level` is a code, `ord` and `f_`/`p_` columns
  need explaining. ADR 0030's console views are the documented, versioned form, but live only inside the app.
- **Views and moved folders:** `events` names absolute paths and is recreated when Debrief opens the store, so a client opening a moved
  or unpacked folder gets a broken view until then (`schema.rs`).
- **Backup:** copy the folder with Debrief closed, leaving out `index/` and `tmp/`, or Save project (`.debriefz`, exists).
- **Repair:** the index and `ord` rebuild from segments; a missing segment is refused with a message; index repair is planned (WP-R).
  A native table has one failure domain, while segments lose only what a bad file holds.
- **Removal:** a source leaves at once through the view, and its bytes at compaction. Deleting a project is deleting a folder.
- **Size of the gain from DuckLake:** a standard schema and snapshots, at the cost of one extension for every client. Its DuckDB catalogue has
  the same one-process limit ([catalogue guide](https://ducklake.select/docs/stable/duckdb/usage/choosing_a_catalog_database)).

ST-6 turns this into a feature: `Open in DuckDB…` writes a script of the documented views with relative paths, and takes a consistent
catalogue snapshot through the worker. No extra process needs the lock.

## 6. Our own format, and MEM-6

**Events: no.** Tested on the expectation, not assumed. A row format loses three measured things: column pruning (page reads above
touch a few of 13 columns), compression (raw text is about 6.5× the Parquet size: 365 against 56 bytes an event in the 3M corpus), and SQL
with every client. It would also make us own a parser for hostile files. Parquet writing is already 4.38M events/s, so there is no speed
to win. An `.etlx`-style cooked format solves a problem Parquet does not have.

**Captures: measured against the caps in `limits.rs`** (20M objects, 100M references, 20M samples; generated; warm page cache).

| Operation | Plain arrays, mmap | Parquet + DuckDB |
|---|---|---|
| File size, references | 400 MB + 160 MB offsets | 410 MB (random ids barely compress) |
| File size, 20M samples | 400 MB | **111 MB** |
| One object's references | **50 ns**, footprint 1.5 MB | 10 to 47 ms, 17 MB |
| Reachability over the graph | **1.18 s**, 84 MB | 19.2 s, **1,230 MB** (recursive SQL, over its 768 MB limit) |
| Hotspots by stack, time window, 5M of 20M | 19 ms, 3 MB | 47 to 79 ms, 115 MB |
| Validate on open (offsets ordered, every id in range) | **39 ms** | Done by the reader |

Reachability figures use the same shape but not the same random edges, and the SQL result count was not kept. RSS of the arrays peaked at
562 to 644 MB of clean file pages; macOS footprint does not count them (above). On Linux, PSS does by definition (unverified on our watcher), so
the rule 26 watcher could charge them to the process. Lengauer-Tarjan's working memory and build time in the real crate are not measured.

**So, per kind:**

- **Samples, frames, stacks: Parquet.** Queries are a few times slower than arrays and still far inside the budget. Samples get 3.6× smaller.
  SQL and clients work. The existing segment writer and worker do the job, and captures become self-contained, where today they re-import
  from a path that may be gone ([profiling.md](../profiling.md)).
- **Heap graph: memory-mapped arrays.** Dominators and paths need millions of random jumps, and Parquet is 200,000× slower for one lookup (about 2,000× per lookup in a batch of 200). `ObjectGraph` is
  already arrays (`edge_start`, `edge_to`, `type_of`, `size`), so the on-disk layout is the in-memory layout. Eclipse MAT does the same in spirit:
  index files are built once, and [later opens need less memory](https://github.com/eclipse-mat/mat/blob/53ab242c2301f0e675f27cb605776ba801f71e7a/plugins/org.eclipse.mat.ui.help/tasks/configure_mat.html).

**Container for the arrays:**

| | Arrow IPC file | Plain versioned arrays | FlatBuffers | Cap'n Proto | rkyv |
|---|---|---|---|---|---|
| Generic, other tools can read | **Yes**, PyArrow and others | No | Needs our schema | Needs our schema | Rust only |
| Zero-copy with mmap | Yes, if aligned | Yes | Yes | Yes | Yes |
| Size limit | Batches, 64-bit | None | [2 GiB](https://docs.rs/crate/flatbuffers/latest/source/src/builder.rs) in the Rust builder; 64-bit support in Rust unverified. The graph at the caps is about 1.1 GB, so no room if caps rise | 29-bit lists; [64 MiB traversal limit](https://capnproto.org/encoding.html) to raise | Unverified |
| Open on untrusted bytes | Arrow validates layout; unsafe skip exists | Our 39 ms check | [Verifier](https://docs.rs/flatbuffers/latest/flatbuffers/struct.VerifierOptions.html) checks layout only | Lazy checks | [`bytecheck`](https://rkyv.org/validation.html) |
| Semantic checks still ours (`edge_to < n`) | Yes | Yes | Yes | Yes | Yes |
| Schema evolution | Append columns | Version and rebuild | [Add at end, never remove](https://flatbuffers.dev/evolution/) | Yes | Docs state no policy |
| New dependency, tooling | `arrow-ipc` (Arrow is in the workspace, not in `debrief-profile`) | `memmap2`, `bytemuck` | `flatc` step | `capnpc` step | Derives |

All of these formats wrap the same arrays and all still need our range checks, so what they add is schema machinery for a record with five
vectors. **Rank: Arrow IPC first, as it meets "binary and generic" and leaves no format of ours to own; plain arrays if the IPC overhead or
alignment gets in the way in the real crate.** FlatBuffers and the others: no.

**Cost of owning a format, kept low by making it a cache.** PerfView's `.etlx` [regenerates when missing, older or unreadable](https://github.com/microsoft/perfview/blob/main/src/TraceEvent/TraceLog.cs);
do the same. The heap file lives in a cache folder, is never exported (like `index/`), and a version mismatch means rebuild, so there is no migration code.
Hostile-input safety then has two parts: the importer stays as hardened as today, and the cache gets its 39 ms check on open and a parser that
never panics. Written to a temporary name and renamed.

**mmap risks:** a truncated or changed file is [undefined behaviour](https://docs.rs/memmap2/latest/memmap2/struct.Mmap.html) and, in practice, a bus error, which is why section L8 of the
[DuckDB 2 note](../../../../docs/research/2026-10-duckdb-2-and-large-data.md) refused mmap for input files. Here the file is ours and immutable, but the map must still live **outside the UI process**, as
MEM-6 proposes. Windows cannot delete a mapped file; reuse the store's `trash` approach.

**MEM-6 recommendation:** approve the ADR and measurement (3 days), on these two formats. The 8 to 12 day build stays the user's call after the numbers, as already decided.
MEM-1 and MEM-2 are still needed first: they stop the crash risk regardless.

## 7. Audit findings

Consistent with this note; each maps to a package: analysis JSON blobs (Q-2), fallback scans keeping all ids (Q-1, `read.rs:538`), samples and heap graphs in the UI process
(MEM-6). One correction: the store has no SQL `OFFSET` left at this commit (grep of `debrief-store-duck/src`). "OFFSET paging" means a positional offset into the kept id list (`read.rs:658`), and pages by
`ord` position (`pos.rs`). Q-1 should cover both, and ST-3 replaces the second.

## 8. Recommendation

1. **Build ST-1 and ST-2 within Q-1.** Highest gain per day in this note, no dependency, no format risk.
2. **Then ST-3**, after an ADR, since it changes how the grid's scrollbar behaves.
3. **ST-5 before any further decision at 1B**: the product path, real writers, Tantivy, promoted columns.
4. **Run the DuckLake trial (Y3) with this gate.** Adopt only if all hold: promoted columns have a design (today [extra columns are refused or unreachable](https://ducklake.select/docs/stable/duckdb/metadata/adding_files)); per-import registration stays in the
   768 MB budget; it passes on Windows; ownership of files works with Tantivy's segment keys and `segment_blocks` (ADR 0062); and the code it removes outweighs a 13 to 17 MB extension that follows each DuckDB version. Otherwise it stays out, with ST-1 already giving its read speed.
5. **Fallback** if segment count defeats pruning on real data (for example, sources all covering the same time): compact by time into sorted segments, then DuckLake.

## 9. Work packages

Sizes are judgement until an ADR exists. Budgets follow ADR 0014; every number goes to `performance.md` (rule 48). Parallelism and cost: ST-1 and ST-2 are the "cheaper" answer of rule 51; no thread counts change.

| WP | What | Owns | Test first, done when | Days |
|---|---|---|---|---|
| ST-1 | Zone maps in `segments` (ts, source, level), filled while writing, backfilled on open; `FORMAT_VERSION` bump | `schema.rs`, `segments.rs`, `writer.rs`, `stats.rs` | Zone maps equal `parquet_metadata` on a generated store | 1 |
| ST-2 | Read paths pass pruned file lists; bind no full view for a page; `events` only as the fallback and console. Known schema instead of `union_by_name` reads where segments allow | `read.rs`, `pos.rs`, `schema.rs` | A rows-read test (audit Q-4); 1B page under 0.5 s and 300 MB on the generated corpus | 2.5 |
| ST-3 | ADR, then keyset paging and histogram scroll; `ord` only below a size threshold | `pos.rs`, `read.rs`, `ports`, grid | Anchor, jump to time, count per position stay right; no `ord` build above the threshold | 5 |
| ST-4 | Analysis results as rows or Parquet (is audit Q-2) | `analysis.rs` | Page of a large result reads one page | in Q-2 |
| ST-5 | Product bench at 100M and 1B: arrow-rs writers, real shapes, Tantivy size and rate, footprint per phase. Runs on a quiet machine with about 200 GB free | `tests/bench*`, generator | Results block; per-source index decision | 2 |
| ST-6 | `Open in DuckDB…`: script of documented views with relative paths, worker-made catalogue snapshot, a schema page | `console/`, `project.rs`, app command, docs | The script reads the right rows after a source was removed and with the store moved | 2 |
| MEM-6a | ADR and measurement: samples, frames and stacks as Parquet; heap graph as IPC and as arrays; dominators on the map; Linux PSS and Windows working set under each watcher | `debrief-profile`, notes | Numbers for the user's build decision | 3 (audit's) |
| Y3 | DuckLake trial with the section 8 gate; Windows included | Trial branch | Gate answered in an ADR | 3 (plan's) |

New days: about 12.5 (ST-1, 2, 3, 5, 6). ST-1 and ST-2 can start after D-a on the 1.2 base, app only; they touch files Q-1 also owns, so they should be one agent.

## 10. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| Real data has overlapping time per file more than the corpus | Fewer files pruned | ST-5 uses the bundle's invented shapes; time-sorted compaction is the fallback |
| Generated events are shorter than real ones | Absolute times optimistic, footers bigger with promoted columns | Ratios guide; ST-5 sets the numbers |
| Tantivy at 1B dominates disk and time | Search ready long after import | Measure; per-source or lazy index |
| Pruned lists differ from the view (non-ready sources) | Wrong rows | Keep the `READY` filter on every path; differential tests exist |
| A big `IN` of paths or a long list costs too | Part of the gain lost | 693 KB of list was the whole store; pruned lists are about 2% |
| DuckDB 2.0.0 is an x.0 | Regressions | No 2.0.1 yet ([releases](https://github.com/duckdb/duckdb/releases)); the crate has 1.20000.1 while the lockfile has .0: check it |

## 11. Decisions for the user

1. Accept "keep Parquet, catalogue and Tantivy, add pruning" and add ST-1 to ST-3, ST-5 and ST-6 to Debrief 1.2 (about 12.5 days)?
2. Is an exact position for every event needed above some size, or is scrolling by time with an estimated position fine? ST-3 depends on this. Suggested: exact below about 100M.
3. MEM-6: ADR and measurement on Parquet for samples and an Arrow IPC or plain array cache for heap graphs, as above?
4. Is a billion events a real target for 1.2, or is it the ceiling to design for? If 100M is the 1.2 target, ST-3 and ST-5's 1B run can wait, and ST-1 and ST-2 still pay off.

## 12. Sources

Read 2026-11-05 unless noted.

- DuckDB: [concurrency](https://duckdb.org/docs/2.0/connect/concurrency), [migration](https://duckdb.org/docs/2.0/operations_manual/migration), [storage](https://duckdb.org/docs/2.0/internals/storage), [2.0 announcement](https://duckdb.org/2026/10/21/announcing-duckdb-200), [schemas across files](https://duckdb.org/docs/2.0/data/multiple_files/combining_schemas), [Vortex extension post](https://duckdb.org/2026/01/23/duckdb-vortex-extension.html) (TPC-H, single laptop: 12 to 18% faster than Parquet v2).
- DuckLake: [docs](https://ducklake.select/docs/stable/), [1.0](https://ducklake.select/2026/04/13/ducklake-10/), [1.1](https://ducklake.select/2026/10/21/ducklake-11/) (vendor benchmarks), [adding files](https://ducklake.select/docs/stable/duckdb/metadata/adding_files), [maintenance](https://ducklake.select/docs/stable/duckdb/maintenance/recommended_maintenance).
- Formats: [Vortex](https://github.com/vortex-data/vortex), [Lance](https://github.com/lance-format/lance), [chDB](https://github.com/chdb-io/chdb), [sqlite-columnar](https://github.com/sqliteai/sqlite-columnar), [Arrow versioning](https://arrow.apache.org/docs/format/Versioning.html), [Parquet page index](https://parquet.apache.org/docs/file-format/pageindex/), [Tantivy](https://github.com/quickwit-oss/tantivy), [fjall](https://github.com/fjall-rs/fjall), [rust-rocksdb](https://github.com/rust-rocksdb/rust-rocksdb).
- Licences and versions: crates.io and GitHub APIs, the same day. Extension sizes: `extensions.duckdb.org/v2.0.0`, `Content-Length`, the same day.
- Secondary, not checked further: the Vortex audit above; deepwiki pages on PerfView were not relied on.
- Repo: ADR 0010, 0014, 0023, 0030, 0062; [write paths](2026-10-29-duckdb-write-paths.md); [DuckDB 2 note](../../../../docs/research/2026-10-duckdb-2-and-large-data.md).
