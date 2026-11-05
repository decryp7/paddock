# Debrief's storage medium, by kind of data

Status: research note, for a decision, **revised after an independent review; the 1B runs on real segments are still to come (marked not measured).** Date: 2026-11-05.
The note sits on `main` `87c9b8449`, but the code it describes is `debrief/1.2-int` `852b35ac0`, also read at `debrief/p0-idle` `b4bf8cb6d` (DuckDB 2.0, Tantivy 0.27, Parquet 60). `read.rs` line numbers are 1.2-int's. Nothing in the product changed. Results: [performance.md](../performance.md), "2026-11-05, storage medium: real segments and same-window rerun". Raw output and scripts: [2026-11-05-storage-medium/](2026-11-05-storage-medium/); `results/rerun/` holds this revision's.

Questions: what should each kind of Debrief data be stored in, up to a billion events? Is a database easier to manage, and
can a client see it? Would our own format (FlatBuffers and so on) make sense? The user's requirement for any format: binary,
generic, and fast lookup without loading everything.

## 1. Answer

1. **Events: keep Parquet segments, a DuckDB catalogue and Tantivy, and change how they are read.** That recommendation stands, but the first version's headline does not: its "about 100× faster" compared a view query with no upper time bound against a pruned query with one, on a corpus that was the best case for pruning, and its "today" was not what the product runs. See section 3.
2. **Prune by the `histogram` table, not by new min/max zone maps.** On segments the product's own import wrote, min/max ranges selected **98%** of files for a 1/500 time window on the bundle's parsable text (5.6M events) and **28%** on an all-text upper bound (85M events). `histogram` already records each file's minutes, levels and sources, and selected **11%** and **10%** on the same stores, with no schema change, in 3 ms once in memory. For one source it selects 1 file against 7.
3. **What today actually costs at 85M, warm:** an unfiltered page about 65 ms, a source and level page about 130 ms, a time window about 270 ms before rows. The problems are `LIMIT … OFFSET` deep in a filter (8.2 s, 388 MiB), a page while importing or without `ord` (11.5 s mid-data, 393 MiB), and a level-only filter (0.84 s), which no file list can narrow because every file holds every level. So **keyset paging (ST-3) matters as much as pruning.**
4. **Global positions (`ord`) break their budget before the suggested threshold.** 905 MiB at 100M against the 768 MB import budget, on the uniform corpus. At 1B: 15 minutes, 1,567 MiB, 4.27 GB.
5. **1B: not measured on real segments yet.** The first version's 1B figures come from a best-case corpus and do not show the approach reaches 1B. Whether our own approach gets there, and so whether the DuckLake trial should run sooner, waits for those runs.
6. **DuckLake is out of Debrief 1.2 (user decision, 2026-11-05)** and no 1.2 work depends on it. The trial stays after 1.2.
7. **A database file does not by itself give client access.** Measured: while a process holds `data.duckdb` read-write, a DuckDB client cannot open it, not even read-only. The Parquet files can be read at any moment. A small "Open in DuckDB" package closes the rest (section 5).
8. **Our own format: no for events, yes in a narrow form for heap graphs.** Keep samples in Parquet. Keep heap graphs as a memory-mapped cache of plain arrays, preferably in an Arrow IPC file, rebuilt from its source rather than migrated. FlatBuffers, Cap'n Proto and rkyv add cost and no benefit here (section 6). The first version's SQL reachability number was invalid; the corrected result strengthens this.

## 2. Evidence

- **Measured here:** tables marked as such, all on generated data (ER-14) with the DuckDB 2.0.0 CLI on the Mac mini under load 5 to 14. Each query ran twice in one process and the second is quoted, single runs, so differences under about 20% are noise.
- **Warm only.** The OS page cache was warm throughout. "First" means a new process, not a cold disk. The interactive budget says `< 0.5 s cold`, so "within" below means within it when warm; **cold was not measured**, and an over-budget warm result is over-budget cold too.
- **Memory is the CLI's, not the worker's.** Harness figures are peak footprint of a DuckDB CLI process, in MiB. The budget is store worker peak RSS: ≤ 300 MB, with its test allowing 320 MB on Linux and 630 MB on macOS (performance.md). A bare CLI has no catalogue caches or names, so these figures can only compare with each other; **the worker budget is not measured** here, except for the `debrief query` line in section 3.5. Disk sizes are decimal GB.
- **Read in the code:** paths are under `apps/debrief/crates/`.
- **Read in sources:** official pages, linked in section 12. Vendor numbers are marked as vendor numbers.
- **Stores.** `real` and `upper` were written by the real import (`debrief import`, release build of 1.2-int `852b35ac0` plus an unrelated example-only dependency), so segments, promoted columns, `histogram` and `ord` are the product's. The full-text index was not built: the CLI exits before the background indexer finishes (28 MB at 85M).
- **Not verified:** marked where it matters.

## 3. Measurements on events

### 3.1 What the review changed, checked

Each finding was checked against the code and by rerun.

| Finding | Check | Result |
|---|---|---|
| The 100× compared different queries | `harness/probe.py` at `bc477a4b3`: the view query had no upper bound, the pruned and DuckLake ones had one | **Agreed.** The reviewer's rerun (their numbers, second run, load about 5): at 100M and 763 files, no bound 824 ms, same window 98 ms, pruned 67 ms; at 6,104 files, 1,186, 485 and 18 ms. So at 763 files pruning gains about 1.5×, not 100×. My own rerun at 10M: 143, 68, 75 ms. **1B: not measured yet;** the reviewer estimates about 6× at 7,630 files |
| "Today every query binds a view" | `read.rs:559-598`, `:688-728`: an unfiltered page reads `ord` and `rows_by_id` already prunes | **Agreed.** The 26,220 ms "first page" only happens while importing or with `ord` missing |
| The store has no `OFFSET` | `read.rs:628` at both commits | **Agreed, withdrawn** (section 7) |
| The corpus flatters pruning | `probe.py:39` writes one source and a narrow slice per file; `writer.rs` `slot` holds one writer per parse thread; `parallel.rs:221` reads largest first | **Agreed, and it is worse than estimated on the bundle's text:** 98%, against the reviewer's simulated 60% |
| No page index reads in DuckDB | grep at `v2.0.0` | **Agreed:** `column_index` hits are schema positions. Grep only |

### 3.2 The gate: how many files a query selects

Measured on stores the product's import wrote. Median over 100 windows centred on where events are; p90 in brackets. "Min/max" is what `segments` zone maps would select; "histogram" is `SELECT DISTINCT file FROM histogram WHERE …`, to the minute.

| | Uniform corpus (first version) | `real`: the bundle's parsable text | `upper`: every file parses | 1B, real mix |
|---|---|---|---|---|
| Corpus | 1B, 7,630 files, 1 source a file | 5,422 invented files, 590 MB, 2,732 sources, **5.6M events, 53 segments** | 14,255 invented files, 64 rolled families, 7.61 GiB, **85M events, 672 segments** | not measured |
| Sources per segment, median (max) | 1 | 28 (303) | 21 (67) | |
| Segment's time span, median share of all | small | **94%** | 8.7% (p90 74%) | |
| Window 1/500, min/max | 1.9% | **98%** (53 of 53 at p90) | **28%** (42%) | not measured |
| Window 1/500, row groups, min/max | | 86 of 96 | 241 of 1,339 | not measured |
| Window 1/500, **histogram** | | **11%** (17%) | **9.7%** (15%) | not measured |
| Window 1/5000, min/max, histogram | | 98%, 3.8% | 28%, 6.9% | not measured |
| Window 1/50, min/max, histogram | | 99%, 52% | 29%, 18% | not measured |
| One source, min/max, histogram (files) | | 7, **1** | 7, **1** | not measured |
| One level, min/max, histogram | | all, **all** | all, **all** | not measured |
| Rows in the selected files per row wanted, 1/500 | | 431 by min/max, 55 by histogram | 164, 56 | not measured |

**Gate for ST-1 and ST-2:** a 1/500 window selects at most 15% of segments (median, on both bundle-shaped stores), and a source filter at most 2 files. Min/max fails it clearly. The histogram passes it at 1/500 and is close to the mark at p90. Selection does not help a text or level filter: every file holds every level, and words are Tantivy's job.

`real` holds only the small text files, so its 53 segments are few and its result rests on few files. The real bundle's import does not finish today: the worker stops at its memory cap on large files, which another agent is reproducing, so files over 48 MiB, EVTX and dumps are not in these stores. Its events are therefore far fewer than the 57M the review simulated. About 2,793 of its 14,255 files import (`docs/research/2026-11-debrief-bundle-shapes.md` on branch `docs/debrief-bundle-shapes`). `upper` has 85M rather than 57M because its lines are shorter.

Time-sorted compaction, the fallback for overlap: not measured yet.

### 3.3 Today's statement sequences

The product's own statements (`read.rs`, `sql.rs`), run read-only on those stores. Second run in the process, ms, footprint in MiB in brackets. Sums exclude the rows step unless stated.

| Page | Statements | 5.6M `real` | 85M `upper` | 1B real mix | Verdict at 85M, warm |
|---|---|---|---|---|---|
| Unfiltered, not importing | count `ord`, ids by `pos`, rows from pruned files (32 files) | 21 (60) | **65** (111) | not measured | within |
| Source + level | `count(*)` over view, ids, rows | 28 (49) | **131** (73) | not measured | within |
| Time window 1/500 | `count(*)`, ids; rows not run | 38 (47) | **269** (106) | not measured | within, before rows |
| Level only, first page | `count(*)`, ids; rows not run | 61 (47) | **844** (108) | not measured | **over** |
| Level only, `OFFSET` 5% of events | `count(*)`, ids | 106 (122) | **8,380** (388) | not measured | **over** |
| While importing, first page | `count(*)`, ids | 63 (66) | **801** (102) | not measured | **over** |
| While importing, mid-data | `count(*)`, ids | 162 (214) | **11,670** (393) | not measured | **over** |
| Text, substring, no index | scan | | 1,858 (110) | not measured | over; Tantivy answers words |
| Timeline, `histogram` | 1000 buckets | | | 70 at 1B on the uniform corpus | within |

The same predicates with a histogram-pruned file list, ids only:

| Page, ids step | View today | Pruned by min/max | Pruned by histogram |
|---|---|---|---|
| Time window, 85M | 150 ms (106), 283 of 672 files by min/max | 103 ms (73) | **49 ms** (56), 98 files |
| Source + level, 85M | 69 ms (73), plus 52 ms count | | **4 ms** (35), 2 files; count from `histogram` under 1 ms |
| Time window, 5.6M | 20 ms (47) | 19 ms (44), 53 files | **4 ms** (36), 6 files |

So at bundle size pruning gains about 3× on a window page and 17× on a source page, and the budget is not at risk from those pages. It is at risk from deep `OFFSET` and level-only pages.

### 3.4 What grows with sources

Measured at 14,255 sources. `ctx()` (`read.rs:188-247`) rereads all `sources`, `segments.promoted` and `segment_fields` on every page: each statement took under 1 ms at 85M in the CLI, to a discarding output. Decoding rows and building its maps in Rust is not in that figure, and at 14k sources its `HashMap` is rebuilt per page.

| At 85M, 14,255 sources | |
|---|---|
| `histogram` | 6.36M rows (0.075 a row per event) |
| Distinct (file, minute) | 2.75M |
| Distinct (file, source) | 14,721 |
| Catalogue | 402 MB, of which `ord` about 340 MB |
| Selecting files from `histogram` | 198 ms first, 3 ms after; source + level 12 ms, then 1 ms |

`histogram` grows with events and with sources × minutes. A narrower (file, minute) table and (file, source) table would be about 40% and 0.2% of its rows, if selection at 1B needs them. **At 1B and at 171k sources: not measured.**

### 3.5 Costs and the uniform corpus

The first version's corpus: one source and a narrow slice per file, DuckDB-written, 13 columns, no promoted columns. It remains useful for how costs grow, not for what pruning gains. Query rows are from `bc477a4b3` and use simplified statements, not the product's.

| Cost | 1M | 10M | 100M | 1B |
|---|---|---|---|---|
| Segments: files, bytes | 8, 21 MB | 77, 257 MB | 763, 2.6 GB | 7,630, 25 GB |
| View statement | 842 B | 7 KB | 70 KB | 693 KB |
| Peak footprint, bind only | 12 MiB | 14 MiB | 46 MiB | **340 MiB** |
| `ord` rebuild, 768 MB limit | 0.11 s | 0.70 s | 20.7 s | **912 s** |
| `ord` rebuild, peak footprint | 58 MiB | 304 MiB | **905 MiB** | 1,567 MiB |
| Catalogue with `ord` | 3.9 MB | 33 MB | 428 MB | **4.27 GB** |
| 1000-bucket timeline from `histogram` | 6 ms | 35 ms | 61 ms | 70 ms |
| Position of an id, `ord` | 1 ms | 3 ms | 38 ms | 427 ms (first 7,620 ms) |

- **Binding a whole view at 1B costs 340 MiB of footprint** before a row is read. Against a 300 MB worker budget that is a real warning, though the CLI is not the worker. Paths that already prune skip it.
- **`ord` and the 100M threshold.** The user's "exact below about 100M" is over the 768 MB import budget at that size: 905 MiB on the uniform corpus, though the limit was set. It held at 304 MiB at 10M. So either the threshold is lower (where, between 10M and 100M, is not measured; a straight line gives about 80M, unverified), or the budget for this one step is raised, or the ordering gets cheaper. The time is fine at 100M (21 s). ST-3's ADR must choose; on `upper`, the real mix at 85M is queued.
- `ord` is 4 bytes an event, and its rebuild is superlinear past memory: 44× the time for 10× the events.
- **`debrief query` through the real worker** peaked at 533 MiB RSS at 5.6M and 539 MiB at 85M for `*` (ps, 0.1 s polling; includes opening the store). That is within the 630 MB macOS allowance and over 300 MB, but it is the CLI's evaluator, not the app's page path, and not a verdict on the budget. Over its 2M-event limit, the level query refused with a clear message after the worker reached 1,318 to 1,622 MiB: worth a look in Q-4.
- **Search index, not measured at 1B.** `large10` recorded 1.52 GB for 56M events, about 27 bytes a document (performance.md, 2026-10-28), and about 330k documents/s. **Extrapolated, unverified:** about 27 GB and 50 minutes at 1B, against about 50 GB of segments at the product's 50 bytes an event. The first version's "60 GB, more than the segments" was wrong. Search being ready long after import remains the concern, not the disk. The size per event depends on message text.

The native table was not re-run. Recorded results still rule it out for events: 200k events/s against 4.38M, 8.7× larger ([write paths](2026-10-29-duckdb-write-paths.md), ADR 0014). At 200k/s, 1B takes about 83 minutes. The DuckDB 2 columnar append test the user asked for stays in the post-1.2 trial.

DuckLake on the uniform corpus at 1B, from the first version and with the same window as the pruned query, so not affected by the first finding: registering all 7,630 files took 3.8 s at a peak of 4,216 MiB (452 MiB at 100M), catalogue 7.4 MB (no advantage: ours is about 4.5 MB before `ord`), count 3 ms, window page 98 ms. Its file statistics are min/max too, so **expect the same weakness on mixed segments**; that was not measured, and no new DuckLake runs were made.

## 4. What fits each kind of data

Lookup column: how a single item or page is reached without loading everything. All rows here are binary and generic unless
noted.

| Data | Today (code) | Recommendation | Fast lookup by | Why |
|---|---|---|---|---|
| Events | Parquet, arrow-rs (`debrief-store-duck/src/segments.rs`) | **Keep**, prune by the histogram | `histogram` to file, row group statistics inside the file. Parquet 60 writes the page index by default (`file/properties.rs`), but DuckDB 2.0.0 shows no sign of reading it: no `offset_index` or `page_index` in its parquet reader (grep of `parquet_reader.cpp`, `column_reader.cpp`; the `column_index` hits there are schema positions; absence is not proved) | Parallel writers, compression, SQL, every client |
| Segment list, sources, diagnostics | DuckDB catalogue (`schema.rs`) | **Keep** | Primary keys, zone maps | Small, transactional, edited |
| Histogram, field stats, lanes | DuckDB catalogue (`stats.rs`, `lanes.rs`) | **Keep; it is also the file selector** | Filter on minute, source, level | Answers timelines and which files to read without touching events |
| Positions | `ord` table (`pos.rs`) | **Exact below a size threshold, estimated above** (user decision): time keyset paging, histogram-based scroll | Keyset; 1B on real segments not measured | Section 3.5 |
| Word index | Tantivy, mmap (`fts.rs`) | **Keep**, measure at 1B | Term dictionary | Tokeniser matches our needs (ADR 0014). `fts` extension rejected earlier; SeekStorm has far less use (crates.io downloads 2.5k against 5.2M in 90 days) |
| Search hits | Parquet spill (`hits.rs`) | **Keep; the pattern to copy** | `id IN (read_parquet)` | Bounded, already streaming |
| Analysis results | JSON blobs in `analysis_cache` (`analysis.rs:586`) | **Change** (audit Q-2): rows, or a Parquet file per large result | Paged SQL | A blob must be read whole |
| Fallback scan matches | `Vec<u64>` of all ids (`read.rs:538`, `:658`) | **Change** (Q-1): keyset paging, or spill as `hits.rs` does | Keyset | Memory grows with matches |
| Filtered pages | `count(*)` then `LIMIT … OFFSET` over the whole view (`read.rs:600-644`, SQL at `:628`) | **Change** (ST-2, ST-3): prune, count from the histogram where it can answer, keyset | Keyset | A deep `OFFSET` sorts everything before it: 8.2 s at 85M (section 3.3) |
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
| Parquet with row group statistics | Yes: file list, then row groups. Page index written but apparently unused by DuckDB (table above) | **Events, samples, large results** |
| Arrow IPC with a memory map | Yes by batch and array index; zero-copy if aligned ([FileDecoder](https://docs.rs/arrow-ipc/latest/arrow_ipc/reader/struct.FileDecoder.html)); no statistics | **Heap graph container** |
| Plain versioned arrays | Yes; 50 ns | **Heap graph fallback** |
| Vortex | Yes; "100×" is a vendor figure an independent audit calls a benchmark artefact, about 1.3× on ClickBench ([audit, secondary](https://mohakchugh.is-a.dev/blog/compute-pushdown-compressed-data-vortex-columnar-format)) | No: DuckDB's extension has no Windows build (checked: 404; [docs](https://duckdb.org/docs/2.0/core_extensions/vortex)) |
| Lance | Yes; vendor "100×" | No: 74 to 93 MB extension, third party, and crate majors move fast (14.0.2 now, 4.0 in April) |
| FlexBuffers | Yes for keys, binary search ([docs](https://flatbuffers.dev/flexbuffers/)) | Not needed: JSON documents are small and big results belong in rows |

## 5. Options, with manageability

| Option | Write 1B | Read at 1B | Live client access | Licence, health | Fit | Rank |
|---|---|---|---|---|---|---|
| **Segments + catalogue + histogram pruning + keyset** | Today's writers | Bundle shapes measured; 1B on real segments not measured | Parquet: yes. Catalogue: copy or close (below) | MIT, Apache; active | No new dependency; about 13.5 days (ST-1, 2, 3) | **1** |
| DuckLake over the segments | Same writers, then register | Uniform corpus only, at min/max selection; mixed segments not measured | DuckDB catalogue: as today. SQLite catalogue: a second process read a small test table while the first held it (measured). Clients need the extension | MIT, spec 1.1, [backward compatibility promised](https://ducklake.select/2026/04/13/ducklake-10/); DuckDB's [secondary tier](https://duckdb.org/docs/2.0/core_extensions/overview) | 13 to 17 MB per platform (compressed); promoted columns conflict; would own our files. **Out of 1.2 (user decision)** | **After 1.2** |
| Native tables, all events | About 83 min at the recorded 200k/s (one Appender; 355k/s with four), 8.7× larger | Within 1 to 3× of Parquet, faster, on the 563,220-event bake-off in ADR 0014; not re-run | **No while Debrief runs** (measured) | MIT | Rejected in ADR 0014 | 3 |
| Hybrid: fan-out, then convert | Adds conversion | Not measured | No, for converted data | MIT | Costs disk and time twice; the post-1.2 trial still measures it | 4 |
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
| DuckDB 1.5.6 client on a catalogue the product wrote (`STORAGE_VERSION` `v1.5.0`, `lib.rs:241`), views included | Works (read-only, and a copy with its WAL) |
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

**Events: no. This is reasoning from recorded numbers: no row format of ours was built or run.** A row format would give up column pruning (a page
reads a few of 13 columns), compression (the recorded `bench.rs` 3M corpus is about 365 bytes an event as CLEF text and 56 as Parquet,
[write paths](2026-10-29-duckdb-write-paths.md), derived there), and SQL with every client. It would also make us own a parser for hostile files. Parquet writing is
recorded at 4.38M events/s (same note), so there is little speed to win. An `.etlx`-style cooked format solves a problem Parquet does not have.

**Captures: measured against the caps in `limits.rs`** (20M objects, 100M references, 20M samples; generated; warm page cache; decimal MB). No budget applies to these yet, so they carry no verdict. Build: `clang -O2 harness/cooked.c -o cooked`; run: `harness/run2.sh`, `harness/reach.sh`.

| Operation | Plain arrays, mmap | Parquet + DuckDB |
|---|---|---|
| File size, references | 400 MB + 160 MB offsets | 410 MB (random ids barely compress) |
| File size, 20M samples | 400 MB | **111 MB** |
| One object's references | **50 ns**, footprint 1.5 MB | 10 to 47 ms, 17 MB |
| Reachability over the graph | **1.18 s**, 84 MB; reached 19.86M objects | **Did not finish:** recursive SQL ran out of memory at its 768 MB limit after 22 s, footprint 1,285 MB. Roots with no references return at once, which made the first version of this table wrong (below) |
| Hotspots by stack, time window, 5M of 20M | 19 ms, 3 MB | 47 to 79 ms, 115 MB |
| Validate on open (offsets ordered, every id in range) | **39 ms** | Done by the reader |

Reachability uses the same shape but not the same random edges. Correction: this note first quoted 19.2 s for SQL, but that harness dropped the result and started at an object with no references, so it reached one object. The rerun keeps the count (`harness/reach.sh`): start 0 reaches 1 in 8.8 s (the time is the scan of the file); start 7 fails as above. A heap graph cannot use recursive SQL here. RSS of the arrays peaked at
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
Hostile-input safety then has two parts: the importer stays as hardened as today, and the cache gets its check on open and a parser that
never panics. The check took 39 ms with the pages already in memory. It touches every offset and reference, so on a cold cache it reads about 560 MB first, and at open that gives up the laziness of the map: measure it cold in MEM-6a, or trust the file by a checksum written last. Written to a temporary name and renamed.

**mmap risks:** a truncated or changed file is [undefined behaviour](https://docs.rs/memmap2/latest/memmap2/struct.Mmap.html) and, in practice, a bus error, which is why section L8 of the
[DuckDB 2 note](../../../../docs/research/2026-10-duckdb-2-and-large-data.md) refused mmap for input files. Here the file is ours and immutable, but the map must still live **outside the UI process**, as
MEM-6 proposes. Windows cannot delete a mapped file; reuse the store's `trash` approach.

**MEM-6 recommendation:** approve the ADR and measurement (3 days), on these two formats. The 8 to 12 day build stays the user's call after the numbers, as already decided.
MEM-1 and MEM-2 are still needed first: they stop the crash risk regardless.

## 7. Audit findings

The ids are from the 2026-11-05 Debrief memory and query audit, which is not in the repo yet: MEM-1 lend captures by range, MEM-2 byte budget for open documents, MEM-6 ADR with measurement for samples in Parquet and a mapped heap graph outside the UI process; Q-1 narrow fallback scans and keyset paging, Q-2 pageable analysis results, Q-4 a rows-read test per view.

Consistent with this note; each maps to a package: analysis JSON blobs (Q-2), fallback scans keeping all ids (Q-1, `read.rs:538`), samples and heap graphs in the UI process
(MEM-6). "OFFSET paging" is real: `read.rs:628` has `LIMIT {limit} OFFSET {offset}` on filtered pages, at `b4bf8cb6d` and at `debrief/1.2-int` `852b35ac0`. The first version of this note said it had gone; that was a bad grep and is withdrawn. Q-1 should cover it, and section 3 shows what it costs.

## 8. Recommendation

Weighed against both requirements, a 1B target and opening the 14,000-file, 7.6 GiB bundle:

| Package | Bundle | 1B |
|---|---|---|
| Import that survives large files (another agent) | **Blocks it** | Needed |
| ST-1 histogram selection | Small gain: pages are within budget | Likely needed: binding the whole view costs 340 MiB on the uniform corpus; real mix not measured |
| ST-2 read paths use it | Small gain | Likely needed |
| ST-3 keyset, positions | **Needed for filtered deep pages** at 85M; the bundle's real events are fewer | **Needed:** `ord` over budget from below 100M |
| ST-5 product bench | Yes, once the import survives | **Decides the 1B answer** |

1. **Do ST-3's `OFFSET` part first.** It fixes the worst pages measured, at any size.
2. **Then ST-1 and ST-2 within Q-1, read paths first.** They pass the gate only through `histogram`; do not add min/max zone maps for this.
3. **ST-5 on real segments before calling 1B reachable.**
4. **DuckLake stays out of 1.2.** The trial runs after 1.2, sooner only if the corrected numbers show this approach cannot reach 1B. Its gate when it runs: promoted columns have a design (today [extra columns are refused or unreachable](https://ducklake.select/docs/stable/duckdb/metadata/adding_files)); per-import registration stays in the 768 MB budget; it passes on Windows; ownership of files works with Tantivy's segment keys and `segment_blocks` (ADR 0062); and it selects as few files as the histogram on mixed segments.
5. **Fallback** if the histogram grows too large at 1B: the narrower (file, minute) and (file, source) tables; then time-sorted compaction.

## 9. Work packages

Sizes are judgement until an ADR exists. Budgets follow ADR 0014; every number goes to `performance.md` (rule 48). Parallelism and cost: ST-1 and ST-2 are the "cheaper" answer of rule 51; no thread counts change.

| WP | What | Owns | Test first, done when | Days |
|---|---|---|---|---|
| ST-1 | **File selector from `histogram`** for time, source and level conjuncts; no new columns, no `FORMAT_VERSION` bump. Untimed rows (`minute` NULL) and files lacking histogram rows always selected | `read.rs` or a new `prune.rs`, `sql.rs` | On generated stores, the selected files are a superset of the files holding matching rows; the gate in 3.2 passes on both bundle shapes | 1.5 |
| ST-2 | Route the paths that have those conjuncts through it: `read.rs` 20 uses and `pos.rs` 4 first (2.5 days), then `analysis.rs` 17, `anomalies.rs` 10, `lanes.rs` 3, `spans`, `qlang`, `context`, `catalogue` 2 each (2 days). Cache `ctx()` per generation (0.5). Counts from `histogram` where the filter is only those conjuncts. Text filters keep the full list | as named | Differential test against the view; a rows-read test (audit Q-4) | 5 to 6 |
| ST-3 | ADR, then keyset paging for filtered and importing pages (replacing `OFFSET`), histogram-based scroll, `ord` below the threshold, estimates above, and a view holding about 200 rows (user decisions). The ADR settles the threshold against the 768 MB budget | `pos.rs`, `read.rs`, `ports`, grid | Anchor, jump to time and count stay right; no `ord` build above the threshold; deep page within 0.5 s at 85M | 6 |
| ST-4 | Analysis results as rows or Parquet (is audit Q-2) | `analysis.rs` | Page of a large result reads one page | in Q-2 |
| ST-5 | Product bench at 100M and 1B: real import on real shapes, Tantivy size and rate, cold reads, worker RSS per phase, time-sorted compaction. A quiet machine with about 200 GB free | `tests/bench*`, generator | Results block; verdict on 1B; per-source index decision | 3 |
| ST-6 | `Open in DuckDB…`: script of documented views with relative paths, worker-made catalogue snapshot, a schema page | `console/`, `project.rs`, app command, docs | The script reads the right rows after a source was removed and with the store moved | 2 |
| MEM-6a | ADR and measurement: samples, frames and stacks as Parquet; heap graph as IPC and as arrays; dominators on the map; cold validation; Linux PSS and Windows working set under each watcher | `debrief-profile`, notes | Numbers for the user's build decision | 3 (audit's) |

New days: about 17.5 to 18.5 (ST-1, 2, 3, 5, 6), up from 12.5. ST-1, ST-2 and ST-3 touch files Q-1 also owns, so they should be one agent, after the 1.2 base settles.

## 10. Risks

| Risk | Effect | Mitigation |
|---|---|---|
| Real events differ from these stores (EVTX, large files are missing) | Selection changes | ST-5 on the bundle's invented shapes once the import survives them |
| Generated events are shorter than real ones | Absolute times optimistic, footers bigger with promoted columns | Ratios guide; ST-5 sets the numbers |
| `histogram` is large at 1B | Selection slower, catalogue larger | Measure; narrower tables are the fallback |
| Selected list misses a file | Wrong rows | Superset test in ST-1; differential test in ST-2; keep the `READY` filter on every path |
| Estimated positions above the threshold | A scrollbar that is not exact | ADR; the user accepted this above about 100M |
| Tantivy at 1B | Search ready long after import | Measure; per-source or lazy index |
| DuckDB 2.0.0 is an x.0 | Regressions | No 2.0.1 yet ([releases](https://github.com/duckdb/duckdb/releases)); the crate has 1.20000.1 while the lockfile has .0: check it |

## 11. Decisions for the user

Settled 2026-11-05: 1B is a real 1.2 target; exact positions below about 100M with estimates above; a view holds about 200 rows; the bundle must open; DuckLake is out of 1.2.

1. Accept "keep Parquet, catalogue and Tantivy, prune by the histogram, replace deep `OFFSET`" and add ST-1, 2, 3, 5 and 6 to Debrief 1.2 (about 18 days)?
2. The 100M threshold breaks the 768 MB import budget (905 MiB). Lower the threshold, raise the budget for this one step, or have ST-3's ADR look for a cheaper ordering first? Suggested: the last, with a lower threshold as the fallback.
3. MEM-6: ADR and measurement on Parquet for samples and an Arrow IPC or plain array cache for heap graphs, as above?

## 12. Sources

Read 2026-11-05 unless noted.

- DuckDB: [concurrency](https://duckdb.org/docs/2.0/connect/concurrency), [migration](https://duckdb.org/docs/2.0/operations_manual/migration), [storage](https://duckdb.org/docs/2.0/internals/storage), [2.0 announcement](https://duckdb.org/2026/10/21/announcing-duckdb-200), [schemas across files](https://duckdb.org/docs/2.0/data/multiple_files/combining_schemas), [Vortex extension post](https://duckdb.org/2026/01/23/duckdb-vortex-extension.html) (TPC-H, single laptop: 12 to 18% faster than Parquet v2).
- DuckLake: [docs](https://ducklake.select/docs/stable/), [1.0](https://ducklake.select/2026/04/13/ducklake-10/), [1.1](https://ducklake.select/2026/10/21/ducklake-11/) (vendor benchmarks), [adding files](https://ducklake.select/docs/stable/duckdb/metadata/adding_files), [maintenance](https://ducklake.select/docs/stable/duckdb/maintenance/recommended_maintenance).
- Formats: [Vortex](https://github.com/vortex-data/vortex), [Lance](https://github.com/lance-format/lance), [chDB](https://github.com/chdb-io/chdb), [sqlite-columnar](https://github.com/sqliteai/sqlite-columnar), [Arrow versioning](https://arrow.apache.org/docs/format/Versioning.html), [Parquet page index](https://parquet.apache.org/docs/file-format/pageindex/), [Tantivy](https://github.com/quickwit-oss/tantivy), [fjall](https://github.com/fjall-rs/fjall), [rust-rocksdb](https://github.com/rust-rocksdb/rust-rocksdb).
- Licences and versions: crates.io and GitHub APIs, the same day. Extension sizes: `extensions.duckdb.org/v2.0.0`, `Content-Length`, the same day.
- Secondary, not checked further: the Vortex audit above; deepwiki pages on PerfView were not relied on.
- Repo: ADR 0010, 0014, 0023, 0030, 0062; [write paths](2026-10-29-duckdb-write-paths.md); [DuckDB 2 note](../../../../docs/research/2026-10-duckdb-2-and-large-data.md).
