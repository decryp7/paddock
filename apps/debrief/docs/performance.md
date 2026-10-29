# Performance record (bundle sizes and startup)

Rule 35: every release records the initial chunk size, and the budgets in `app/angular.json` are measured size plus 10%.
Measured with `cd app && npx ng build` (production) and a headless Chromium load of `dist/app/browser` with the mock
backend (browser build; the desktop app loads no mocks).

## 2026-10-23, baseline before the Angular phase-1 migration (main at 67a51214)

| Measure | Value |
|---|---|
| Initial chunks (what `ng build` budgets) | 171.76 kB raw (styles 164.42 kB, `main` 6.80 kB, polyfills 0.5 kB); 15.77 kB transfer |
| Why so small | `main.ts` loads translations, then lazily imports `bootstrap`; the real startup cost is the lazy `bootstrap` chunk graph |
| `bootstrap` chunk (static closure, 122 chunks) | 1.52 MB raw |
| Largest single chunks | notices-generated 766 kB, a shared vendor chunk 678 kB, messages-pseudo-json 563 kB (only for the pseudo locale), bootstrap 466 kB |
| Browser startup (mock backend, to `networkidle`, 5 runs) | about 1.1 s, 170 requests, 3.69 MB raw (2.5 MB of it is the closure of the mock-backends chunk, browser only) |
| Total chunks | 3 initial + 285 lazy |

Budgets set from this: `initial` warn 190 kB / error 210 kB (measured + 10%, was 1.5 MB / 3 MB); `anyScript` warn 845 kB
(largest chunk + 10%) / error 1 MB.

## 2026-10-23, after Angular phase 1 (lazy Settings screen and diagnostics collector)

| Measure | Before | After |
|---|---|---|
| Initial chunks (budgeted) | 171.76 kB | 171.76 kB (unchanged: `main` only loads translations, then `bootstrap`) |
| Startup JS in the `bootstrap` static closure, raw | 2.08 MB (123 files, incl. 563 kB pseudo-locale messages loaded only for pseudo) | 1.87 MB (115 files) |
| Same, gzip | 602 kB | 544 kB |
| Browser startup bytes (mock backend, networkidle) | 3.69 MB, 170 requests, about 1.1 s | 3.52 MB, 169 requests, about 1.1 s |

The Settings screen (with its Storage, Parsers, Keyboard and other tabs) and the diagnostics collector dialog were
imported statically by the services that open them; their `open()` now loads them on first use. Views registered through
`component: () => import(...)` were already lazy. Largest remaining startup weight (stats-json, bytes in output):
dockview-core 479 kB, the mock backends 248 kB (browser only), Angular core 158 kB, echarts about 200 kB (pulled in by
chart cards and the Home view), the share/report, tools and AI folder-setup modules (statically imported by commands).
Next candidates: lazy dialog services for share/report, the format wizard and folder setup, and an `@defer` around echarts
consumers. How to measure: `npx ng build --stats-json`, then group `dist/app/browser-stats.json` outputs by input.

## 2026-10-24, what the main window loads before first paint (branch perf/startup-bundle)

`ng build`'s `initial` budget only sees `main`, polyfills and styles, because `main.ts` imports `bootstrap` dynamically and
`bootstrap` imports `app` and `app.config` dynamically. The real first-paint set is the static-import closure of the
chunks for `src/main.ts`, `src/bootstrap.ts`, `src/app/app.ts` and `src/app/app.config.ts` (for a pop-out `popout-shell.ts`
replaces `app.ts`; for a dialog window the dialog shell, its bootstrap and its content). `app/scripts/window-weight.mjs`
measures it from `dist/app/browser-stats.json` (`npx ng build --stats-json`, then `node scripts/window-weight.mjs main`;
`npm run check:window-weight` does both) and prints the heaviest chunks and packages. CI runs it after the frontend build.

| Measure | Before | After | Budget (measured + 10%) |
|---|---|---|---|
| Main window first-paint JS (static closure) | 1710.5 kB, 186 chunks | 1231.3 kB, 147 chunks | CI fails over 1355 kB |
| Pop-out window (reported, not enforced) | 1460.3 kB, 163 chunks | 1170.0 kB, 148 chunks | none (boots the whole `app.config`) |
| About dialog window | 441.2 kB, 30 chunks | 442.1 kB, 30 chunks | CI fails over 480 kB |
| Initial chunks (`ng build` budget) | 173.81 kB (styles 165.56, `main` 7.72, polyfills 0.5) | 173.81 kB | warn 185 kB, error 192 kB (was 190/210) |

What moved out of the first-paint set (all load when used, with the same behaviour): the dialogs, wizards and editors that
commands and services imported statically (format wizard, folder setup, report export, incident summary, redaction,
export, import range, add-tile, clock, updates, parser editor and import confirm, mark/marker editor, infer/extract/mask/drop,
note editor, knowledge editor, package review, layout dialogs, About) through
`DialogService.openLazy(() => import(...), options)`; What's new (now a document of the dock) and its generated changelog load with the view; the search-everything palette (fetched on first open); and, with
`@defer`, the filter bar (and the query field), side bar, command palette, import toast, partial-import banner and tour
overlay. Dialog windows no longer create the workspace facade, time cursor, live follow or live tail: `sharedUiState`
takes a scope (`display`, `summary`, `full`) and a dialog window asks for the narrowest one its backends allow.

What is left (about 780 kB of the 1231 kB is `src/app`, about 500 kB Angular, CDK and RxJS): the `provide*()` of about 50
features in `app.config.ts`, each of which statically pulls its stores, commands and start-up work (share 54 kB, popout 37,
profiling 30, AI 28, video 18, notes 15, dumps 11, playbooks 11 ...), the four built-in themes (27 kB, needed for the first
paint), and the dialog registry (17 kB). Getting a pop-out or the main window much lower needs lazy feature registration (a
manifest of command and view stubs per feature, the feature's code loaded on first use) and per-view provider sets for
pop-outs; that is a design change, proposed as an ADR rather than done here.

## 2026-10-24, start-up timings (branch perf/startup)

Every start logs one structured `Startup timings` line (`app/src-tauri/src/startup_timings.rs`): milliseconds since
the process start for `windowCreated`, `setupDone`, `pageLoad`, `firstPaint` (the page's first contentful paint),
`windowReady` (ADR 0058), `workspaceLoaded` and `firstInteractive` (the first frame after the load), plus `setup`'s
steps slowest first (`Steps`). The `startup_timings` command returns the same. The nightly `build` run starts the
debug app five times after a first run (`scripts/ci/startup-bench.mjs`) and checks the medians against
`scripts/ci/startup-budgets.json` (macOS; Linux reports only until its numbers are recorded).

Debug app on the Mac mini, empty data folder, median of 15 everyday starts, before → after this branch (interleaved
runs, load average 9–13 from other builds, so ±30 ms):

| Mark | Before (ms) | After (ms) |
|---|---|---|
| windowCreated | 217 | 215 |
| setupDone | 282 | 264 |
| pageLoad | 530 | 524 |
| firstPaint | 636 | 610 |
| windowReady | 637 | 611 |
| workspaceLoaded | 648 | 619 |
| firstInteractive | 653 | 624 |

Steps that left the main thread: stored settings (11–250 ms under load: applying a plugin's section starts its
process) and the tray icon (9–13 ms, now after the first paint); the store worker starts after the first paint
(about 0.5 s of background work the first query no longer waits for); the AI start-up log line no longer scans every
process. What remains before the first paint is the Tauri builder (`before setup`, ~115 ms in a debug build), creating
the main window's webview (~100 ms) and WebKit starting its content process after `setup` (~260 ms between
`setupDone` and `pageLoad`, outside Debrief's code). The native menu bar is built twice per start (the standard menus,
then the UI's once the workspace loaded), was 3–4 times.

The 1.4 s the user's 1.0.2 log showed between the window and the AI line was not reproduced with an empty data
folder (30 ms here); it depends on that Mac's state (stored settings with plugin sections, an AI model loading). The
`Steps` field of the next start's line names the step.

## Query and import performance, DuckDB 1.5.6 against 2.0 (2026-10-28 and 29)

Every block says what it measured: machine (Apple M4 Pro, 14 cores, macOS), the load average when it ran (load over 10 is marked and
was not used for conclusions), the corpus (invented, same shape as the samples; never user logs), the command, and the build (release).
Reproduce with `/Volumes/d7sst7/dev-targets/duck2-bench` (`ab.sh`, `imem.sh`, `reads.sh`, `fmt/suite.py`); the decisions are in
ADR 0014's DuckDB 2.0 amendment.

### Real import pipeline, 1.5.6 against 2.0 (`ab.sh`)

Release CLI plus store worker, 16,847,461 events (2.42 GB: CLEF, log4net text, compressed), three interleaved pairs, load 33 to 65
(over 10: the speeds are lower bounds). Events per second 1.5.6 / 2.0: 707k / 685k, 687k / 738k, 855k / 991k. Worker CPU 91 to 94 s
against 79 to 81 s. Peak physical footprint 932 to 979 MB against 905 to 945 MB (both above the 768 MB budget; not new). End-of-import
footprint 319 to 344 MB against 452 to 536 MB (explained next).

### Memory after an import, and a bigger limit during it (`imem.sh`)

In-process import of 3,000,000 CLEF events (release, corpus cached, load 3 to 7), then 30 s idle, three runs per variant.

| Variant | Import | Peak footprint | DuckDB's own use right after | 30 s later | RSS 30 s later |
|---|---|---|---|---|---|
| A: shipped 192 MB limit | 4.2 to 5.3 s (570k to 717k events/s) | 756 to 816 MB | 10 to 12 MB | 172 to 176 MB | 1,092 to 1,133 MB |
| B: 512 MB during the import, 192 MB after | 4.2 to 4.3 s | 783 to 815 MB | 14 to 15 MB | 171 to 175 MB | 1,115 to 1,150 MB |
| C: 512 MB throughout | 4.1 to 4.3 s | 785 to 829 MB | 14 to 15 MB | 205 to 211 MB | 1,126 to 1,183 MB |
| D: 64 MB set after the import | 4.2 to 4.4 s | not sampled | 11 MB | 56 to 59 MB | 1,078 to 1,132 MB |
| E: 32 MB set after the import | 4.2 to 4.3 s | not sampled | 11 to 12 MB | 25 to 30 MB | 1,057 to 1,096 MB |

Explanation of the extra memory after an import: it is DuckDB's buffer pool. The import itself uses 10 to 15 MB of DuckDB memory (events go
to Parquet through arrow-rs, not through DuckDB); the background full-text index and catalogue builds then read the segments through DuckDB
and fill its pool up to `memory_limit` within 30 s (172 MB at the 192 MB limit, 205 to 211 MB at 512 MB). It is a cache bounded by the
limit, not a leak. A higher limit during the import gains nothing (A against B and C: same time, same peak). Setting a lower limit after
the import caps it (D, E): 115 to 145 MB less DuckDB memory; RSS moves little in this process because it also holds the allocator's
pages (RSS is not the right gauge; rule 26). Recommendation: do not raise the limit for imports; lower it to about 64 MB while the store
is idle after an import and restore 192 MB on the first query, if the idle budget needs the memory.

### Write stage against the number of writers (`layout_bakeoff`, 60 copies of the samples, 1.68M events, load 2.6 to 3.6)

| Writers | Events/s | Against one writer | Peak over baseline |
|---|---|---|---|
| 1 | 1.15M | 1.0x | 0 to 6 MB |
| 2 | 2.28M to 2.29M | 2.0x | 0 to 4 MB |
| 4 | 4.39M to 4.45M | 3.8x | 2 to 3 MB |
| 8 | 7.8M to 8.2M | 7.0x | 12 to 51 MB |
| 12 | 10.5M to 10.7M | 9.2x | 52 to 61 MB |
| Arrow appender into one native table | 737k to 745k | 0.64x | +175 to +470 MB |

Verdict: the write stage scales linearly with writers up to the core count (14 cores; the pipeline's real ceiling is parsing, 0.7 to
1.0M events/s in the pair table above, so writers beyond about the parse rate buy nothing). A single appender is one writer and tops out
at 0.74M events/s, below one Parquet writer. Appender with two writers was not measured (a native table has one appender per connection;
the test has no such variant).

### Query formats on the same 3M events (`fmt/suite.py`, load 3 to 5, 2 rounds of 7 repetitions, 192 MB limit, 4 threads)

p50 / p95 over the 14 repetitions of each query. File sizes: Parquet segments 165 MB; native table 1,681 MB (storage v2.0.0, 2.8 s to
create from the view) and 1,675 MB (v1.5.0, 4.0 s); Vortex 388 MB (13.4 s; `extra`, a MAP, is not supported and was written as JSON text).

| Query | Parquet + view | native v2.0.0 | native v1.5.0 | Vortex |
|---|---|---|---|---|
| count | 5 / 8 ms | 0 / 0 | 0 / 0 | 0 / 1 |
| level facet | 8 / 8 | 2 / 2 | 2 / 2 | 2 / 2 |
| minute x level | 18 / 19 | 7 / 9 | 7 / 7 | 7 / 8 |
| first page | 23 / 24 | 4 / 4 | 3 / 4 | 5 / 5 |
| extra facet | 119 / 120 | 59 / 64 | 57 / 62 | 87 / 98 |
| ILIKE | 40 / 41 | 45 / 46 | 43 / 44 | 23 / 23 |
| regex | 44 / 45 | 33 / 33 | 33 / 34 | 34 / 34 |
| by id | 2 / 3 | 0 / 1 | 0 / 1 | 1 / 1 |
| deep page | 77 / 80 | 57 / 58 | 56 / 57 | 57 / 58 |
| peak RSS | 125 to 132 MB | 294 to 326 MB | 302 to 304 MB | 898 to 930 MB |

The corpus is 20 shifted copies of the samples, which zstd compresses far better than DuckDB's native encodings would compress real logs,
so the 10x size gap is exaggerated; the direction is not. All runs are warm for the operating system's cache; "cold" here is only the
first run of a fresh process and is not separated. Native storage v2.0.0 (DICT_FSST) and v1.5.0 are the same in speed and size on this
data. Verdict on formats: a native table answers the scans 1.5 to 5x faster and uses 10x the disk and 2.3x the memory; at 3M events
every query is far under 0.5 s in all formats, so the Parquet default does not need to change on speed.

### Appender against the Parquet fan-out (answer to "is the fan-out needed?", from the write-stage and format blocks)

1. **Fan-out** (today): parse threads write segments in parallel; the write stage is never the limit (1.15M events/s per writer, 7 to 10M
   with 8 to 12); 165 MB per 3M events; catalogue of segments, compaction, deletion list and `union_by_name` are the code it costs.
2. **One Appender** into a native table: 0.74M events/s, so with 8 parse threads producing about 1M events/s it would be the bottleneck and
   needs a queue in front of it; peak +175 to +470 MB; 975 MB to 1.7 GB on disk for 1.7M to 3M events. It would delete the segment catalogue,
   compaction and `union_by_name`, but not the need for a deletion path or for the Tantivy index.
3. **Hybrid** (fan-out, then `INSERT INTO native SELECT * FROM view`): 2.8 s (v2.0.0) or 4.0 s (v1.5.0) for 3M events, about 1M events/s,
   doubling the disk and holding 1.2 GB of RSS while it runs.

Not measured, honestly: the real pipeline with the sink swapped (the importer's sink interface would need an Appender implementation; the
write-stage numbers above bound it), end-to-end time until search is ready for each, the compacted-Parquet, DuckLake, Quack, VARIANT and
scaling-with-size cells, and the cold-cache runs (they need a cache purge this account cannot run). They are listed in the ADR as open.

### Results blocks recorded after the fact (rule 48)

**Writer bench, first run (the 387k / 725k / 5.1M figures quoted in the ADR and reports).** Harness
`crates/debrief-store-duck/tests/it/layout_bakeoff.rs`; command
`DEBRIEF_BENCH_COPIES=20 DEBRIEF_THREADS=8 cargo test --release -p debrief-store-duck --test it layout_bakeoff:: -- --ignored --nocapture`;
corpus: 20 copies of the invented samples (about 0.56M events, 28k per copy); build: branch `debrief/duckdb-2` at 7b5653fdd plus the
progress relay later committed as 08cf9f036, DuckDB 2.0.0; machine: M4 Pro, 14 cores; load: not recorded (the machine was shared; the
later runs below were taken at load 2 to 4). Results: native table with one Arrow appender 387,515 events/s (peak +526 MB, 324.5 MB on
disk); one Parquet writer 725,432 events/s (peak +2 MB, 38.1 MB, 48 segments); eight writers 5,087,660 events/s (115 ms, peak +9 MB).
These are lower than the quiet runs of the writer table above for the appender (737k to 745k at load 2.6 to 3.6) and the one-writer
case (1.15M); the first run was taken under load, so treat the quiet figures as the reference.

**Quiet real import, 1.3 to 1.55 million events/s.** Not measured by this branch: it is the research note's figure
(`docs/research/2026-10-duckdb-2-and-large-data.md`, branch `docs/research-duckdb-2` c755e717d, measured at 355fd94ae with DuckDB 1.5.6):
56,152,132 events in 8.07 GB, the release CLI plus store worker (`debrief import`), 36 to 56 s, parse pool `min(cores - 1, 8)`, corpus from
`gen_large.py` scale 1.0 (invented), machine M4 Pro; load: not recorded beyond "quiet". This branch's own pair runs (load 33 to 65) reached
0.69M to 0.99M events/s on a 2.42 GB corpus and must not be read as the pipeline's speed. Re-measure on a quiet machine with
`ab.sh LABEL 156|200 large03` (scale 0.3) before quoting a number.

**More than one Appender (`DEBRIEF_APPENDERS=1,2,4`, 20 copies, load 2.1 to 2.8, three runs).** n appenders on n connections of one
database, each appending its share of the batches in its own transaction, one checkpoint at the end:

| Appenders | Events/s | Against one | Peak over baseline | On disk |
|---|---|---|---|---|
| 1 | 738k to 744k | 1.00x | +477 to +479 MB | 324.5 MB |
| 2 | 925k to 937k | 1.25x | +217 to +316 MB | 327.4 MB |
| 4 | 774k to 793k | 1.06x | +157 to +180 MB | 504 to 506 MB |

This settles the open conflict between "a second appender adds nothing" and "four in-process appenders gain 1.8x": in this harness a
second appender adds 25%, a fourth falls back to 1.06x and writes a 55% larger file (more, smaller row groups). Neither 1.0x nor 1.8x
holds as a rule; the gain is small, peaks at two writers and stays below one Parquet writer (1.15M events/s) on every count. The Quack
harness's 355k against 200k was not reproduced here (a different harness and corpus).

## Import of an 8 GB folder until search is ready (2026-10-29)

The question: how long from "import folder" until search works, for the typical case of a folder of 7+ GB of logs, and where
does the time go? Every block below gives the date, build, machine, load, corpus, command and numbers (rule 48).

- Machine: Apple M4 Pro, 14 cores, 64 GB, macOS; corpus on a Samsung T7 Shield (USB SSD, about 1 GB/s at best).
- Corpus: `large10`, 56,152,132 events in 8.07 GB, 618 files: 8 log4net text files of 587 MB, 6 CLEF files of 409 MB, 4 gzip
  files, 600 small Serilog text files. It comes from `gen_large.py` scale 1.0 (invented data, never user logs).
- Build: release. The harness `crates/debrief-store-duck/tests/import_ready.rs` is on branch `wip/debrief-duckdb2-prototypes`;
  the code measured is `debrief/duckdb-2` at 9e3bb4a7f. The harness runs the shipped `StoreOptions` in-process with the store
  worker's allocator and times, from the start: the import, the full-text index covering every segment, and the catalogue built.
- Command: `DEBRIEF_READY_CORPUS=…/large10 DEBRIEF_READY_STORE=<new folder> cargo test --release --test import_ready -- --nocapture`,
  through `/Volumes/d7sst7/dev-targets/duck2-bench/ready.sh`. Load is the 1-minute load at the start of each run; the run itself
  adds about 9.
- "Warm" means the corpus was read once just before (`cat > /dev/null`, 9.4 s for 7.2 GB from cold).

### Baseline

| Run | Load at start | Import | Events/s | Index ready | Catalogue ready | Peak RSS (import / after) |
|---|---|---|---|---|---|---|
| cold | 9.7 | 47.6 s | 1.18M | 222.7 s | 444.6 s | 1,918 / 2,463 MB |
| cold, sampled with sample(1) | 9.7 | 54.6 s | 1.03M | 237.8 s | 433.1 s | 1,880 / 2,511 MB |
| warm | 5.2 | 39.7 s | 1.41M | 208.3 s | 376.4 s | 1,902 / 2,564 MB |
| warm | 9.96 | 46.1 s | 1.22M | 218.7 s | 386.3 s | 1,949 / 2,589 MB |

Store: 4.95 GB (segments 2.82 GB, Tantivy index 1.52 GB, `data.duckdb` 222 MB). Search is ready 5 to 10 times later than the import
ends, so the background builds are the long pole, not the import.

### Where the time goes (sample(1) call trees, self time; `sampcat.py`, `sampwho.py`)

Three windows of the sampled cold run: the import (1 to 47 s), index and catalogue (60 to 200 s), catalogue alone (240 to 420 s).

1. **Import, parse threads** (about 7.5 of 8 busy):
   - Regex 38%. 88% of that is `Regex::captures_at` in `LineParser::line`; the bounded backtracker resolves the groups.
   - Threads blocked in `read()` 19% (cold only).
   - memcpy 10%, other parsing 8%, zstd 6%, Parquet encoding 4%, Arrow batch building about 1%.

   Large files are already split into 64 MB chunks parsed with rayon work stealing, largest file first. Warm, the threads stay
   busy to the end. Cold, a 6 s near-idle stretch (16 to 22 s) is disk waits during the sequential newline pre-scans of the large
   files: each large file is read twice, the second time from the page cache.
2. **End of the import**: about 8 s at 3.5 cores is `pos::rebuild`, 20% of the warm import. Its plan is a window sort and then a
   second full `ORDER BY pos` sort. The sort keys, about 1.3 GB at 56M rows, exceed the 192 MB limit and spill.
3. **Index**: bound by Tantivy's 2 indexing threads (`WRITER_THREADS`, 64 MB heap), busy about 90%. The segment reader and the
   docs pool mostly wait. About 330k docs/s.
4. **Catalogue**: one thread, one source at a time.
   - Entity extraction 34%, DuckDB row fetch 26%, allocation 14%.
   - DuckDB also opens the segment files for every per-source query.
   - About 1 core for 3 to 6 minutes.
5. **No overlap**: the import holds the store's write lock for its whole length (`lib.rs`, `import_keeping`), so the index and
   the catalogue start only when it ends.
