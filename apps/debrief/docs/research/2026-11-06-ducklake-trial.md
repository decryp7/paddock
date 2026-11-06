# DuckLake trial for Debrief's store (2026-11-06)

Status: research note; trial run 2026-11-07 01:15 to 03:20, within its budget. Decision for the user and the coordinator: see
[Recommendation](#recommendation). Results block (rule 48): [performance.md, "2026-11-06, DuckLake
trial"](../performance.md#2026-11-06-ducklake-trial-branch-docsdebrief-ducklake-trial). Harness and scripts:
[2026-11-06-ducklake-trial/](2026-11-06-ducklake-trial/).

Question (queued 2026-10-30, asked again 2026-11-06): would DuckLake, DuckDB's lakehouse table format (Parquet data files plus
a SQL catalog with snapshots), be a better store for Debrief's events than what Debrief 1.2 keeps: Parquet segments that the
parse threads write themselves, listed in a DuckDB catalog (2.0 storage format) and read through a `read_parquet` view
(`crates/debrief-store-duck/src/writer.rs`, `segments.rs`, `compact.rs`, `skew.rs`, `schema.rs`)? Measured: import speed,
querying while an import runs, disk use, undo of an import and a time shift on about 10% of the rows, and physical footprint.

## Facts (sources read 2026-11-06)

Verified locally means: run with the DuckDB 2.0.0 CLI (`v2.0.0 (Cyanoptera) d54cbbe3df`) or the trial harness, on this
machine, on invented data.

| Topic | Fact | Source |
|---|---|---|
| Current release | DuckLake **1.1** (specification 1.1), released 2026-10-21 together with DuckDB 2.0.0; the extension shipped for DuckDB 2.0 reports `extension_version 417a9117f5` and writes `version = 1.1` into a new catalog (verified locally). DuckLake 1.0 came out 2026-04-13 with DuckDB 1.5.2. The release calendar names the DuckDB 2.0 extension "2.0" and the spec "1.1"; the blog calls the release "DuckLake 1.1". | [DuckLake 1.1](https://ducklake.select/2026/10/21/ducklake-11/), [DuckLake 1.0](https://ducklake.select/2026/04/13/ducklake-10/), [release calendar](https://ducklake.select/release_calendar.html) |
| Stability | 1.0 is "a production-ready release with guaranteed backward-compatibility"; the 1.1 extension "keeps reading and writing DuckLake 1.0 catalogs"; moving a catalog to the 1.1 spec is opt-in (`AUTOMATIC_MIGRATION`). DuckLake 2.0 (branching, protected snapshots, REST, roles) is a roadmap item without a date. | [FAQ](https://ducklake.select/faq), [1.1 post](https://ducklake.select/2026/10/21/ducklake-11/), [roadmap](https://ducklake.select/roadmap) |
| Licence | MIT for the specification and the `ducklake` extension (allowed by Paddock's deny.toml baseline). | [ducklake.select](https://ducklake.select/), [github.com/duckdb/ducklake](https://github.com/duckdb/ducklake) |
| DuckDB 2.0 and duckdb-rs | DuckDB 2.0.0 published 2026-10-21 (LTS to at least October 2027); duckdb-rs 1.20000.0 (2026-10-22) is the crate Debrief pins (1.20000.1 followed on 2026-10-28). The harness loaded the extension through duckdb-rs 1.20000.0 against the official prebuilt `libduckdb` 2.0.0 that Debrief links (verified locally, see the results block). | [DuckDB 2.0.0](https://duckdb.org/2026/10/21/announcing-duckdb-200), [crates.io duckdb](https://crates.io/crates/duckdb), `apps/debrief/Cargo.toml` on `debrief/1.2-int` |
| Offline loading | Works: `SET autoinstall_known_extensions = false; SET autoload_known_extensions = false; LOAD '<absolute path>/ducklake.duckdb_extension'` loads the file with no network and without `allow_unsigned_extensions` (DuckDB checks the file's signature against its built-in keys). A relative path is refused on macOS ("relative path not allowed in hardened program"). Verified locally. Extension sizes for v2.0.0: osx_arm64 12.7 MB gzipped / 43.2 MB unpacked, windows_amd64 15.5 MB gzipped, linux_amd64 17.2 MB gzipped (HTTP `Content-Length`, 2026-11-06). A SQLite catalog also needs `sqlite_scanner` (41.9 MB unpacked on osx_arm64). Statically linking it would need a source build of libduckdb, which Debrief avoids (`debrief-store-duck/Cargo.toml`: never `bundled`). | [installing extensions](https://duckdb.org/docs/2.0/extensions/installing_extensions.html), `http://extensions.duckdb.org/v2.0.0/<platform>/ducklake.duckdb_extension.gz` |
| Catalog options | DuckDB file: "local data warehousing with a single client"; "only one process can attach it at a time unless every process attaches it with `READ_ONLY`"; several connections of one process may share it. SQLite: "multiple local clients", one writer at a time, DuckLake retries lock conflicts. PostgreSQL for multi-user; MySQL not recommended. | [choosing a catalog database](https://ducklake.select/docs/stable/duckdb/usage/choosing_a_catalog_database) |
| Concurrency | Optimistic: snapshot ids are sequential; two writers that take the same id collide on the primary key, and DuckLake retries the metadata commit without rewriting data files when the changes do not conflict logically (concurrent inserts into one table do not conflict; deletes from different files do not). Readers see committed snapshots. | [conflict resolution](https://ducklake.select/docs/stable/duckdb/advanced_features/conflict_resolution) |
| Snapshots, time travel, rollback | Every commit is a snapshot; `AT (VERSION => n)` / `AT (TIMESTAMP => t)` per table, `ATTACH ... (SNAPSHOT_VERSION n)` for the whole lake. There is **no revert-to-snapshot command**: the 2.0 extension's functions are `ducklake_add_data_files`, `_cleanup_old_files`, `_commit`, `_current_snapshot`, `_delete_orphaned_files`, `_expire_snapshots`, `_flush_inlined_data`, `_last_committed_snapshot`, `_list_files`, `_merge_adjacent_files`, `_options`, `_rewrite_data_files`, `_scan`, `_set_commit_message`, `_set_option`, `_settings`, `_snapshots`, `_table_changes`, `_table_deletions`, `_table_info`, `_table_insertions` (verified locally). Undo is a new commit (DELETE or UPDATE); `ROLLBACK` undoes only an open transaction. | [time travel](https://ducklake.select/docs/stable/duckdb/usage/time_travel), local `duckdb_functions()` |
| Compaction, small files | `ducklake_merge_adjacent_files` merges small files, `ducklake_rewrite_data_files` rewrites files with many deletes, `ducklake_expire_snapshots` then `ducklake_cleanup_old_files` remove old files ("DuckLake also never deletes old data files" until then); data inlining (default 10 rows) keeps tiny commits in the catalog. Maintenance can run through `CHECKPOINT`. | [recommended maintenance](https://ducklake.select/docs/stable/duckdb/maintenance/recommended_maintenance), [1.0 post](https://ducklake.select/2026/04/13/ducklake-10/) |
| Schema evolution | Add, drop and rename columns and struct fields, and widening type promotions, all without rewriting data files. Not supported: ENUM, UNION, VARINT, indexes, primary keys. | [schema evolution](https://ducklake.select/docs/stable/duckdb/usage/schema_evolution), [unsupported features](https://ducklake.select/docs/stable/duckdb/unsupported_features) |
| Adopting existing Parquet | `ducklake_add_data_files` registers files without copying, and "the ownership of the Parquet file is transferred to DuckLake": compaction and cleanup may delete it. Extra columns fail unless `ignore_extra_columns => true` (then they are unreadable through DuckLake); types must match the table. Debrief's segments (unsigned ids, `MAP(VARCHAR, VARCHAR)` extra, per-segment promoted `p_*` columns) were accepted with `ignore_extra_columns` (verified locally). | [adding files](https://ducklake.select/docs/stable/duckdb/metadata/adding_files) |
| Moving a store | The catalog stores `data_path` as an absolute path. A copied lake attached without `DATA_PATH ... OVERRIDE_DATA_PATH true` reads the **original** folder, and fails when that folder is gone ("Cannot open file .../smoke2/data/..."); with the override it reads its own copy. Files DuckLake writes are relative to the data path; files adopted from outside it are stored with absolute paths (verified locally). Debrief moves and copies stores (projects, `.debriefz`), so every attach would have to pass the override. | local test, `ducklake_metadata`, `ducklake_data_file.path_is_relative` |

## What the store already does that DuckLake would replace

Read from `origin/feat/debrief-duckdb2-storage-format` (19aff3b83, `debrief/1.2-int` plus the 2.0 storage format):

- **Atomic visibility.** A source becomes visible through the `events` view when it is finished and all segments holding
  its rows are listed (`writer.rs` module doc); segments are written to `.tmp` and renamed (`segments.rs`).
- **Undo of an import** is `remove_source`: three catalog deletes, the view hides the rows at once, compaction rewrites the
  segments later (`lib.rs`, `remove_source`).
- **Time shift** rewrites the affected segments with `ts` moved, keeps their id blocks, swaps them in one catalog
  transaction, then marks them unindexed so Tantivy adds them again with the new times (`skew.rs` module doc).
- **Compaction** merges small segments and drops rows of removed sources, only for indexed segments (`compact.rs`).
- **Stable ids and positions.** Each segment owns a contiguous block of event ids (`segments.rs`), and the time-order
  positions are renumbered after an import or shift. DuckLake keeps its own row id per row ("Every row has a unique row
  id that is maintained", [data file spec](https://ducklake.select/docs/stable/specification/tables/ducklake_data_file)),
  but it assigns them at commit; Debrief's `id` would stay an ordinary column, found through file min/max statistics, and
  the id blocks, `segment_blocks` and positions would stay Debrief's own code on top.
- **Full-text index.** Tantivy, outside DuckDB in both designs.

## How it was measured

Invented events only: `gen_large.py` at scale 0.18 (10M events) and the kept `large10` corpus (scale 1.0, 56M events),
each imported as two imports (everything except one CLEF file, then that file). The harness
([ducklake_trial.rs.txt](2026-11-06-ducklake-trial/ducklake_trial.rs.txt), placed as
`crates/debrief-store-duck/tests/ducklake_trial.rs` at 19aff3b83) runs the store as shipped, and DuckLake tables filled
from the store's own segments in three ways:

- **insert:** DuckDB writes new Parquet files, one commit per 8 segments;
- **adopt:** `ducklake_add_data_files` registers the store's segments as they are;
- **SQLite:** insert, with a SQLite catalog instead of a DuckDB file.

The insert variant was also run with zstd files and with a 768 MB memory limit. Each variant then gets the same undo
(the second import's sources), time shift (+1 h on 11.3% of the rows) and compaction as the store. The query is a time
range plus a field filter that returns 205 rows. It runs through the store's events grid (`Query::page`), and as plain
SQL over the store's segments and over the DuckLake table. Three runs per cell, each through plock, with the load
recorded; ranges, setup and caveats are in the results block.

## Results (ranges over 3 runs; full table in the results block)

| At 56M events | Store today | DuckLake insert (snappy) | DuckLake insert (zstd) | DuckLake adopt |
|---|---|---|---|---|
| Import of 54.4M events | 27.9–37.2 s end to end (1.46–1.95M/s) | write stage only: 10.7–13.4 s | write stage only: 18.4–20.2 s | registration only: 1.04–1.05 s |
| Query during the import, p50 / max | page 177–483 / 2,109–2,171 ms | SQL 25–26 / 68–132 ms | 28–44 / 85–119 ms | 18–19 / 19 ms |
| Quiet query, p50 | page 476–485 ms; same SQL over the segments 54–55 ms | 32–35 ms | 39–43 ms | 22 ms |
| Disk after the import | 2,808–2,817 MB segments + 390–496 MB data.duckdb | 4,150 MB | 2,376 MB | none new |
| Undo of one import (1.7M events) | 5.07–5.13 s | 0.10–0.14 s, space not freed | 0.10 s | 0.07 s |
| Time shift of 6.4M events | 22.2–23.8 s, searchable after 38.3–40.6 s | 2.39–2.44 s, no index or positions | 3.60–3.96 s | 2.19–3.60 s |
| Disk after compaction | 2,698–2,707 MB + 629–630 MB | 4,576 MB | 2,608 MB | 3,907–3,908 MB |
| Peak footprint | 1,361–1,671 MB | 1,936–2,043 MB | 1,880–1,943 MB | 2,562–2,599 MB (compaction) |

What the numbers say:

- **Import speed: no gain measured.** The store's import is parse-bound and includes the time ordering. DuckLake does
  not parse; its write stage alone took 10.7–20.2 s for 54.4M already-parsed events, on top of parsing. Adopting the
  segments costs only about 1 s, but that keeps the store's own writer, so its import rate would be the store's.
- **Query during an import: the slow part is the page path, not the files.** The grid query took 177–483 ms p50 and up
  to 2.2 s during the first import, and 1.4 s p50 (p95 up to 3.1 s) during the second. The same filter as one SQL
  statement over the same segment files took 54–55 ms quiet. DuckLake's SQL took 25–44 ms p50 during the import and
  32–43 ms quiet. At 10M the raw SQL over the segments (19 ms) was faster than DuckLake (25–32 ms). At 56M DuckLake's 58
  larger files beat the store's 441–446 segments (32–43 against 55 ms). That difference is file count and size, not the
  catalog.
- **Disk:** DuckLake with zstd stored the events in 2,376 MB, against the store's 2,808–2,817 MB of segments (the
  store's data.duckdb also holds positions and catalogues that DuckLake would not replace). DuckDB's default snappy files
  were 4,150 MB. After the undo and the shift, DuckLake's maintenance left the disk larger (2,608 MB zstd), because
  `ducklake_rewrite_data_files` with its defaults rewrote nothing. The store had freed the undone import at once.
- **Undo and time shift** are 10 to 50 times faster in DuckLake. Most of that gap is work the store does and DuckLake
  would not remove: the store's undo and shift include its compaction, the renumbering of time positions and, for the
  shift, re-adding 6.4M documents to the full-text index. A DuckLake-based store would still need all three.
- **Memory:** DuckLake's write and maintenance peaked at 1.9–2.6 GB without a limit. With 768 MB (the store's import
  budget) and zstd, 1 of 3 runs at 10M failed in `ducklake_merge_adjacent_files`, and 3 of 3 at 56M failed in the
  time-shift UPDATE, both with "Out of Memory Error". The store stayed at 1.36–1.67 GB, including the index build.
- **Catalogs:** SQLite worked; its write stage was slower and more variable (16.8–28.2 s at 56M) than with a DuckDB
  file. Multi-process access, the reason to choose SQLite, was not exercised. Debrief's store worker is one process.

## Not measured

- An end-to-end DuckLake import (parse into an Appender or into DuckLake-written files). The Appender works (probe:
  1,000 rows), but no import pipeline was built.
- 100M events (skipped by brief).
- Cold-cache queries.
- Readers in another process.
- Windows and Linux.
- `ducklake_rewrite_data_files` with a lower delete threshold.
- `SET SORTED BY (ts)` (sorted tables) as a stand-in for time ordering.
- Whether merging the store's segments into fewer, larger files gives the same query gain at 56M. That is the
  hypothesis the 55 against 33 ms points to.

## Recommendation

**Do not adopt DuckLake for Debrief's store (not now, and not on the measured evidence).** Ranked:

1. **Keep the store as it is** (Parquet segments, DuckDB catalog, Tantivy). DuckLake measured no import gain, used more
   memory, failed under the store's memory budget, and did not reclaim space after undo and shift with default
   maintenance. Its fast undo and shift would not remove Debrief's own index and position work. It would also add a
   43 MB native extension per platform (12.7 to 17.2 MB gzipped), an absolute `data_path` that every attach must
   override because stores move and are copied, and hidden promoted `p_*` columns on adopted segments.
2. **Fallback, only if a new requirement appears** (readers in another process, time travel, or a standard table format
   for other tools): use DuckLake as a catalog over the store's own segments ("adopt": registration took 0.20–0.27 s at
   10M and about 1 s at 56M, at a footprint of 33–36 MB and 126–137 MB; its compaction peaked at 2.6 GB). Bundle the extension and pass
   `OVERRIDE_DATA_PATH` on every attach.

The trial points at two cheaper changes inside the current store. Both are unmeasured as fixes:

- **WP-1, the grid query (1–2 days to diagnose).** `Query::page` took 476–485 ms for 205 rows, where the same filter as
  plain SQL over the same files took 54–55 ms. It reached 1.4–3.1 s while a second import ran. Profile the page path,
  for example a total count or the view's `source IN (ready)` filter, with this harness's query and the `bench_reads`
  harness.
- **WP-2, fewer, larger segments (1 day to measure).** At 56M the store keeps 441–446 segments; DuckLake's 58 files
  answered the same SQL in 32–35 ms against 54–55 ms. Measure a compaction target of several segments per file (and its
  effect on undo and shift) before changing `compact.rs`.

Risks of the recommendation: the comparison of the write stage leaves out parsing and ordering. The DuckLake undo,
shift and compaction numbers come from SQL that does no index or position work. Both are stated in the results block.
