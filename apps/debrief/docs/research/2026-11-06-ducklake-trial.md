# DuckLake trial for Debrief's store (2026-11-06)

Status: research note; trial run 2026-11-07 01:15 to 03:20, within its budget. Decision for the user and the coordinator: see
[Recommendation](#recommendation). Results block (rule 48): [performance.md, "2026-11-06, DuckLake
trial"](../performance.md#2026-11-06-ducklake-trial-branch-docsdebrief-ducklake-trial). Harness and scripts:
[2026-11-06-ducklake-trial/](2026-11-06-ducklake-trial/).

Update 2026-11-07: [trial 2](2026-11-07-ducklake-trial-2.md) measured adopt mode end to end inside the store, with the WP-1
page fix. A plain ts min/max file list over the same segments (24 ms at 56M) beat DuckLake's access path (53–57 ms), so
the access-path gain below comes from file pruning that the store can do without DuckLake. Trial 2's recommendation
replaces WP-2 here. The macOS CI check used in this trial was blind at the time; its reconstruction is in the trial 1
results block.

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

The insert variant was also run with zstd files, and with zstd plus a 768 MB DuckDB memory limit (the store worker's
import budget): first with 8 threads and DuckDB's defaults, then with 4 threads (the store's DuckDB thread count) and
`preserve_insertion_order = false` (DuckDB's own advice in its out-of-memory error). Each variant then gets the same
undo (the second import's sources), time shift (+1 h on 11.3% of the rows) and compaction as the store. The DuckLake
shift is one UPDATE over 6.4M rows; the store's is a per-segment rewrite. The query is a time range plus a field filter
that returns 205 rows. It runs through the store's events grid (`Query::page`), and as plain SQL over the store's
segments and over the DuckLake table. Three runs per cell, each through plock, with the load recorded; ranges, setup,
budgets and caveats are in the results block.

## Results (ranges over 3 runs; full tables in the results block)

| At 56M events | Store today | DuckLake insert (snappy) | DuckLake insert (zstd) | DuckLake zstd, 768 MB, 4 threads | DuckLake adopt |
|---|---|---|---|---|---|
| Import of 54.4M events | 27.9–37.2 s end to end (1.46–1.95M/s) | write stage only: 10.7–13.4 s | write stage only: 18.4–20.2 s | write stage only: 14.95–15.04 s | registration only: 1.04–1.05 s |
| Query during the import, p50 / max | page 177–483 / 2,109–2,171 ms | SQL 25–26 / 68–132 ms | 28–44 / 85–119 ms | 48–53 / 157–162 ms | 18–19 ms (4 queries during a 1 s registration) |
| Quiet query, p50, 8 DuckDB threads | page (4 threads, 192 MB) 476–485 ms; same SQL over the segments 54–55 ms | 32–35 ms | 39–43 ms | 54 ms (4 threads) | 22 ms |
| Quiet query, p50, 4 DuckDB threads | same SQL over the segments 62.6–63.8 ms | | | | 31.4–31.5 ms |
| Disk after the import | 2,808–2,817 MB segments + 390–496 MB data.duckdb | 4,150 MB | 2,376 MB | 2,375 MB | none new |
| Undo of one import (1.7M events) | 5.07–5.13 s | 0.10–0.14 s, space not freed | 0.10 s | 0.12 s | 0.07 s |
| Time shift of 6.4M events | 22.2–23.8 s, searchable after 38.3–40.6 s | 2.39–2.44 s, no index or positions | 3.60–3.96 s | 3.46–3.51 s | 2.19–3.60 s |
| Disk after compaction | 2,698–2,707 MB + 629–630 MB | 4,576 MB | 2,608 MB | 2,606–2,607 MB | 3,907–3,908 MB |
| Peak footprint | 1,361–1,671 MB (DuckDB capped at 192/768 MB) | 1,936–2,043 MB (DuckDB uncapped) | 1,880–1,943 MB (uncapped) | 1,206–1,269 MB | 2,562–2,599 MB (compaction, uncapped) |

What the numbers say:

- **Import speed: not measured end to end.** The store's import includes parsing and the time ordering. DuckLake does
  not parse: its write stage alone adds 10.7–20.2 s for 54.4M already-parsed events on top of parsing. Adopting the
  segments costs about 1 s, but keeps the store's own writer, so the import rate would stay the store's.
- **Query: the slow part is the page path, and the faster DuckLake query comes from its access path.** The grid query
  took 177–483 ms p50 and up to 2.2 s during the first import, and 1.4 s p50 (p95 up to 3.1 s) during the second. The
  same filter as one SQL statement over the same files took 54–55 ms quiet. Through DuckLake, over the store's own 441–446
  files (adopt), it took 22 ms with 8 threads and 31 ms with 4, against 55 and 63 ms for plain
  `read_parquet(..., union_by_name = true)` over the same files. DuckLake's own 58 larger files were slower (32–43 ms).
  So the gain is the access path, not the file count: the catalog's per-file statistics, which prune files before
  reading, or skipping `union_by_name` schema merging across segments with different `p_*` columns. Which of the two
  was not measured. At 10M the raw SQL over the segments (19 ms) was faster than DuckLake's own files (25–32 ms) and close
  to adopt (17.5–27.9 ms).
- **Disk:** DuckLake with zstd stored the events in 2,375–2,377 MB, against the store's 2,808–2,817 MB of segments (the
  store's data.duckdb also holds positions and catalogues that DuckLake would not replace). DuckDB's default snappy files
  were 4,150 MB. After the undo and the shift, DuckLake's maintenance left the disk larger (2,606–2,608 MB zstd), because
  `ducklake_rewrite_data_files` with its defaults rewrote nothing. The store had freed the undone import at once.
- **Undo and time shift** run faster in DuckLake: undo 22–39 times at 10M and 37–52 times at 56M; the shift 5–19 times
  at 10M and 4–11 times at 56M. The store's undo and shift include its compaction, the renumbering of time positions and,
  for the shift, re-adding the moved documents to the full-text index. A DuckLake-based store would still need all three.
  How much of the gap each one accounts for was not broken down by phase.
- **Memory:** with 8 threads, DuckDB's defaults and no limit, DuckLake peaked at 1.9–2.6 GB, against the store's
  1.36–1.67 GB, but the store caps DuckDB at 192 MB (768 MB while it compacts and orders) and DuckLake was uncapped. With
  768 MB, 8 threads and defaults, 1 of 3 runs at 10M failed with "Out of Memory Error" in `ducklake_merge_adjacent_files`.
  The 56M 3-of-3 failures (in the time-shift UPDATE) come only from the discarded first 56M pass. With 768 MB, 4 threads
  and `preserve_insertion_order = false`, all 6 runs passed: 940–986 MB peak at 10M (store 993–1,129 MB) and
  1,206–1,269 MB at 56M (store 1,361–1,671 MB). The write stage took 2.79–2.81 s and 14.95–15.04 s, and the quiet query 42–43 ms and 54 ms.
  Under a like-for-like cap, DuckLake used about the same as the store's import phase (916–941 MB at 10M), without
  parsing or indexing.
- **Catalogs:** SQLite worked; its write stage was slower and more variable (16.8–28.2 s at 56M) than with a DuckDB
  file. Multi-process access, the reason to choose SQLite, was not exercised. Debrief's store worker is one process.
- **Unexplained spreads** (reported as ranges, not explained): the 10M snappy write took 1.92 s in run 1 and 4.86–5.14 s
  in runs 2 and 3; the store's 56M import took 37.2 s in run 1 and 27.9–28.4 s in runs 2 and 3; the SQLite write at 56M
  took 16.8–28.2 s.

## Not measured

- An end-to-end DuckLake import (parse into an Appender or into DuckLake-written files). The Appender works (probe:
  1,000 rows), but no import pipeline was built.
- 100M events (skipped by brief).
- Cold-cache queries.
- Readers in another process.
- Windows and Linux.
- `ducklake_rewrite_data_files` with a lower delete threshold.
- `SET SORTED BY (ts)` (sorted tables) as a stand-in for time ordering.
- Which part of DuckLake's access path makes the same 441–446 files faster: file pruning by statistics, or no
  `union_by_name` schema merging.
- The undo and shift gap broken down by phase (compaction, positions, index).
- The page query with the plain-SQL setup (8 threads, no limit).

## Recommendation

**Do not adopt DuckLake for Debrief's store now.** Ranked:

1. **Keep the store as it is** (Parquet segments, DuckDB catalog, Tantivy).
   - **No requirement needs it now.** Debrief needs neither readers in another process nor time travel; the store worker
     is one process.
   - **Native extension per platform.** It would add a 43 MB native extension per platform (12.7 to 17.2 MB gzipped)
     to ship and load by path.
   - **Path override.** The catalog keeps an absolute `data_path`, so every attach must pass `OVERRIDE_DATA_PATH`,
     because stores move and are copied.
   - **Ownership of adopted files.** Adopted segments become DuckLake's to delete, and their promoted `p_*` columns
     become unreadable through it.
   - **Index and positions stay.** The full-text index and the time positions remain Debrief's work either way.
   - **Measured costs.** The import was not measured end to end; the write stage alone adds 10.7–20.2 s at 56M on top
     of parsing. Deleted and updated rows were not reclaimed by default maintenance.
2. **Fallback, only if a new requirement appears** (readers in another process, time travel, or a standard table format
   for other tools): use DuckLake as a catalog over the store's own segments ("adopt": registration took 0.20–0.27 s at
   10M and about 1 s at 56M, at a footprint of 33–36 MB and 126–137 MB; its compaction peaked at 2.6 GB uncapped).
   - Bundle the extension and pass `OVERRIDE_DATA_PATH` on every attach.
   - Adopted files outside the data path are stored with absolute paths, which the override does not fix when a store
     moves. Write the segments inside DuckLake's data path before adopting them; that is an open risk, not tested.

The trial points at two cheaper changes inside the current store. Both are unmeasured as fixes:

- **WP-1, the grid query (1–2 days to diagnose).** `Query::page` took 476–485 ms for 205 rows, where the same filter as
  plain SQL over the same files took 54–55 ms. It reached 1.4–3.1 s while a second import ran, over the 0.5 s
  interactive budget.
  - First hypothesis, a confound: the page query runs on the store's connection with 4 DuckDB threads and a 192 MB
    limit, while the plain-SQL runs had 8 threads and no limit. At 4 threads the plain SQL took 62.6–63.8 ms, so the
    thread count alone explains little; the memory limit is untested.
  - Then profile the page path (for example a total count, or the view's `source IN (ready)` filter) with this harness's
    query and the `bench_reads` harness.
- **WP-2, the access path over the same files (about 1 day).** Over the same 441–446 segments at 56M, DuckLake answered
  in 22 ms (31 ms at 4 threads) against 55 ms (63 ms) for plain `read_parquet(..., union_by_name = true)`. Measure:
  - the plain SQL with the file list pruned by the store's per-segment min/max `ts` before `read_parquet`;
  - the cost of `union_by_name` across segments whose `p_*` columns differ.

  Only after that, measure a larger segment size.

Risks of the recommendation: the comparison of the write stage leaves out parsing and ordering. The DuckLake undo,
shift and compaction numbers come from SQL that does no index or position work. Both are stated in the results block.
