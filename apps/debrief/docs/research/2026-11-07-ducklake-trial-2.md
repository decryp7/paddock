# DuckLake trial 2: adopt end to end, file pruning and time layout (2026-11-07)

Status: research note; runs 2026-11-07 10:26 to 15:57, plus three targeted runs 16:21 to 16:37, on a shared machine. Results block (rule 48):
[performance.md, "2026-11-07, DuckLake trial 2"](../performance.md#2026-11-07-ducklake-trial-2-branch-docsdebrief-ducklake-trial-2).
Trial 1: [2026-11-06-ducklake-trial.md](2026-11-06-ducklake-trial.md). Harness, store patch, scripts and per-run output:
[2026-11-07-ducklake-trial-2/](2026-11-07-ducklake-trial-2/).

Question (user, 2026-11-07): a store that keeps Debrief's parsing, segments, Tantivy index and positions, but uses
DuckLake (adopt: `ducklake_add_data_files` over its own segments) as the catalog for queries, undo and time shift: is it
better overall than today's store? Added the same day:

- the WP-1 page-query fix as the baseline;
- C1, how well today's segments are ordered by time;
- C2, a row-group size sweep;
- C3, a full time-sorted rewrite as an upper bound;
- C4, files chunked by time with their metadata in the store's own SQL catalog.

## What was compared

All store variants run inside the real store (`DuckStore`, its own connection: 4 DuckDB threads, 192 MB, 768 MB while an
import compacts and orders). Every variant includes the WP-1 fix (`origin/perf/debrief-grid-query` 13e165c75 and
c149483e9, cherry-picked onto 19aff3b83). A test-only patch selects the variant with `DEBRIEF_TRIAL_STORE`.

| Variant | What it is | Where |
|---|---|---|
| B0 base | today's store with the WP-1 fix | store |
| B1 pruned | B0, but a page query with a time range reads only the segments whose `histogram` minutes overlap it | store |
| B2 mirror | a DuckLake catalog in the store folder mirrors `segments` (register on listing, close by `DELETE … WHERE filename`), `p_*` columns declared before registering; `events` and `events_wide` read `lake.events`; undo, shift and compaction stay the store's | store |
| B3 native | B2, but undo is a lake DELETE, the time shift a lake UPDATE (768 MB and no insertion order for the UPDATE), then the store's own index re-adding and position renumbering; reads by id and the indexer's reads of moved segments go through the lake; store compaction off | store |
| C2 | B0 with 16,384, 32,768 or 131,072 rows per row group (default 65,536) | store |
| C1, C3, C4 | the first store run's kept segments, read with the page's SQL shape (count, first 500 ids in time order, their rows) on the store's DuckDB settings | SQL only |

The SQL-level layouts:

- **seg:** all segments read.
- **seg_pruned:** segments chosen by their ts min/max.
- **lake:** DuckLake adopting the segments.
- **sorted (C3):** one full time-sorted rewrite, 131,072 rows per file.
- **b10 (C4):** one file per 10-minute bucket, written import by import (the second import arrives late into the same
  buckets), then merged per bucket.
- **b60, bs60 (C4):** the same per 1-hour bucket, and per source and 1-hour bucket.

Bucket sizes come from C1: an average row group of today's segments spans 1,725 s (10M) and 2,450 s (56M). A 10-minute
bucket is a quarter of that, and a 1-hour bucket about one row group. One day is longer than the whole 10M corpus (about
5.7 h) and as long as the 56M one (about 26 h), so it prunes nothing and was not run.

Correctness gate: the crate's integration tests (`tests/it`, 95 tests) with each variant set.

- **B0 and B1:** passed 95 of 95.
- **B2:** passed 93 of 95:
  - `redacted_copy::the_copy_holds_no_original_secret_anywhere_and_still_opens` fails because the DuckLake catalog keeps
    absolute paths, so a redacted project copy carries the original folder's path;
  - `segments::reopening_recovers_from_a_crash_and_refuses_damage` fails because the lake keeps a file that crash
    recovery removed.
- **B3:** passed 88 of 95: the same two, four that need the store's compaction (off by design), and
  `skew::an_offset_moves_a_files_events_and_is_kept_with_the_store` (an offset reset after a reopen gives wrong times).

So B2 and B3 are measured with known defects, stated with the numbers below.

DuckDB lets one transaction write only one attached database: "the system only supports *writing* to a single attached
database in a single transaction" ([ATTACH](https://duckdb.org/docs/2.0/sql/statements/attach.html), read 2026-11-07); the
trial hit it as "a single transaction can only write to a single attached database". So the store's catalog and the lake cannot change in one transaction. The trial syncs the lake on its own
connection right after each change (idempotent, repaired on the next refresh or open). A product would need a two-phase
order and a reconcile on open.

## Results

Ranges over the counted runs (3 to 4 each at 56M: B0 runs 2–5, B1 runs 1–4, B2 and B3 runs 1–3; 3 each at 10M). All
times are wall time, all memory physical footprint.

Load: 16 of the 23 counted 56M store runs peaked above a 1-minute load of 12 from other agents' jobs. Each run is flagged
in [runs.txt](2026-11-07-ducklake-trial-2/runs.txt). The quiet page query held steady across them: the one clean run per
variant gave B0 170.9, B1 92.0, B2 98.6 and B3 118.6 ms, inside the ranges. Import, end-to-end searchable time and the
queries during an import are load-sensitive; read their ranges with that in mind.

**Store variants at 56M events** (import 1: 54.4M, import 2: 1.7M, shift 6.4M rows):

| | B0 base | B1 pruned | B2 mirror | B3 native |
|---|---|---|---|---|
| Import 1 | 30.3–41.7 s | 35.6–43.3 s | 40.9–45.4 s | 34.8–42.9 s |
| Searchable end to end (index and catalogue) | 306–332 s | 317–329 s | 275–290 s | 274–287 s |
| Page query quiet, p50 | 168–176 ms | 91–108 ms | 91–100 ms | 117–123 ms |
| Page query quiet after compaction | 139–155 ms | 73–79 ms | 75–85 ms | 95–198 ms |
| Plain field filter (no time range), quiet p50 | 1,621–1,649 ms | 1,616–1,777 ms | 1,687–1,745 ms | 1,730–1,758 ms |
| Page query during import 2, p50 / max | 295–1,900 / 3,265–4,542 ms | 174–1,737 / 410–4,484 ms | 172–1,769 / 3,112–4,981 ms | 208–1,674 / 3,698–4,244 ms |
| Undo of import 2, searchable again | 5.22–5.55 s | 4.97–5.77 s | 5.71–8.20 s | 5.41–6.16 s |
| Time shift, searchable again | 33.5–41.3 s | 32.3–37.9 s | 35.2–42.8 s | 29.8–37.0 s |
| Disk after shift | 4,902–4,976 MB | 4,894–5,034 MB | 4,874–4,966 MB | 5,567–5,696 MB (532–544 MB lake files) |
| Moved folder answers the same | yes | yes | yes | **no** (plain filter 267,018 → 297,283–299,370 rows; at 10M 47,715 → 55,867) |
| Peak footprint, import phase / whole run | 1,263–1,581 / 1,785–2,059 MB | 1,383–1,487 / 1,947–2,036 MB | 1,234–1,282 / 1,696–1,778 MB | 1,461–1,521 / 1,948–2,076 MB |

At 10M the same order holds. Page query quiet:

- **B0:** 78.4–78.9 ms;
- **B1:** 64.7–67.0 ms;
- **B2:** 71.3–73.7 ms;
- **B3:** 89.3–91.2 ms.

Undo was 0.79–0.93 s and the shift 4.5–8.0 s for all variants. Full ranges are in the block.

**SQL-level layouts at 56M** (the page's SQL shape; standard 45 s window, 205 rows; p50 of 10 runs):

| Layout | Files | Query p50 | Cost to make |
|---|---|---|---|
| seg (all files) | 446 | 112–114 ms | none |
| seg_pruned (ts min/max catalog) | 446 | 24.2–24.8 ms | catalog read under 0.01 s |
| lake (DuckLake adopt) | 446 | 52.7–56.7 ms | registration 0.5 s |
| sorted (C3) | 429 | 9.6–12.4 ms | one rewrite: 74.5–87.7 s, peak 1.3–1.6 GB |
| b10 (C4), before merge | 220 (58 late files) | 11.8–14.7 ms | write 160–189 s for import 1, 1.8–3.3 s for import 2 |
| b10 (C4), after merge | 162 | 8.7–8.8 ms | merge 21.2–21.8 s |
| b60, bs60 (C4) | — | — | the 1-hour partitioned write ran out of memory at 768 MB at both sizes: 10M 3 of 3 and 56M 3 of 3 with insertion order on, 56M 3 of 3 with it off; bs60 was never reached |

At 10M:

- **seg:** 40–42 ms;
- **seg_pruned:** 24–25 ms;
- **lake:** 36–37 ms;
- **sorted:** 16 ms (rewrite 11.6–12.2 s);
- **b10:** 12–14 ms, and 15.5–15.8 ms after the merge (write 11.7–14.5 s, merge 3.0–3.1 s).

Every `lay-*` run ended with rc 101: the first `b60` step ran out of memory (`ducklake_trial.rs:906` in that build). The
layouts before it (seg to b10) completed and are the numbers above. The committed harness copy is the later version, with
insertion order off for the bucket writes (the `lay60-*` reruns), which still ran out of memory at 56M.

**C1, ordering of today's segments** (averages per random 45 s window; 20 windows):

| | Row groups | Rows per group | Span of a row group | Files / row groups whose range covers the window | Files / row groups holding rows in it | Row groups holding query rows |
|---|---|---|---|---|---|---|
| 10M segments | 159 | 63,578 | 1,725 s | 11.6 / 14.4 | 9.8 / 10.4 | 1.6 |
| 56M segments | 878 | 63,955 | 2,450 s | 18.3 / 22.8 | 12.3 / 12.7 | 2.8 |
| 56M sorted (C3) | 858 | 65,445 | 400 s | 3.5 / 4.0 | 1.6 / 1.7 | 0.8 |
| 56M b10 (C4) | 941 | 59,673 | 593 s | 1.6 / 6.3 | 1.6 / 6.3 | 2.5 |

The segments are time-ordered within a source but not across sources: a 45 s window's rows sit in about 10 to 13 files.
Their ranges cover 12 to 18 files, so even a perfect file-pruning catalog reads about 10 to 13 row groups. A sorted
layout reads 1 to 2.

Compaction: the import's own compaction merged nothing at either size (`replaced=0`). After compaction the layout is the
import's, less the undone import's files, so the "after compaction" rows of C1 equal the import rows.

**C2, row-group size** (B0, page query quiet p50, end to end until searchable):

| Rows per group | 10M page | 56M page | 56M searchable | 56M disk after import |
|---|---|---|---|---|
| 16,384 | 61.6–61.9 ms | 249.9–273.4 ms | 358–375 s | 4,830–4,831 MB |
| 32,768 | 57.7–63.4 ms | 172.9–189.2 ms | 319–344 s | 4,781–4,782 MB |
| 65,536 (today, the B0 runs) | 78.4–78.9 ms | 167.6–175.5 ms | 306–332 s | 4,735–4,740 MB |
| 131,072 | 120.1–125.8 ms | 200.9–206.6 ms | 302–317 s | 4,632–4,730 MB |

**Store worker, 768 MB budget (56M, release CLI `debrief import` through `debrief-store-worker`):** B0 imported 54.4M
events in 39.8 s with a worker peak footprint of 1,388 MB. The "best lake variant" run is **check broken**: the worker
does not inherit the trial's environment. A 10M check import with `DEBRIEF_TRIAL_STORE=mirror` produced no
`lake.ducklake`, so that run was B0 again and is not counted.

## What the numbers say

- **The one measure where DuckLake was ahead: searchable end to end at 56M.** B2 took 275–290 s and B3 274–287 s,
  against B0's 306–332 s and B1's 317–329 s (clean pair: B2 run 1 275 s at peak load 10.5, B0 run 4 321 s at 11.9). The
  gap is in the background build.
  - **Where it is:** three targeted runs split it (`cat-56m-*`, one each, peak loads 12.5–17.7). The index was ready
    after 98–120 s in all of them, and the catalogue was the long pole: B0 270 s, B2 239 s.
  - **Cause:** the catalogue reads each source with `FROM events WHERE source = ?`. In B0 that is `read_parquet` over all
    446 segments with `union_by_name`; in B2 it is `lake.events`, which skips files by their per-file `source`
    statistics.
  - **Without DuckLake:** passing only the segments that `histogram` names for the source (a trial patch, 95 of 95 tests
    pass) brought B0's catalogue to 249 s, about two thirds of B2's gain.
  - **Confidence:** single runs on a loaded machine, so this is partly confirmed, not settled.
- **B2's import 1 was slower:** 40.9–45.4 s against B0's 30.3–41.7 s (registering the segments on every listing).
- **DuckLake as the catalog buys nothing in the page query that B1 does not.** In the store's real page query, B2 mirror (91–100 ms at
  56M) equals B1 pruned (91–108 ms). At the SQL level, DuckLake's pruning over the same 446 files (53–57 ms) is slower
  than a plain ts min/max list (24–25 ms). Trial 1's 22 ms through DuckLake was against an unpruned `read_parquet` with
  `union_by_name`. Pruning the file list explains that gain; DuckLake itself does not.
- **Undo and time shift through DuckLake (B3) are not better end to end.** Undo until searchable: 5.4–6.2 s against B0's
  5.2–5.6 s. Shift until searchable: 29.8–37.0 s against B0's 33.5–41.3 s; the Tantivy re-adding dominates both. B3
  leaves 532–544 MB of extra lake files after one shift, a moved store answers wrongly, and it fails 7 correctness tests.
- **B2 costs correctness work.** The redacted copy leaks the original path, a crash leaves a dangling file, and every
  change needs a second, separate transaction. B2 also needs the 43 MB extension and the path rewrite on move (which
  worked: same answers after a move).
- **File-list pruning is the cheap win:** 168–176 → 91–108 ms (56M) with no format change, and the catalog it needs (the
  histogram) already exists. It does nothing for a query without a time range: the plain field filter stays at
  1.6–1.8 s at 56M in every variant, over the 0.5 s interactive budget.
- **Better time ordering gives little of what is left in the store.** At the SQL level: 24 ms (pruned) → 10–12 ms
  (sorted) or 9–15 ms (10-minute buckets). But the store's page with pruning takes 91–108 ms against 24 ms for its SQL
  shape (and B2 91 ms against 53 ms). Ordering can therefore save at most about 12–15 ms of about 91 ms; the other
  roughly 65 ms is store overhead in the page path that this trial did not attribute. The cost is at import: a sorted rewrite took 75–88 s and 10-minute bucketing 160–189 s for 54.4M events, while
  the whole store import took 30–43 s. Late rows (the second import) became 58 small files in existing buckets, merged
  in 21–22 s. 1-hour buckets did not fit 768 MB in DuckDB's partitioned write.
- **Row-group size:** tied with 32,768 at 56M (168–176 against 173–189 ms); keep the default 65,536. 16,384 is clearly
  worse at 56M (250–273 ms and 50–60 s slower to searchable) and 131,072 worse (201–207 ms). At 10M, 16,384 and 32,768
  were about 20% faster (58–63 against 78–79 ms), so the best size depends on the store's size, and the ordering matters
  more.
- **Memory:** every 56M store run peaked at 1.7–2.1 GB for the whole run in-process (index included) and 1.2–1.6 GB in
  the import phase. Through the worker, B0's import peaked at 1,388 MB, over the 768 MB importing budget and under the
  2 GB hard cap. The lake variants changed this by less than the run-to-run spread.

## Not measured

- **The lake variant through the store worker** (check broken, see above).
- **C4 at 1-hour buckets at 56M** (out of memory at 768 MB); per-source buckets at 56M (bs60, not reached).
- **C3 and C4 in the store's real page path.** Only the SQL shape was measured: positions, ids and the writer were not
  changed for them. Their import cost is a rewrite after the store's import, a proxy for bucketing inside the writer,
  not that.
- **Rows without a timestamp** in the bucket layouts (excluded; a product needs an "untimed" bucket).
- **The cause of B3's wrong answers after a move.**
- **Cold-cache queries, Windows and Linux.**
- **Clean loads on every 56M run** (16 of 23 peaked above 12; see Results).
- **Cold-cache page queries:** the page-query verdicts are "met warm; cold not measured".
- **The remaining store overhead of the page path** (about 65 ms of B1's 91 ms at 56M) is not attributed by phase.
- **The catalogue cause, settled:** one targeted run per variant only.

## Recommendation for the Debrief 1.2 store work

1. **Do not adopt DuckLake, in either adopt form.**
   - **Mirror:** at best equal to file-list pruning.
   - **Native:** no faster end to end, more disk, and wrong answers after a move.
   - **Both:** they cost the 43 MB extension, a second transaction per change, path handling and the defects above.
2. **Adopt file-list pruning from the store's own catalog.**
   - **Page path (B1):** use the per-segment ts range from `histogram`, or a ts min/max column in `segments`. It is
     small, needs no format change and nearly halves the 56M page query after the WP-1 fix (168–176 → 91–108 ms).
   - **Catalogue:** read each source from its own segments (`histogram` names them). That took about two thirds of
     DuckLake's end-to-end gain in one targeted run (catalogue 270 → 249 s, B2 239 s); confirm it with three runs on a
     quiet machine as part of the work.
   - **Estimate:** 2–3 days with tests for the page path, plus 1 day for the catalogue.
3. **Keep 65,536 rows per row group.**
4. **Attribute the page path's remaining store overhead next** (about 65 ms of B1's 91 ms at 56M, outside the SQL
   shape). Improve time ordering only after that, and only if it still matters. Ordering's measured upper bound is 24 →
   10–12 ms at the SQL level, at most about 12–15 ms of the store's 91 ms. The cost is a rewrite or bucketing step of 75–190 s
   per 54M events, or a writer change. The cheaper option to test next is compaction that merges segments into
   time-ordered files in the background after the import, with the late-row merge cost measured here (21 s per 58
   files). Design it with the 768 MB budget in mind: DuckDB's partitioned write did not fit with 1-hour buckets.
5. **Separately: the plain field filter (1.6–1.8 s at 56M) and the worker's import footprint (1.4 GB against 768 MB)
   are over budget in every variant.** Layout does not fix either; they need their own work package.
