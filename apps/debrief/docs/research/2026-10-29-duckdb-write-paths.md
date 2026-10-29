# DuckDB write paths for bulk event import

Status: research note; write results measured, scaling and per-method query tables pending. Date: 2026-10-29.
Owner: Debrief storage (decryp7). Machine: Mac mini (M4 Pro, 14 cores, 64 GB), DuckDB 2.0, unless a row says otherwise.

Question (user): how do we get events into DuckDB fast, compared with the other ways of writing: row inserts, one
Appender, several Appenders, one Parquet writer, many Parquet writers, and the Quack server? Is writing straight into
the database the normal way, is writing separate Parquet files faster, and does DuckDB 2.0 bring a faster file format?

**Answer: keep ADR 0014's fan-out.** Writing into a native table with `INSERT` or the Appender is the normal way and the
right one for data that is edited, but Appenders scale poorly: our two benches disagree on how much (1.0× for a second
Appender on the writer bench, 1.8× for four in the Quack harness at about twice the memory), and the native write path
spends its time compressing the long `raw` strings. For bulk, append-only events, each parse thread writes its own zstd Parquet
file and the `.duckdb` catalog keeps a `VIEW` over `read_parquet([...], union_by_name = true)`: 1.3–1.55M events/s in a
real import on a quiet Mac, against about 387k events/s for one Appender on the writer bench. DuckDB 2.0 brings storage
format v2.0.0 for the native file, not a new exchange format; Parquet is still the format for writing in parallel.

## Context

- [ADR 0014](../adr/0014-database-first-engine.md) §"Implementation notes (R2b, 2026-10-20)" chose zstd Parquet segments
  plus a small native catalog after a storage bake-off on the Windows VM. This note checks that decision on DuckDB 2.0
  and the Mac mini, and adds the server mode.
- [2026-10-28-duckdb-server-mode.md](2026-10-28-duckdb-server-mode.md) measured the Quack server against today's design;
  its conclusion (do not migrate) is recorded as an ADR 0010 amendment that is not yet merged.
- [docs/research/2026-10-duckdb-2-and-large-data.md](../../../../docs/research/2026-10-duckdb-2-and-large-data.md) covers
  the 2.0 migration, the storage version pin and VARIANT.

## The three architectures

The same 8 parse threads feed each design. The difference is where the threads have to wait in line.

![Three write architectures: A, parse threads queue into one Appender that writes events.duckdb; B, each parse thread
writes its own Parquet segment and a view in catalog.duckdb reads them; C, clients send rows over the Quack protocol to a
server process.](img/duckdb-write-architectures.svg)

- **A, single writer into a native table** (purple): `INSERT` or the Appender, Debrief's import before R2b
  (2026-10-20). The line forms at one writer; DuckDB compresses the rows as it writes them, and string compression limits
  the speed.
- **B, fan-out** (teal): one Parquet file per thread and a view over them, Debrief's import today (ADR 0014). No shared
  lock while writing. Segments are written by the `parquet` crate's `ArrowWriter` (arrow-rs), not by DuckDB, to a
  temporary name and renamed when complete (`crates/debrief-store-duck/src/segments.rs`).
- **C, server mode** (blue): clients send rows over the Quack protocol to a server process; a network hop and a copy per
  client. Evaluated 2026-10-28 and rejected (ADR 0010 amendment, not yet merged).

## Test setup

Every corpus is synthetic (Paddock ER-14; never a user's logs). Where a detail is not in the repo, it says "not recorded".

### Runs

| Run | Date | Machine, load | Build | Corpus | Command or harness |
|---|---|---|---|---|---|
| R2b bake-off | 2026-10-20 | Windows VM (24 cores, shared), ±30 % | release; DuckDB 1.x (exact version not recorded in the bake-off) | `/samples` × 20: 563,220 events, 195 MB of logs | `DEBRIEF_BENCH_COPIES=20 cargo test --release --test it layout_bakeoff:: -- --ignored --nocapture` in `crates/debrief-store-duck` ([layout_bakeoff.rs](../../crates/debrief-store-duck/tests/it/layout_bakeoff.rs)) |
| Writer bench | 2026-10-28/29 | Mac mini, busy (load 33–65 for most runs) | release, DuckDB 2.0 | pre-parsed rows from a small sample (size not recorded) | not recorded in the repo; results block pending (rule 48, adopted, not yet merged) |
| Quack harness | 2026-10-28 | Mac mini, load 17–40 at start (each run waited for load < 40) | release; duckdb-rs `=1.20000.0`, prebuilt libduckdb 2.0.0, `parquet`/`arrow-ipc` 60 | `bench.rs` corpus, 3,000,000 events (1,095 MB of CLEF), converted to Arrow IPC in 8,192-row batches | [harness/](2026-10-28-duckdb-server-mode/harness/) `bench.sh import`; see [2026-10-28-duckdb-server-mode.md](2026-10-28-duckdb-server-mode.md) §2 |
| Real import, at load | 2026-10-28 | Mac mini, load 33–65 | release CLI and worker, branch `debrief/duckdb-2` (08cf9f036) | `scripts/perf/gen_large.py` scale 0.3: 16,847,461 events, 2.42 GB | 3 interleaved pairs; see [performance.md](../performance.md) "2026-10-28, DuckDB 2.0 against 1.5.6" |
| Real import, quiet | 2026-10-28/29 | Mac mini, quiet (load not recorded) | release, DuckDB 2.0 | not recorded | not recorded in the repo; results block pending |

Machine details from [performance.md](../performance.md): the Mac mini is an M4 Pro with 14 cores and 64 GB, shared with
other agents' builds. Disk and macOS version: not recorded.

### Schemas

The event table (`events_empty` in [schema.rs](../../crates/debrief-store-duck/src/schema.rs), the shape of every
segment and of the native table in the bake-off):

| Column | Type | Null | Notes |
|---|---|---|---|
| `id` | `UBIGINT` | not null | dense per segment |
| `ts` | `BIGINT` | nullable | |
| `ts_state`, `level` | `UTINYINT` | not null | |
| `source` | `UINTEGER` | not null | |
| `line` | `UINTEGER` | not null | |
| `byte_offset` | `UBIGINT` | not null | |
| `message` | `VARCHAR` | not null | long string |
| `template`, `exception` | `VARCHAR` | nullable | long strings (stack traces) |
| `markers` | `UBIGINT` | not null | bit set |
| `raw` | `VARCHAR` | not null (nullable in the bake-off DDL) | the longest string: 190 of the 215 MB of text in the R2b corpus |
| declared columns | per profile | | added by the profile's parsers |
| `extra` | `MAP(VARCHAR, VARCHAR)` | not null | free-form fields |

- **Parquet segments** store these columns plus promoted `p_<key>` columns, which a segment picks from its first batch
  (`crates/debrief-store-duck/src/promote.rs`), so segments can differ in their columns.
- **Native table in the Quack harness**: `CREATE OR REPLACE TABLE events (...)` generated from the segments' Arrow schema
  (the same columns, promoted ones included, with no `NOT NULL`), `harness/src/main.rs` `ddl()`.
- **Catalog** (`ddl()` in `schema.rs`): `meta`, `sources`, `parsers`, `diagnostics`, `marker_kinds`, `segments` (`file`,
  `first_id`, `end_id`, `rows`, `live`, `indexed`, `promoted`), `segment_fields`, `histogram`, `trash`, `ord`.
- **Views** (`events_view()` in `schema.rs`), over the absolute paths of the listed segments:

```sql
CREATE OR REPLACE VIEW events AS
  SELECT <event columns> FROM read_parquet(['…/seg-….parquet', …], union_by_name = true)
  WHERE source IN (SELECT id FROM sources WHERE ready);
CREATE OR REPLACE VIEW events_wide AS
  SELECT * FROM read_parquet([…], union_by_name = true)
  WHERE source IN (SELECT id FROM sources WHERE ready);
```

The Quack harness uses `CREATE VIEW events AS SELECT * FROM read_parquet('<dir>/*.parquet', union_by_name = true)`.

### Corpus sizes

| Corpus | Events | Raw | Parquet | Native `.duckdb` | Bytes per event (raw / Parquet) |
|---|---|---|---|---|---|
| R2b, `/samples` × 20 | 563,220 | 195 MB | 36.7 MB | 318 MB | about 346 / 65 (derived: size ÷ events) |
| `bench.rs`, 3M | 3,000,000 | 1,095 MB CLEF | 169 MB (28 segments) | not recorded | about 365 / 56 (derived) |
| `gen_large.py` 0.3 | 16,847,461 | 2.42 GB | not recorded | not recorded | not derived |

Average row width in memory: not recorded.

### Writer settings

| Setting | Product (`segments.rs`, `lib.rs`) | R2b bake-off | Quack harness |
|---|---|---|---|
| Batch from the parse threads | 8,192 rows (`BATCH_ROWS`, `crates/debrief-batch/src/lib.rs`) | 8,192 (the same batches) | 8,192 |
| Segment rotation | 131,072 rows (16 batches), or after 2 s (`SEGMENT_AGE`) | about 131k rows (16 batches) | 131,072 rows |
| Parquet row group | 65,536 rows | 122,880 rows | 65,536 rows |
| Compression | zstd, `ZstdLevel::default()` = level 1 in `parquet` 58.4.0 | zstd level 1 | zstd, default level |
| Writer threads | parse threads, `min(cores − 1, 8)` (`default_threads()`, `crates/debrief-core/src/ingest/parallel.rs`), raisable in Settings | 1 and 8 | 1, 8, 12 |
| DuckDB `memory_limit`, `threads` | 192 MB, 4 (low-memory mode 128 MB) | 256 MB, 4 | 192 MB, 4 (Quack clients 256 MB, 2) |
| Storage version | main: DuckDB 1.5.6's default; the 2.0 branch pins `v1.5.0` (WP-D3 in the DuckDB 2.0 note) | not recorded | DuckDB 2.0 default |
| Appender | none: events are not appended to a table | one long-lived Arrow appender, one transaction, checkpoint once; also 2/4/8 appenders on own connections | `append_record_batch` per 8,192-row batch, one flush at the end; 1 or 4 appenders |

## Measured results

Two kinds of run are mixed here, so compare within a kind. The **writer bench** writes pre-parsed rows from a small
sample, so it shows the ceiling of each writer. The **real import** includes reading and parsing the logs. The R2b
bake-off is older code and data on another machine and is not like-for-like with the 2.0 figures.

![Write throughput by method, events per second on a linear scale: Quack server with 1 client 115k; Appender, 1
long-lived 387k; Parquet, 1 writer 725k; real import with 8 writers at load 0.7–1.0M and on a quiet machine 1.3–1.55M;
Parquet, 8 writers, small sample 5.1M.](img/duckdb-write-throughput.svg)

For the two import bars, the solid end marks the measured range and the outline runs from zero to its low end.

| Method | Writers | Events/s | Memory | Run kind | Date | Machine, load | Source |
|---|---|---|---|---|---|---|---|
| `INSERT` row by row | 1 | not measured | | | | | |
| Appender, 1 long-lived | 1 | ≈387k | +526 MB peak | writer bench | 2026-10-28/29 | Mac mini, load 33–65 | results block pending |
| Appender, 2 | 2 | 1.0× of 1 Appender | not recorded | writer bench | 2026-10-28/29 | Mac mini, load 33–65 | results block pending |
| Appender, native table | 1 | 36.5k (60k with `raw` NULL) | +429 MB | R2b bake-off | 2026-10-20 | Windows VM, ±30 % | ADR 0014 bake-off |
| Appenders, native table | 2/4/8 | 61–75k (no scaling) | not recorded | R2b bake-off | 2026-10-20 | Windows VM, ±30 % | ADR 0014 bake-off |
| Appender, in process | 1 / 4 | 200k / 355k (medians) | 845–892 / 1,828–1,852 MB footprint | Quack harness | 2026-10-28 | Mac mini, load < 40 | server-mode note §3.1 |
| Parquet, 1 writer | 1 | ≈725k | +2 MB | writer bench | 2026-10-28/29 | Mac mini, load 33–65 | results block pending |
| Parquet, 1 writer | 1 | 261–409k | +50 MB peak | R2b bake-off | 2026-10-20 | Windows VM, ±30 % | ADR 0014 bake-off |
| Parquet, 8 writers | 8 | 5.1M | not recorded | writer bench, small sample | 2026-10-28/29 | Mac mini, load 33–65 | results block pending |
| Parquet, 8 / 12 writers | 8 / 12 | 4.38M / 4.41M (medians) | 734–814 / 899–928 MB footprint | Quack harness | 2026-10-28 | Mac mini, load < 40 | server-mode note §3.1 |
| Parquet, 8 writers | 8 | 0.97–1.7M | +78 MB peak | R2b bake-off | 2026-10-20 | Windows VM, ±30 % | ADR 0014 bake-off |
| Import, 8 writers, at load | 8 | 0.7–1.0M (685k–991k) | worker peak 905–945 MB | real import | 2026-10-28 | Mac mini, load 33–65 | performance.md, DuckDB 2.0 against 1.5.6 |
| Import, 8 writers, quiet | 8 | 1.3–1.55M | not recorded | real import | 2026-10-28/29 | Mac mini, quiet | results block pending |
| Quack server | 1 client | ≈115k (110k–124k) | client 1,337–1,370 + server 2,033–2,071 MB footprint | Quack harness | 2026-10-28 | Mac mini, load < 40 | server-mode note §3.1 |
| Quack server, 1 GB limit | 4 clients | 416k (394k–433k) | about 4.6 GB, client plus server | Quack harness | 2026-10-28 | Mac mini, load < 40 | server-mode note §3.1 |
| Quack server, 192 MB limit | 4 clients | out of memory in 2 of 3 rounds | server 1,047–2,002 MB | Quack harness | 2026-10-28 | Mac mini, load < 40 | server-mode note §3.1 |
| DuckLake | many | not measured | | | | | |

In the Quack runs `memory_limit` covers only DuckDB's buffer manager, so the processes grow well past it.

### Side by side

| Method | Writers that help | Updates and deletes | Use it for |
|---|---|---|---|
| `INSERT` row by row | 1 | easy, transactional | settings, small edits, user notes |
| Appender into a native table | 1; the benches disagree: 1.0× for a 2nd (writer bench), 1.8× for 4 at about 2× memory (Quack harness) | easy, transactional | moderate volumes that will be edited |
| One Parquet writer | 1 | rewrite the file | exports, one-off loads |
| Parquet per thread + view | `min(cores − 1, 8)` | by a deletion list, then compaction | **bulk append-only events (Debrief)** |
| Quack server | 4 help, at a large memory cost | easy, transactional | remote clients; not for Debrief |
| DuckLake | many, with a server catalog database | snapshots, delete files | if Debrief ever needs time travel or many writers on one store |

## Scaling

Measured so far:

- **Parquet writers scale close to linearly up to 8 on the writer bench**: 725k events/s with 1 writer, 5.1M with 8,
  about 7×. On the Quack harness, 12 writers gave no more than 8 (4.41M against 4.38M).
- **Appenders scale poorly, and the two benches disagree on how much.** Writer bench: 1.0× for a second Appender. Quack
  harness: 1.8× for four (200k to 355k events/s) at about twice the memory (1.83 GB footprint at a 192 MB limit). R2b on
  the Windows VM: 2/4/8 appenders 61–75k events/s, with no gain from 2 to 8. None of these is linear.
- **The real import is bounded by parsing**, not by the writers: 1.3–1.55M events/s quiet against 5.1M for the writers
  alone.
- **Data size**: no measured curve yet. R2b's 10M-event run (ADR 0014 "Measured (R2b, release, the same VM)") gives one
  point: 123k events/s end to end, peak RSS 415 MB, write-side reads under 0.5 s.

> **Placeholder, to be filled from the scaling runs (in progress, not measured yet).**

| Writers | 1 | 2 | 4 | 8 | 12 |
|---|---|---|---|---|---|
| Parquet events/s | pending | pending | pending | pending | pending |
| Appender events/s | pending | pending | pending | pending | pending |
| Peak memory | pending | pending | pending | pending | pending |

| Corpus size (events) | Write time | Size on disk | Query p50 | Query p95 |
|---|---|---|---|---|
| pending | pending | pending | pending | pending |

## Query performance by write method

Fast writes only help if the queries stay fast. Measured so far:

- R2b (Windows VM, 563,220 events, cold/warm): queries over Parquet were 1–3× slower than over a native table, for
  example a page in 158–180 ms against 64–99 ms (ADR 0014 bake-off).
- DuckDB 2.0 against 1.5.6 over the Parquet view (3M events, nine queries): 0.56–0.66 s wall with `async_threads` 4
  against 0.76–1.09 s ([performance.md](../performance.md), 2026-10-28).
- Quack harness, Parquet view in process (3M events, DuckDB 2.0, cold/warm): page of 500 rows 13.6/12.8 ms, facet
  110.0/110.3 ms, 1000-bucket histogram 34.7/33.9 ms, bulk fetch of 100,000 rows 94.1/85.3 ms (server-mode note §3.2).

> **Placeholder: cells marked "pending" are being measured; do not quote them.**

| Write method | Write time | Size | Peak RSS | Query p50/p95 warm | Query p50/p95 cold |
|---|---|---|---|---|---|
| a) Parquet segments + view | pending (R2b: 0.97–1.7M events/s, 8 writers) | pending (R2b: 36.7 MB) | pending (R2b: +78 MB) | pending (R2b page: 158–180 ms) | pending |
| b) Compacted Parquet | pending | pending | pending | pending | pending |
| c) Native, storage v1.5.0 | pending (R2b, DuckDB 1.x: 36.5k events/s) | pending (R2b: 318 MB) | pending (R2b: +429 MB) | pending (R2b page: 64–99 ms) | pending |
| d) Native, storage v2.0.0 | pending | pending | pending | pending | pending |
| e) Hybrid: fan-out, then converted to native (incl. conversion time) | pending | pending | pending | pending | pending |
| f) Quack server | pending (115k events/s, 1 client) | pending | pending | pending | pending |
| g) DuckLake | pending | pending | pending | pending | pending |
| h) Vortex | pending | pending | pending | pending | pending |

The R2b figures in brackets are Windows VM, older code and data, and not like-for-like with the rows that replace them.

## Trade-offs

Why fan-out wins for appends:

- **Nothing shared while writing.** DuckDB allows parallel appends, but in our runs extra Appenders scaled poorly (1.0× for
  a 2nd on the writer bench, 1.8× for 4 at about 2× memory in the Quack harness); R2b traced the limit to string compression on the native write path. Separate files share nothing until the view reads
  them.
- **Compression runs in every thread.** The segments are written by the Rust `parquet` crate, not by DuckDB, so DuckDB's
  memory limit does not apply to them. Each writer encodes and zstd-compresses its own row groups, so compression scales
  with the threads instead of sitting on one path.
- **Each file is written once.** A segment goes to a temporary name and is renamed when complete, so a crash leaves
  either the whole file or nothing.
- **Reads are slower, but acceptable.** DuckDB scans Parquet files in parallel and skips row groups by their min/max
  statistics. R2b still measured queries over Parquet 1–3× slower than over a native table.

What fan-out costs:

- **Many small files.** A background job merges small segments; otherwise file opens slow the queries down.
- **Edits are indirect.** Deletes go to a deletion list that the view filters, and compaction removes them for good. Use
  a native table where data is edited often.
- **Schema drift between segments.** `union_by_name = true` lines up columns by name and fills the missing ones with
  null.
- **The catalog is the source of truth.** A segment counts only once it is in the catalog, so a half-written file is
  never read.

## File formats in DuckDB 2.0

- **Storage format v2.0.0** applies to the native `.duckdb` file: DICT_FSST string compression and compact deletes. It
  makes that file smaller and faster, but it is not a file you write in parallel. Debrief's catalog stays pinned at
  v1.5.0 for now (plan: Later); the events are in Parquet, so v2 would only shrink the small catalog.
- **Parquet** is still the format for writing in parallel. DuckDB reads and writes it natively, and every segment is a
  standard file other tools can open.
- **DuckLake** is a table format, not a new file format. It is a catalog (tables, snapshots, delete files) over Parquet
  files, the same idea as Debrief's segment catalog in standard form.
- **VARIANT** arrived in 1.5; 2.0 adds shredded storage and shredded Parquet reading and writing. It could hold Debrief's
  free-form event fields; adopting it is planned for Later.
- Other columnar formats exist (Arrow IPC, Vortex, Lance), but DuckDB 2.0 did not add a new built-in one, and none were
  measured here.

## Memory during an import

- **The Parquet writers use little memory** (+78 MB peak for 8 in R2b) and do not go through DuckDB's buffer manager, so
  a larger DuckDB memory limit would not speed up writing.
- **The steps after writing do use memory:** Tantivy indexing, aggregates and compaction. A larger budget for the length
  of the import job helps there, and fewer of those steps spill to disk.
- **DuckDB keeps unpinned blocks cached** up to its memory limit. That cache is the likely source of the extra 190 MB
  seen after an import.
- **Plan, to be measured:** raise the memory limit when the import job starts, then lower it when the job ends so DuckDB
  evicts its cache. DuckDB's allocator flush settings apply to its jemalloc builds on Linux; how much memory goes back to
  macOS and Windows is still to be measured. Two numbers decide it: the time saved, and the memory held once the import
  is over.

## Recommendation

1. Keep the fan-out (ADR 0014): one zstd Parquet segment per parse thread, a native catalog and a view. No change to the
   write path.
2. Keep native tables for data that is edited (settings, notes, small user data) and for the catalog.
3. Do not adopt the Quack server for local writes (see the server-mode note).
4. Leave storage v2.0.0 and VARIANT for Later; neither changes the write path.
5. Measure the memory plan above before changing the import's memory limit.

Fallback: if the segment count or edit volume grows past what compaction handles, evaluate DuckLake, which keeps the
Parquet write path and standardises the catalog.

## Open questions

- Quiet re-runs of three writer-bench cells are scheduled; the bench numbers above were taken at load 33–65.
- Commit the writer bench and the quiet real-import runs with their commands, so their rows get a source in the repo.
- The scaling and per-method query tables above are placeholders until those runs finish.
- How much memory DuckDB returns to macOS and Windows after lowering `memory_limit`.
- The cause of the extra 190 MB held after a 2.0 import.

## References

Repo (paths from `apps/debrief`):

- [ADR 0014](../adr/0014-database-first-engine.md), "Implementation notes (R2b, 2026-10-20)", "Storage bake-off
  (decision)" and "Measured (R2b, release, the same VM)".
- [2026-10-28-duckdb-server-mode.md](2026-10-28-duckdb-server-mode.md) §2, §3.1, §3.2, and its
  [harness](2026-10-28-duckdb-server-mode/harness/) and [results](2026-10-28-duckdb-server-mode/results/).
- [performance.md](../performance.md), "2026-10-28, DuckDB 2.0 against 1.5.6" and "2026-10-20, store bake-off and
  R2a/R2b/R2c".
- [docs/research/2026-10-duckdb-2-and-large-data.md](../../../../docs/research/2026-10-duckdb-2-and-large-data.md),
  storage version and VARIANT.
- Code: `crates/debrief-store-duck/src/segments.rs`, `src/schema.rs`, `src/lib.rs`, `tests/it/layout_bakeoff.rs`,
  `tests/bench.rs`; `crates/debrief-core/src/ingest/parallel.rs`; `scripts/perf/gen_large.py`.
- Full results blocks go into [performance.md](../performance.md) and
  [docs/benchmarks.md](../../../../docs/benchmarks.md) under the new benchmark rule (rule 48, adopted, not yet merged).

DuckDB (official, read 2026-10-28):

- DuckDB 2.0.0 announcement: https://duckdb.org/2026/10/21/announcing-duckdb-200
- Migration guide 2.0 (storage versions): https://duckdb.org/docs/2.0/operations_manual/migration.html
- VARIANT type: https://duckdb.org/docs/2.0/sql/data_types/variant.html
- Quack remote protocol: https://duckdb.org/docs/2.0/quack/overview
