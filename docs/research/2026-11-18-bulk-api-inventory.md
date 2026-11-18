# Bulk APIs: inventory of per-item calls and the plan to remove them (2026-11-18)

Status: research note; the decisions are recorded in [ADR 0108](../adr/0108-bulk-apis-one-call-for-many-items.md) (accepted 2026-11-18,
user). Read-only inventory; no code was changed. Where this note and the ADR differ (the home of G2, the owner of G1 and G2),
the ADR wins.

**Evidence heads (ADR 0091).** origin/main `f5f08f71e598b2306bc43b28540dfb13d51832bc`; origin/perf/git-heavy-tests
`51a5a617e2efd08baa68d167d786025ec6c1c276` (read for its scope only). All `file:line` references are at origin/main
f5f08f71e. Nothing here claims shipped behaviour of a tag; no `git tag --contains` claim is made.

**The principle.** engineering-rules commit 7fcacee (2026-11-18): PRINCIPLES.md "Bulk APIs" (an API on items takes many
at once and returns a result per item; a single-item form is a thin wrapper; errors per item; bulk calls stay bounded) and
ER-20 (work over many items is one bulk call or one long-lived process or connection, through one shared mechanism; a
command's count of processes or round trips is a measured budget that a test guards).

**Method.** Five read-only sweeps (Debrief; Dyno; Pitwall, Pitcrew, Spotter and the demos; `crates/` and `ui/`;
`tools/paddock-dev`, `scripts/`, `conformance/`, `.gitea/`). Each candidate was kept only after its looping caller was
read. I re-read a sample of the loops myself (SP-2, PW-1, PC-1, DY-1, DY-2, TD-1, the stdin blocker CR-2, DB-1, DB-10,
DB-11, DB-21, UI-1): all hold. `apps/pacenotes` and `apps/sighter` have no `.rs` or `.ts` code (docs only).

**Costs used.** A process start: about 6 ms on the Mac and about 45 ms on the Windows runner (ER-20, docs/benchmarks.md,
train 14 block); docs/benchmarks.md:1499-1500 records 0.11-0.15 s per git child under test load on the Windows runner.
One synthetic measurement in this sweep (Mac mini, 1-minute load 18-22, the first 200 tracked `.rs` paths, 3 runs):
one `git show HEAD:<p>` per file 2235-2699 ms (11-13 ms per file) against one `git cat-file --batch` for all 200 in
55-57 ms. A Tauri invoke round trip is **not measured** anywhere in the repo (no row in docs/benchmarks.md or an app's
performance.md); WP-G3 measures it. Debrief store calls are an invoke plus a frame to the store worker plus a DuckDB
statement (`apps/debrief/app/src-tauri/src/engine/mod.rs:55`).

Severity: **High** = user-waited with N of about 50 or more, or a process start per item on a hot path (CI plan,
pre-push, live sampler); **Medium** = smaller N on a recurring or user-waited path; **Low** = small fixed N or a rare path.
Effort is agent-days and includes the failing-first round-trip or spawn-count budget test.

## Summary

| Area | Findings (High / Medium / Low) | Recommended set (d) | Before the app's release (d) | Can follow (d) | Optional, user decision (d) |
|---|---|---:|---:|---:|---:|
| WP-0 shared mechanisms and guard (G1, G3-G6) | - | 3.25 | 3.25 (lands first) | - | - |
| Dyno | 8 (0 / 5 / 3) | 5.0 | 3.0 | 2.0 | +2.5 (DY-3 FFI, DY-5 Security.framework) |
| Pitwall | 4 (0 / 2 / 2) | 2.75 | 1.75 | 1.0 | - |
| Debrief | 21 (4 / 10 / 7) | 13.5 | 10.75 | 2.75 | +4.5 (measure first: DB-7, DB-13, DB-16, DB-17) |
| Pitcrew | 5 (1 / 1 / 3) | 3.75 | 2.5 | 1.25 | - |
| Spotter (not in the current release order) | 5 (4 / 1 / 0) | 6.0 | - | 6.0 | +1.5 (SP-3 via paddock-live) |
| Shared crates and ui (UI-1, CR-2 in G1, CR-5 to CR-7, UI-2) | 6 (0 / 2 / 4) | 2.5 | 1.0 (UI-1, with Debrief) | 1.5 | +2.5 (CR-5 in-process SCM, not recommended) |
| paddock-dev, scripts, conformance (incl. G2, TD-2) | 16 (5 / 2 / 9) | 8.5 | - (foundation, not release-gated) | 8.5 | - |
| Merge check (perf/git-heavy-tests) | 3, listed only | 0 here | - | - | - |
| **Total** | **65 (+3 listed)** | **45.25** | **22.25** | **23.0** | **+11.0** |

The cheaper common-practice option for every decision below (the Debrief minimum instead of the full store batch, memo
plus bounded parallel spawns instead of the batch git reader, one connection instead of a Pitcrew protocol change, browser
reuse within one pass instead of a context pool, `no-await-in-loop` instead of a custom lint) totals about **37.5
agent-days**; the recommended set is about 8 days more, half of it the full Debrief store batch (D-DB1, 4.5 d more).

## Shared mechanisms (ER-22): one fix, many findings

| Id | Mechanism | Where | Fixes | Effort |
|---|---|---|---|---:|
| G1 | **A bounded run that takes standard input.** `paddock_proc::run_bounded` sets `Stdio::null()` (`crates/paddock-proc/src/run.rs:208-215`), so no shared code can drive `git cat-file --batch`; merge_check says so (`tools/paddock-dev/src/merge_check/mod.rs:34-35`), and `tools/paddock-dev/src/input_hash.rs:208-220` (`Exec`, `spawn_piped`) is a private second runner that does pipe stdin. Add `run_bounded_with_input(cmd, input: &[u8], limits)` (input written on its own thread, size bounded) and retire the private path. | paddock-proc | CR-2; enables G2 | 0.5 |
| G2 | **One shared git object reader.** `files_at(&[(rev, path)]) -> Vec<Result<Option<Vec<u8>>, _>>` over one `git cat-file --batch` (missing object means `None`), with a `starts()` counter, in the new crate `crates/paddock-git` (user, 2026-11-18; built on `perf/git-heavy-tests-2`, see ADR 0108). Today paddock-repo-graph's `History::file_at` runs `git show` per path (`crates/paddock-repo-graph/src/history.rs:20,115-119`); it moves onto the reader. Replaces the private reader in `tools/paddock-dev/src/check_ids.rs:346-395` (the good model) and the merge check's per-file `cat-file blob`. | paddock-git | TD-1 (=CR-1), TD-3, TD-6, TD-8, TD-10, MC-1 | 1.5 |
| G3 | **Counting seams for budget tests.** (a) A process-start counter in paddock-proc's one spawn path (test-visible, `starts_since(mark)`), so any app's spawn budget is one assertion; today only per-fake counters exist (`crates/paddock-testkit/src/runner.rs:159` `FakeRunner::count`, `apps/dyno/crates/dyno-platform/src/linux/tools.rs` `FakeTools::calls`, `crates/paddock-chromium` fake `calls()`, Pitwall `FakeServer.calls()` `apps/pitwall/app/src-tauri/src/server_client.rs:307`, Pitcrew `Fake.sent` `apps/pitcrew/app/src-tauri/src/commands.rs:737-747`, the DuckDB QueryLog count `apps/debrief/crates/debrief-store-duck/tests/it/regress_1_2.rs:177-198`). (b) A counting `ipc` stub for vitest: every paddock-ui backend and each app's `tauri-backend.ts` takes an injectable invoke, and nothing counts invokes today. (c) Measure the Tauri invoke round trip once on the Mac and Windows (rule 48 row in docs/benchmarks.md). | paddock-proc, paddock-ui testing | every finding's failing-first test | 1.0 |
| G4 | **A per-item result type for bulk commands.** `ItemResult<T> = { ok: T } \| { err: ItemError }` in Rust with the generated TypeScript twin, so a bulk command reports per item (PRINCIPLES, Bulk APIs) the same way in every app. Rule of three is met: SP-1, SP-2, PC-3, PW-1, DY-6, DB-18, DB-19, DB-20, DB-21. | paddock-commands (or the crate that owns the generated contract) | the bulk commands above | 0.5 |
| G5 | **ESLint rule `paddock/no-backend-call-in-loop`** (see Guard). | `eslint/src/rules` | guard | 1.0 |
| G6 | **Rule and review text.** A Paddock rule (docs/rules.md) pointing at PRINCIPLES "Bulk APIs" and ER-20, and a line in the rules-review checklist. | docs | guard | 0.25 |
| | Further shared pieces with one or two users: `Http::get_many` for paddock-dev's curl client (TD-7, TD-9); paddock-chromium browser contexts (CR-3, Spotter); paddock-live coalesced pushes (SP-3); a bulk `has_many` on paddock-secrets' vault port (`crates/paddock-secrets/src/vault.rs:9`, PC-4). Counted in their rows. | | | |

G1 to G6: 4.75 days; WP-0 holds G1 and G3 to G6 (3.25 d), and G2 (1.5 d) is counted with the dev tools that use it (WP-11).

## Dyno (release order 1)

Overall Dyno's wire and UI are already bulk (helper requests take lists or pages, `counterValues(paths)`,
`metricReadings(queries)`, `resolveHosts(addresses)` at 64, `acknowledgeAlarms(ids)`). What is left is per-item spawns in
platform readers, a re-sweep per page in the helper, and per-statement autocommits on the sampler thread.

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d | When |
|---|---|---|---|---|---|---|---|---:|---|
| DY-1 | macOS helper starts `powermetrics` once per counter id (twice on Apple silicon after the "unrecognized sampler" retry) | `apps/dyno/crates/dyno-platform/src/helper_counters.rs:210-237` | `apps/dyno/crates/dyno-core/src/helper_counters.rs:89-100` (`answer` maps `reader.read(*id)`); request always `[Thermal, Fans, CpuEnergy]` at `apps/dyno/crates/dyno-agent/src/helper_readers/counters.rs:76-80` | 3 ids per 30 s request: 2 spawns on Intel, 4 on Apple silicon, where 1 does | root spawn with at least 200 ms sampling each; feeds live temperatures and fans | `CounterReader::read_many(&[CounterId])`: one `powermetrics` per query; remember the sampler set that works | Med | 0.5 | before |
| DY-2 | Helper takes a full process sweep (and 2 `dscl` spawns on macOS) for every `Processes` page | `apps/dyno/crates/dyno-helper/src/dispatch.rs:237-248`; `dscl` at `apps/dyno/crates/dyno-platform/src/helper_sweep.rs:116-128,148-149` | `apps/dyno/crates/dyno-agent/src/helper_readers/processes.rs:97-125` follows `page.next` | pages = ceil(processes / 1024), up to 16 (`MAX_SWEEP` 16384); `dscl` pair every 5 s while the page is open | live path (Processes page); pages also come from different sweeps | keep the last sweep for one TTL or a sweep id in `ProcessPage`; cache `homes()` | Med | 1.0 | before |
| DY-5 | "Unusual programs" calls full `details()` per new program, spawning `codesign` (macOS) or `dpkg -S`/`rpm -qf`/`pacman -Qo` (Linux), **holding the `ports.details` lock** | `apps/dyno/crates/dyno-platform/src/macos/details.rs:29-51`; `linux/packages.rs:44-80` | `apps/dyno/crates/dyno-agent/src/unusual.rs:80-98` | at most 3 per 30 s (`MAX_SIGNATURE_CHECKS`) | spawn per exe, timeout 8 s; blocks a user's ProcessDetails request (`handlers.rs:81`) for up to 3 x 8 s | `SignatureProbe::signatures(&[&Path])` with a (path, size, mtime) cache; Linux one package-manager call per batch; no details lock | Med | 1.0 | before |
| DY-7 | History writer: several autocommit statements per tick, one per disk, one per rollup bucket, two per alarm change, **on the sampler thread under the core lock** | `apps/dyno/crates/dyno-agent/src/store.rs:195,213,288,526,534`; `store/alarms.rs:27,59` | `rollup.rs:52-63,108-116`; `alarms.rs:58-68`; sampler `sampler.rs:166-194` | every 1-2 s tick; up to 12 x 3 rollup buckets on catch-up | WAL + `synchronous=NORMAL`, so cheap per statement, but under the lock | one `transaction()` per `HistoryWriter::record` and per alarm batch, `prepare_cached` as `put_app_minutes` does (`store.rs:237-244`) | Low | 0.5 | before |
| DY-3 | macOS `diskutil info -plist` per mount | `apps/dyno/crates/dyno-platform/src/volumes.rs:292-301` | `apps/dyno/crates/dyno-agent/src/disk/volumes.rs:166-182` | a few mounts; cached per (mount, source) | spawn per new mount; first Disk listing waits | bounded parallel (about 4), or DiskArbitration in process (D-DY1) | Med | 0.5 | follow |
| DY-4 | `smartctl -a -j <dev>` per drive, **implemented twice** (an unregistered copy, rule 55) | `apps/dyno/crates/dyno-platform/src/hardware.rs:374-440`; `smart_reader.rs:37-75` | the two loops (`hardware.rs:403`, `smart_reader.rs:48`) | up to 16 drives, every 3 h | spawn plus SMART I/O, up to 20 s per disk; background | one shared reader, bounded parallel; smartctl takes one device per run (from the man page as remembered, not re-checked) | Med | 0.75 | follow |
| DY-6 | `WolState` helper call per network card | `HelperRequest::WolState` (`apps/dyno/crates/dyno-core/src/helper_wire.rs`); `helper_link.rs:534-537` | `apps/dyno/crates/dyno-agent/src/helper_readers/wol.rs:70-87` | 1-2 cards | one round trip on the kept connection; readiness panel | `WolStates(Vec<IfName>) -> Vec<ItemResult<WolReply>>`, keep `WolState` for old helpers | Low | 0.5 | follow |
| DY-8 | Lifespan overview: one readings query per device, uncached `prepare` | `apps/dyno/crates/dyno-agent/src/store/hardware.rs:55` | `hardware.rs:184-185,214-215` | a handful | one SQLite query each; Lifespan page | one `ORDER BY device, at` query, as `disk_free` (`store.rs:296-315`) | Low | 0.25 | follow |

Checked, not findings (Dyno): the Angular UI makes no backend call per item in a loop; `metric_readings` answers from the
last live update (`app/src-tauri/src/shell.rs:625-627`); plugin `Sensors` is one call per plugin process; the macOS GPU
`ioreg` runs every 5 ticks, not per item; Windows WMI temperatures keep one connection per component (sysinfo 0.40.1);
the per-pid `proc_pidinfo` and `/proc` reads are inherent to the OS; single-spawn readers (`launchctl print-disabled`,
`systemctl list-unit-files`, `wevtutil`, `nettop`, `typeperf`, `nvidia-smi`, `pmset`) run once per whole-system read.

## Pitwall (release order 2)

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d | When |
|---|---|---|---|---|---|---|---|---:|---|
| PW-1 | "Clean unused" deletes models one by one, and each delete re-scans the whole library (opens and parses every GGUF header) | `apps/pitwall/app/src-tauri/src/commands.rs:261` → `crates/pitwall-server/src/mgmt.rs:270-276` → `library.rs:214-236` (`delete` → `local()` → `scan` `:56-69`) | `apps/pitwall/app/src/app/features/models/models.store.ts:254` (`for (const m of models) await backend.deleteModel(m.id)`) | unused models (not derivable) | invoke + a new loopback TCP connection (`crates/pitwall-loopback/src/lib.rs:183`) + full scan each: O(N x M) file opens; user waits | `delete_models(ids) -> Vec<ItemResult<()>>`: scan once | Med | 0.75 | before |
| PW-2 | Embeddings cache: one SQLite commit and one full-table `SUM` per stored vector, one point query per input | `crates/pitwall-server/src/cache_db.rs:170-197` (`Put`, `trim` `:242-252`, `Touch`), `get` `:270-289` | `crates/pitwall-server/src/http.rs:572-575,615-619` | up to 2048 inputs per request (`crates/pitwall-core/src/chat.rs:548`) | commit + O(entries) SUM per put; reads on the request path; the queue of 4096 drops caching when full | one `PutMany` in one transaction with one trim and a running size; one `WHERE key IN (...)` chunked | Med | 1.0 | before |
| PW-3 | Model update check: 2 HTTP calls per repository | `crates/pitwall-server/src/updates.rs:59-71` | `updates.rs:233-238` | up to 40 repos | 2 HTTPS each, background | one call per repo if `/api/models/{repo}?blobs=true` gives the tree (**unverified**) | Low | 0.5 | follow |
| PW-4 | Settings flush sends the whole document once per waiting field | `apps/pitwall/app/src/app/features/settings/settings.store.ts:261-271` | `:296-305`, `:426` (`Promise.all(ids.map(sendField))`) | fields edited in one debounce (small) | invoke + loopback + save each | merge the waiting edits into one `save_settings` with per-field refusals | Low | 0.5 | follow |

Checked, not findings: benchmarks `runAll` (serial by design, model load dominates); `connect_saved` over a fixed short
list on long-lived MCP connections; `os_signals` fixed 2 spawns per tick; Angular `Promise.all` calls that fetch
different endpoints. Good model: only cache misses go to llama.cpp, in one call (`http.rs:600-609`).

## Debrief (release order 3)

Cost note: each store-port call from the app is an invoke, a frame to the store worker and a DuckDB statement over the
`events` view of `read_parquet([...all segments])` (`apps/debrief/crates/debrief-store-duck/src/schema.rs:130,159`), so a
per-item `WHERE` or facet scans every segment unless row-group statistics prune it (pruning not measured).

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d | When |
|---|---|---|---|---|---|---|---|---:|---|
| DB-1 | Search everything and Find run one facet scan per field | `Query::facet` `apps/debrief/crates/debrief-core/src/ports/mod.rs:165` | `crates/debrief-core/src/search_all/mod.rs:379-384`; `find/mod.rs:728-729`; `crates/debrief-ai/src/web/terms.rs:28-29` | up to 24 fields each (`MAX_VALUE_FIELDS`), both sent per 150 ms typing pause: up to 48 scans | about 110 ms per facet on 3M events (read: `apps/debrief/docs/performance.md:570`); the 2 s budget already cuts searches short; user waits | `facets(FacetsRequest{query, fields, limit}) -> Vec<ItemResult<Facet>>`, one statement (GROUPING SETS or UNPIVOT), chunked at 8 fields | High | 2.0 | before |
| DB-2 | Dashboard: one invoke and one query per tile, re-run on every data change | Tauri `dashboard_tile` `apps/debrief/app/src-tauri/src/share.rs:203` → `crates/debrief-core/src/tiles.rs:22` | `apps/debrief/app/src/app/share/dashboard/dashboard-tile.ts:174-193` (an effect per `@for` tile) | up to 64 (`MAX_TILES`); sample profile 10; live tail re-runs | invoke + IPC + scan per tile; live | `dashboard_tiles({query, tiles}) -> Vec<ItemResult<TileData>>` through DB-1 and DB-4 | High | 1.25 | before |
| DB-3 | Outliers analysis: 2 facets and 2 counts per candidate field | `facet` | `crates/debrief-core/src/analysis/outliers.rs:221-222,241-256` | up to 60 fields: 120 scans | analysis job the user waits for | DB-1 `facets`, or one pass with `count(*) FILTER (WHERE subset)` | High | 0.5 | before |
| DB-15 | Plugin sniff: one IPC call per file per remote importer, each copying up to 64 KiB | `PluginRequest::Sniff` `crates/debrief-plugin-host/src/remote.rs:183-190,259-266` | `crates/debrief-core/src/importers/mod.rs:150-176`, per file from `ingest/plan.rs:145,164`; `parser/set.rs:317-328` | 17 remote importers x 618-680 files per bundle (read: `performance.md:121,1126`), up to 100,000 files | 17 round trips and about 1 MiB of copies per file; import, user waits | `SniffMany{file, head, importers}` per plugin process, plus the manifest's `extensions` pre-filter (`manifest.rs:84`) | High | 1.75 | before |
| DB-4 | Charts: 11 invokes for the Monitor on every live change, one per chart card | Tauri `query_chart` `apps/debrief/app/src-tauri/src/workspace_commands.rs:432` | `app/src/app/tools/monitor/monitor-charts.store.ts:89` (`Promise.all` over `MONITOR_METRICS`); `app/src/app/metrics/chart-card.ts:215` | 11 always (Monitor); one per card | invoke + IPC + aggregate scan each; live | `query_charts(Vec<ChartRequest>)`; the Monitor's 11 as one scan with 11 aggregates | Med | 1.5 | before |
| DB-5 | Markers manager: one count query per rule | `MatchCounter.count` → `queryPage({limit: 1})` `app/src/app/context/match-count.ts:12-16` | `app/src/app/markers/markers-manager.ts:243-252` (`Promise.all`) | 6 default rules + plugin kinds + user rules | invoke + IPC + count scan each; view open | `count_many(Vec<Filter>)`: one statement, `count(*) FILTER (WHERE ...)` per rule | Med | 1.0 | before |
| DB-6 | Live alert rules: one page query per rule per tail tick | `store.page` | `crates/debrief-core/src/live.rs:328-346` | user rules (none by default) | windowed count each; live | DB-5 `count_many` + `min(ts)` | Low | 0.25 | before |
| DB-8 | Tantivy commit (fsync) per deleted source | `Fts::delete_source` `crates/debrief-store-duck/src/fts.rs:675` (commit in `with_writer` `:643-650`) | `crates/debrief-store-duck/src/lib.rs:1277-1281,1321`; `tail.rs:248-252`; `crates/debrief-core/src/live.rs:171-175` | every source of a changed archive (618-680) | commit per id; refresh and live | `delete_sources(&[u32])`, one commit (model: `delete_blocks` `fts.rs:685`); `Ingest::remove_sources` | Med | 0.75 | before |
| DB-9 | Refresh: one `SELECT` per changed path under the write lock | `prepare`/`query_map` in the loop | `crates/debrief-store-duck/src/lib.rs:1249-1268` | changed paths per refresh | one statement each | one query over `unnest(?::VARCHAR[])` | Low | 0.25 | before |
| DB-10 | Fingerprints: one autocommit `INSERT` per imported file, **no transaction** | `crates/debrief-store-duck/src/lib.rs:810-821` (`record_fingerprints`) | `lib.rs:777`, end of every import | 618-680 per bundle; 100,000 in the ADR 0051 bench | commit + WAL sync each; import, user waits | one transaction, multi-row INSERT chunked at 200 as `encodings::record_many` (`encodings.rs:40-60`) | Med | 0.25 | before |
| DB-11 | Compaction: one autocommit `UPDATE` per segment, **no transaction** | `crates/debrief-store-duck/src/compact.rs:206-208` | the same loop | about 430 segments at 56M events (131,072 rows each) | commit each; background | one `UPDATE ... FROM (VALUES ...)` in one transaction | Low | 0.25 | before |
| DB-18 | Diagnostics report thumbnails: one invoke and one zip re-open per snapshot | Tauri `diagnostics_report_image` `app/src-tauri/src/activity/report/open.rs:555-565` (`read_image` re-opens the zip, `:362-367`) | `app/src/app/diagnostics/diagnostics-report.store.ts:57-59` (`Promise.all`) | up to 50 (`MAX_SNAPSHOTS`) | invoke + zip parse + base64 each; user waits | `diagnostics_report_images(ids)`, one zip open | Med | 0.5 | before |
| DB-19 | Error snapshots: one invoke per image on list refresh, full-size base64 | Tauri `read_error_snapshot` `app/src-tauri/src/snapshots.rs:718` | `app/src/app/diagnostics/snapshots.store.ts:36-44,55` | 50 by default, up to 500 | invoke + file read + full base64 each; user waits | `read_error_snapshots(ids)` returning thumbnails | Med | 0.5 | before |
| DB-12 | Redacted copy: one scan of events per source | `crates/debrief-store-duck/src/redaction.rs:116-132` | the same loop | every ready source (618-680) | one scan each; export job | one `ORDER BY source, line, id` stream that switches output file on source change | Med | 0.75 | follow |
| DB-14 | Survey sampling: up to 4 offset page queries per source group | `store.page` with offset | `crates/debrief-core/src/survey/sample.rs:77-87,119-133` | up to 64 groups x 4 | IPC + offset re-scan each; background job | one `row_number() OVER (PARTITION BY group)` query, streamed | Med | 1.0 | follow |
| DB-20 | Small per-item UI loops: snapshot delete, dumps/captures/video open and close, watched folders | `snapshots.rs:724` and the stores' commands | `snapshots.store.ts:76`; `dumps/dumps.store.ts:114-116,164-166`; `profiling/captures.store.ts:126-128,189-191`; `video/video.store.ts:248-250`; `watched/watched-folders.store.ts:52-57` | user-picked (small); deletes up to 500 | invoke each | `delete_error_snapshots(ids)`, `open(paths)`, `replace_watched_folders` | Low | 0.75 | follow |
| DB-21 | Cancel matching jobs: one `jobs_cancel` invoke per id | Debrief's `jobs_cancel` | `apps/debrief/app/src/app/jobs/job-cancel.ts:16` | active jobs of a window (small) | invoke each | `jobs_cancel_many(ids)` | Low | 0.25 | follow |
| DB-7 | Names rebuild reads events once per look-alike group | `store.page` via `events_naming` `crates/debrief-core/src/names/build.rs:120` | `names/build.rs:176-230` (4 threads) | up to 200 groups | 16 reads took 2.9-7.2 s (read: `performance.md:285-295`, which already names one scan for all rare names as next); background | one scan with an OR of names, `QUALIFY row_number() ... <= 30` | Med | 1.5 | measure first (D-DB2) |
| DB-13 | Catalogue build: one source per turn, repeating 5 tidy `DELETE ... NOT IN` and listings each turn | `crates/debrief-store-duck/src/catalogue.rs:662-711` | `run` → `step`, re-armed by `turns.rs:61-65` | every source; at least 8 statements per source, tidy grows with N | background, competes with queries | tidy once per pass, K sources per turn; a deliberate memory and interrupt design | Med | 1.5 | measure first (D-DB2) |
| DB-16 | BLG import: one `relog` per .blg | `crates/debrief-plugin-windows/src/blg.rs:31-55` | per BLG file through `BinaryImporter::import` | unknown | spawn, small next to relog's own conversion | in-process PDH, or keep | Low in practice | 0.5 | D-DB3 |
| DB-17 | Collect bundle: one `wevtutil epl` per channel, one `reg export` per key | `crates/debrief-collect/src/collector.rs:431-449` | `collector.rs:270-283,295-305` | 2 channels by default; keys user-listed | spawn each; user-started job | in-process `EvtExportLog`, `RegSaveKeyEx` | Low in practice | 1.0 | D-DB3 |

Recurring inside Debrief: per-item autocommitted store writes. bda5389d8 (2026-10-29) fixed the indexer's per-segment
UPDATE (`fts.rs:841`); its message says compaction already ran in one transaction, but `compact.rs:206-208` (DB-11) and
`lib.rs:810-821` (DB-10) are autocommit loops today.

Checked, not findings: `config-diff.store.ts:225` and `hotspots.store.ts:100` (pages, not items);
`native-backend.ts:57` (2 modes); `find/mod.rs:295,575,621` (at most 3-4 steps); prepared statements reused inside one
transaction in `tail.rs`, `catalogue.rs`, `skew.rs`; `debrief-symbols/src/fetch.rs:244` (one GET per PDB is the protocol,
on one shared agent); `alias_job.rs:200` (already chunked). Good models: `Fts::delete_blocks`, the FTS indexer step,
`encodings::record_many`, `catalogue::commit_tail`, `ports::events(EventsRequest)`, `dumps.sniff(paths)`,
`export_diagnostics(path, ids)`.

## Pitcrew (release order 4)

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d | When |
|---|---|---|---|---|---|---|---|---:|---|
| PC-1 | Dashboard and Activity ask the service per runner, each over a new pipe connection | `apps/pitcrew/app/src-tauri/src/commands.rs:269` (`Request::History`), `:317` (`Request::Metrics`); connect + hello + frame + close per request (`service_client.rs:137-143`) | `apps/pitcrew/app/src/app/features/dashboard/dashboard.store.ts:52-62` (1 + 2N invokes per refresh, every 60 s); `features/activity/activity.store.ts:23-28` | up to 16 runners (`crates/pitcrew-core/src/config.rs:26`) | invoke + pipe connect + hello to the LocalSystem service each; user waits | `History{runners}` and `Metrics{runners}` with per-runner results (protocol bump), or keep per-runner requests on one connection (D-PC1) | Med | 1.5 | before |
| PC-2 | Gitea job lookups: one HTTPS request per job, each on a new `ureq::Agent` (new TLS); parallel per-runner read-modify-write of one JSON file | `apps/pitcrew/app/src-tauri/src/gitea.rs:21-43,64-70,104-143` | `commands.rs:299-300` per runner (in parallel through PC-1's `Promise.all`); `run_details` per busy runner from `dashboard-page.ts:175` | up to 25 lookups per runner per refresh x 16 runners | HTTPS + TLS handshake each; user waits. **Possible bug:** the last writer of `job-outcomes.json` wins (read from code, not reproduced) | one shared `ureq::Agent`; one outcomes pass for all runners in one command (fixes the race) | High | 1.0 | before |
| PC-3 | "Update all" applies per runner with a toast each | `Request::ApplyUpdate` (`crates/pitcrew-service/src/supervisor.rs:1646-1667`) | `apps/pitcrew/app/src/app/features/updates/updates-page.ts:213` | up to 16 | invoke + pipe connect + toast each | `ApplyUpdate{runners}` with `ItemResult` per runner, one summary toast | Low | 0.5 | follow |
| PC-4 | Credential-store presence checks per server and per channel | `commands.rs:572` (`has_api_token`), `hasChannelSecret` (`core/tauri-backend.ts:274`) | `features/settings/tokens-section.ts:129`; `features/settings/channels/channels.store.ts:43` | up to 16 + 8 | invoke + OS credential lookup each; Settings open | `has_secrets(keys) -> Vec<bool>`, and `has_many` on paddock-secrets' vault port | Low | 0.5 | follow |
| PC-5 | Update check fetches the same release URL per runner | `crates/pitcrew-service/src/work.rs:49-64` | `supervisor.rs:1005-1033` | up to 16, daily | HTTPS each; background | cache per (kind, channel) for one check window | Low | 0.25 | follow |

Good models: `Request::Status` returns every runner in one snapshot; `subscribe` keeps one long-lived connection.

## Spotter (not in the current release order: follows)

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d |
|---|---|---|---|---|---|---|---|---:|
| SP-1 | "Save imported" saves each draft with its own `save_watch`, each rewriting the whole config with fsync, backup and rename, and pushing all N summaries | `apps/spotter/app/src-tauri/src/commands.rs:135` → `crates/spotter-engine/src/engine.rs:537-604`; `config_store.rs:82-102` | `apps/spotter/app/src/app/features/editor/editor.store.ts:265-277` | up to 500 (`MAX_WATCHES`, `crates/spotter-core/src/limits.rs:14`) | O(N^2) bytes written, N fsyncs; user waits | `import_watches(drafts) -> Vec<ItemResult<WatchSummary>>`, one save, one emit | High | 1.0 |
| SP-2 | "Mark all seen": one acknowledge invoke per watch, unbounded `Promise.all` | `commands.rs:198` → `engine.rs:646-661` | `apps/spotter/app/src/app/state/watches.store.ts:114` | up to 500 | invoke + autocommit UPSERT (`db.rs:204-213`) + full push each | `acknowledge_many(ids)`, one transaction, one emit | High | 0.5 |
| SP-3 | Every change pushes the whole watch list and rebuilds the tray; "Check all" gives at least 3N full pushes | `engine.rs:234` `emit`; `apps/spotter/app/src-tauri/src/lifecycle.rs:148-163` | `check.rs:79,107,442`; SP-1, SP-2 | up to 500 | O(N^2) IPC and tray work | a dirty flag with one push per tick (1 d), or adopt `paddock-live`, whose header names Spotter as a planned adopter (2.5 d, D-SP2) | High | 1.0 |
| SP-4 | One avatar invoke per watch row; each store trims the whole cache directory | `commands.rs:437` → `crates/spotter-engine/src/avatar.rs:193-240,289`; trim `:164-179` | `app/src/app/state/avatars.store.ts:29-37` per row of `watches-page.html:49-54` | up to 500 on first open | invoke + 2 file reads; misses add an HTTP fetch and an O(N) trim. **Possible bug:** a shared `<key>.tmp` name races (`avatar.rs:182-188`) | `watch_avatars(ids)`, misses fetched in the background and pushed, one trim per batch | Med | 1.0 |
| SP-5 = CR-3 | One Chromium cold start with a throwaway profile per rendered watch | `apps/spotter/crates/spotter-render/src/adapter.rs:91-97` (`Chromium::launch`, `crates/paddock-chromium/src/launch.rs:606`) | `crates/spotter-engine/src/pipeline.rs:148-151` ← `check.rs:142-176` (up to 4 at once) | rendered watches, up to 500 | a browser process tree each; 8 Edge cold starts took about 20 s on Windows (read: `apps/spotter/docs/performance.md:20`) | one browser per pass with an isolated CDP browser context per watch (`Target.createBrowserContext`, not in paddock-chromium today); changes Spotter ADR 0002 (D-SP1) | High | 2.5 |

## Shared crates and ui (beyond G1-G6)

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d | When |
|---|---|---|---|---|---|---|---|---:|---|
| UI-1 | One Tauri invoke per forwarded webview log line, in every app | `ui/projects/paddock-ui/shell/diagnostics/src/webview-log.ts:28-38` → `crates/paddock-diag/src/tauri.rs:466` → `crates/paddock-log/src/webview.rs:16` | every `forward()` from `error`, `unhandledrejection` and `ErrorHandler` (`:41-77`) | up to 500 per window (`MAX_FORWARDED_LINES`), in bursts during error storms | invoke + log write per line, competing with UI IPC when a page misbehaves | buffer and flush every about 250 ms or 50 lines and on `pagehide`: `log_from_webview{lines}` (model: plugin `Host::log(&[String])`, `crates/paddock-plugin-api/src/serve.rs:89`) | Med | 1.0 | with Debrief (all apps get it) |
| TD-2 = CR-4 | One serial `cargo metadata` per Cargo workspace on every graph read | `crates/paddock-repo-graph/src/cargo.rs:9,111-140` | `cargo_graph` `cargo.rs:60-79` ← `tools/paddock-dev/src/plan.rs:825` | 25 workspaces; 0.1-0.6 s each (`docs/benchmarks.md:1207,1237`) | 2.5-15 s per plan; CI plan job, each app's plan job, pre-push | cache keyed by the blob ids of the Cargo.toml, lock and `.cargo/config` files (cargo has no multi-workspace mode by design); ADR 0095 D2's `repo::once` is accepted but not built | High | 1.25 | foundation (counted with the dev tools, WP-11) |
| CR-6 | Test reaper takes one process-table snapshot per root (test-only, latent) | `crates/paddock-proc/src/tree.rs:254-256` | `crates/paddock-testkit-start/src/reap.rs:332-336` | 1 root today | 14-18 ms per snapshot | one `snapshot()`, then `usage::descendants(&table, r)` | Low | 0.25 | follow |
| CR-7 | cargo-paddock-plugin: one `cargo build` per target triple | `crates/cargo-paddock-plugin/src/build.rs:71-100` | the same loop | a few targets | heavy cargo spawn each; dev tool | one `cargo build --target a --target b` (multi-target since Cargo 1.64, from the cargo docs as remembered) | Low | 0.5 | follow |
| UI-2 | paddock-e2e helpers make one Playwright round trip per element (test-only) | `ui/projects/paddock-e2e/src/grid.ts:172-181`; `dialog-fits.ts:60-75`; `size-to-content.ts:118-125`; `keyboard.ts:185-201` | 11 grid/dialog and 2 keyboard call sites | up to 1,400 per `headerCovers` call (code bounds) | one CDP round trip each (not measured) | one `locator.evaluateAll` returning all rects | Low | 0.75 | follow |
| CR-5 | Windows service registration runs about 13 `sc.exe`/`reg.exe` | `crates/paddock-service/src/backend/windows.rs:281-296,425,470-479` | the same loops | fixed about 13 per install | about 0.6 s per install; rare | in-process SCM API; **keep**: the sc.exe choice is deliberate (`windows.rs:1-4`) | Low | (2.5) | not recommended |

Checked, not findings: paddock-secrets (all looping callers have N of 1 to a few), paddock-settings (`set(id, values)` is
already per section), workbench layouts (3 keys), notify (`notifications_dismiss_many` exists), paddock-plugin-api
`HostFile` (bounded chunks on purpose), paddock-proc per-pid readings (ADR 0024 rule 4, on purpose), paddock-siblings
(`Import{files}` is bulk), paddock-testkit `write_program` (loops of 2).

## paddock-dev, scripts and conformance (beyond G1-G2)

Every Gitea HTTP call in paddock-dev is a `curl` process with a new TLS connection (`tools/paddock-dev/src/wait.rs:482-575`).

| Id | Finding | API | Looping caller | N | Cost and path | Bulk shape | Sev | d |
|---|---|---|---|---|---|---|---|---:|
| TD-1 = CR-1 | Plan's C100 content rule: 2 `git show` per changed `.rs`/`.ts` file | `crates/paddock-repo-graph/src/history.rs:115-119`; closure `tools/paddock-dev/src/plan.rs:891-905` | `plan.rs:466-479` | 240 files over the last 40 main commits; 791 for train 13 against its base: about 1,582 spawns, 18-21 s on the Mac | CI plan job, app plan jobs, pre-push, member gate | prefetch through G2 (counted in G2) | High | (G2) |
| TD-3 | `GitGitea` one `git show` per .gitea file and per conformance script | `tools/paddock-dev/src/plan_repo.rs:111-143` | `tools/paddock-dev/src/plan_gitea.rs:179-201,226-247,308-309` | up to 213 when a workflow, action or shared script changes | CI plan, pre-push | `ls-tree` already gives the oids: one `cat-file --batch` (G2) | High | 0.5 |
| TD-4 | `disk scan`/`disk clean`: 5 git spawns per folder, 2 of them the same each time | `tools/paddock-dev/src/disk/state.rs:134-185` | `disk/scan.rs:86-87,136`; `disk/clean.rs:82-100` | 270 worktrees today | dev tool | discover and `worktree list` once; one `rev-list <heads> --not ...` answers merged and unpushed | High | 0.75 |
| TD-5 | `worktree list`/`clean`: 4 git spawns per worktree, `status` again in clean | `tools/paddock-dev/src/worktree/inventory.rs:72-131`; `worktree/git.rs:224-245` | `inventory.rs:104`; clean `:236-255` | 270 | dev tool | base rev-parse once; one `rev-list`; reflog read in process; reuse `status` | High | 0.75 |
| TD-9 | Gitea HTTP: one curl process and TLS per run, job list and log | `CurlHttp` `tools/paddock-dev/src/wait.rs:566-575`; `ci_runs/client.rs:221-258` | `ci_runs/flakes.rs:385-470`; `ci/dispatch.rs:170-187,464-470` (busy probe inside the poll loop); `schedule_skip.rs:194-210`; `record.rs:145-160`; `pr/ledger.rs:151-157` | unfinished runs over up to 4 pages of 50; flakes all runs in a window | train path (dispatch, runner-wait) | `Http::get_many(&[url])` as one curl with several `url`/`output` pairs (connection reuse) | Med | 1.25 |
| TD-6 | pre_push base choice: merge-base or cat-file per merged remote-tracking ref | `tools/paddock-dev/src/pre_push/mod.rs:853-879` | the `for-each-ref --merged` loop | 118 merged refs; up to about 236 spawns when `ls-remote` fails | pre-push hook | `for-each-ref --merged=<anchor>` per anchor; one `cat-file --batch-check` (G2) | Med | 0.5 |
| TD-10 | `pr plan-status`: up to 3 git spawns per ref, through an unbounded `.output()` | `tools/paddock-dev/src/pr/mod.rs:155-225` | `pr/plan_status.rs:160-197,271` | refs in plan tables | dev tool; no timeout | one `cat-file --batch-check` + one `rev-list`, on the bounded runner | Low | 0.5 |
| TD-7 | queue `relabel` re-reads all labels: 3+ curl per PR | `tools/paddock-dev/src/queue/real.rs:96-116` | `queue/tick.rs:391-393,629-643` | 3-4 members | queue tick | read labels once per tick | Low | 0.25 |
| TD-8 | queue `verify_train`, `reached_workflows`: git per member and per app | `queue/real.rs:655,712,740` | `real.rs:691,739-742` | 3-4 members, 9 apps | queue tick | one `cat-file --batch` (G2); `merge-tree` per merge is inherent | Low | 0.25 |
| TD-12 | C129 re-reads the base exception list per current exception | `tools/paddock-dev/src/check/c129.rs:164-175` | `c129.rs:538-542` | 20 entries, only when the list grew | conformance | read the base list once | Low | 0.25 |
| SC-2 | C85 `git blame -L n,n` per owed row | `conformance/src/checks/C85-features-register.mjs:443-452` | `:318-320` | 21 rows | conformance | one `git blame --porcelain` | Low | 0.25 |
| SC-3 | Five root checks each list tracked files themselves | C57:28-32, C110:73-77, C112:18-22, C114:88-92, C116:109-112 | runner | about 10 spawns per run | conformance on every change | one memoised `ctx.git.lsFiles()` in `conformance/src/context.mjs:42` | Low | 0.25 |
| SC-4 | C99 `cargo fetch --target t` per target | `conformance/src/util/keychain-graph.mjs:229-232` | `C99-no-keychain.mjs:269` | 2 targets x workspaces | conformance | one `cargo fetch --target a --target b` (cargo-fetch docs: the flag "may be specified multiple times", checked by the sweep at doc.rust-lang.org) | Low | 0.25 |
| TD-11 | `git hash-object --stdin` per cargo config on every `./dev` | `tools/paddock-dev/src/input_hash.rs:86-88,400-404`; shell twin `dev:339-346` | `.map(config_digest)` | 1-3 | small | in process; **not recommended** alone (the shell twin must change too) | Low | 0 |
| SC-1 | `scripts/affected.mjs` repeats TD-1 to TD-3, and its `gitea.after` has no memo | `scripts/affected.mjs:466-490,1014-1071` | `:186-194,667` | as TD-1 to TD-3 | only the `PLAN_PLANNER=node` fallback and the test oracle (ADR 0022 R4: remove with the script) | leave until it is removed | Low | 0 |

**Covered by perf/git-heavy-tests (listed, not planned here).** MC-1: merge check `Git::file_at`
(`tools/paddock-dev/src/merge_check/git.rs:155-171`) runs one `cat-file blob` per (rev, path). MC-2: detectors re-read the
same objects; a06109c92 on that branch memoises full-id reads and is not on main. MC-3: the "Not done: `git cat-file
--batch`" note (`merge_check/mod.rs:34-35`), whose blocker is G1. G1 and G2 are built on `perf/git-heavy-tests-2` (ADR 0108); the merge
check moves onto that reader there (one implementation, rule 55).

Checked, not findings: `plan_cmd.rs:53-82` (stops at the first green run); `plan_since.rs`, `triage.rs`, `verdict.rs`
(Gitea has no bulk log endpoint); gates' fmt and clippy per workspace (no root workspace by design, already on workers);
`publish.mjs` (registries publish per package); `tools/wiki` (retired, ER-30). Good models: `check_ids.rs:346-395`,
`pre_push/mod.rs:283-305` (`OnceLookups`), `queue/real.rs:519-529` and `pr/mod.rs` `fetch(branches)` (one fetch for all
branches), conformance `openTree` (`conformance/src/util/fs.mjs:24-49`).

## Guard: keeping the class fixed

**Comparable practice.**
- ESLint core `no-await-in-loop` flags `await` in a loop body and recommends starting the operations and collecting them
  with `Promise.all` (https://eslint.org/docs/latest/rules/no-await-in-loop, read 2026-11-18). It does not see N calls
  started through `Promise.all(items.map(...))`, which is the shape of SP-2, DB-4, DB-5 and DB-18 here.
- Django's `assertNumQueries(num, func)` asserts the number of queries a call makes
  (https://docs.djangoproject.com/en/5.1/topics/testing/tools/, read 2026-11-18). Rails' Bullet detects N+1 queries at run
  time and, with `Bullet.raise = true`, fails the test (https://github.com/flyerhzm/bullet, read 2026-11-18). That is,
  the common guard for N+1 is a **runtime count in tests**, not a static lint.
- GraphQL DataLoader coalesces the single-key loads of one tick into one batch call whose function returns one result
  per key, an `Error` at a key's index for that key alone (https://github.com/graphql/dataloader, read 2026-11-18).
- Google AIP-231 batch get: one call with a repeated `names` field, a documented maximum size, results in request order
  (it asks for atomic batch gets; https://google.aip.dev/231, read 2026-11-18). Microsoft Graph JSON batching: up to 20
  requests per call, each with its own status (https://learn.microsoft.com/en-us/graph/json-batching, read 2026-11-18).
  Paddock's principle asks for per-item errors, as Graph and DataLoader do.
- git's own bulk interface: `cat-file --batch` and `--batch-command` read object names on stdin from one process
  (https://git-scm.com/docs/git-cat-file, read 2026-11-18).

**Options, cheapest first.**

| Option | What it catches | Misses | Cost (d) |
|---|---|---|---:|
| A. Review checklist line plus a Paddock rule (G6) | anything a reviewer sees | depends on the reader | 0.25 |
| B. Turn on ESLint `no-await-in-loop` in the shared preset, report-only first (ER-41) | `for (...) await backend.x()` (PW-1, SP-1, PC-3, DB-20, DB-21) | `Promise.all(xs.map(...))`, `forkJoin`, per-row components (DB-2, SP-4); also flags harmless sequential awaits | 0.25 |
| C. Custom ESLint rule `paddock/no-backend-call-in-loop` in `eslint/src/rules` (G5): a call on an injected backend, port or `ipc.invoke` / `invoke` inside a loop body, or inside the callback of `.map`/`.forEach` passed to `Promise.all`/`forkJoin`; opt-out `// per-item: <reason>`; RuleTester cases and a positive control (ER-33); report-only until its first green run (ER-41) | the Angular half of this inventory | per-row component effects (DB-2, SP-4: one call per rendered child) | 1.0 |
| D. Runtime budget tests on the counting seams (G3): every bulk command added by this plan gets a test that N items cost at most K starts, statements or invokes, as ER-20 already requires | Rust and TS, including per-row components and per-item spawns | only commands that have a test | in each finding's estimate, plus G3 1.0 |
| E. A static Rust check (conformance or a dylint lint) for spawn, `execute`/`query` or IPC send inside a loop body | Rust loops | indirect calls through helpers; many false positives (prepared statements in one transaction are fine) | 2-3, not recommended |

**Recommendation: A + C + D.** A is nearly free. D is the common-practice guard (Django, Bullet) and is already
required by ER-20; G3 makes it one assertion per command in every app. C covers the Angular side statically, where
most user-waited findings start, for 1 day. B (0.25 d) is the cheaper option instead of C: it catches about half of the
TypeScript shapes found here and flags harmless awaits too. E is not worth it: Clippy has no custom lints without
dylint, and the false-positive rate would be high.

## Work packages, by release order

Each package starts with its failing budget test (ER-11, ER-20), lands through a train, and adds a docs/benchmarks.md or
app performance.md row for any number it quotes (rule 48).

| WP | Contents | d | Lands |
|---|---|---:|---|
| WP-0 | G1 (stdin runner), G3 (spawn counter, invoke stub, invoke cost row), G4 (`ItemResult`), G5 (ESLint rule, report-only), G6 (rule and checklist) | 3.25 | first: foundation, before Dyno |
| WP-1 Dyno | DY-1, DY-2, DY-5, DY-7 (live path, lock) | 3.0 | before Dyno's tag |
| WP-2 Dyno | DY-3, DY-4 (merge the smartctl copies), DY-6, DY-8 | 2.0 | after |
| WP-3 Pitwall | PW-1, PW-2 | 1.75 | before Pitwall 1.1 |
| WP-4 Pitwall | PW-3, PW-4 | 1.0 | after |
| WP-5 Debrief store batch | DB-1 to DB-6 (`facets`, `count_many`, `query_charts`, `dashboard_tiles`) | 6.5 | before Debrief 1.2 |
| WP-6 Debrief writes | DB-8, DB-9, DB-10, DB-11 | 1.5 | before Debrief 1.2 |
| WP-7 Debrief import and images | DB-15, DB-18, DB-19, UI-1 (shared, all apps) | 3.75 | before Debrief 1.2 |
| WP-8 Debrief rest | DB-12, DB-14, DB-20, DB-21; then DB-7, DB-13, DB-16, DB-17 after measurement (D-DB2, D-DB3) | 2.75 (+4.5) | after |
| WP-9 Pitcrew | PC-1, PC-2 | 2.5 | before Pitcrew's next release |
| WP-10 Pitcrew | PC-3, PC-4 (with paddock-secrets `has_many`), PC-5 | 1.25 | after |
| WP-11 dev tools | G2 with TD-1, TD-3, TD-6, TD-8, TD-10; TD-2; TD-4 + TD-5; TD-9; TD-7, TD-12, SC-2, SC-3, SC-4 | 8.5 | foundation, any time (G1 and G2 are owned by perf/git-heavy-tests-2) |
| WP-12 Spotter | SP-1, SP-2, SP-3, SP-4, SP-5 (CR-3 contexts in paddock-chromium) | 6.0 | Spotter's next release |
| WP-13 shared rest | CR-6, CR-7, UI-2 | 1.5 | after |

Before-release work in order: WP-0 3.25 → Dyno 3.0 → Pitwall 1.75 → Debrief 11.75 (incl. UI-1) → Pitcrew 2.5 =
**22.25 d**. The dev-tool work (WP-11, 8.5 d) gates no app but speeds every CI plan job and pre-push; under the current
"foundation first" order it can run as its own lane at once.

**Parallelism and cost (ER-20).** The packages touch disjoint apps and can run as parallel lanes within the machine's
load (ER-24); WP-0 goes first because every package's failing test uses G3 and G4. No package adds parallelism where a
batch removes the work; the bounded-parallel options (DY-3, DY-4, TD-2) are the cheaper common-practice choices named in
the decisions.

## Decisions for the user

- **D-G1 (guard).** A + C + D (recommended, about 2.25 d with G3) or the cheaper A + B + D (about 1.5 d; B misses the
  `Promise.all(map)` shape).
- **D-G2 (git reader).** Build G1 + G2, the shared `cat-file --batch` reader (2.0 d, recommended; measured 45x on 200
  files), or the cheaper memo plus bounded parallel spawns (about 0.5 d per site, still N spawns). An in-process reader
  (gix) is about 3-4 d plus a dependency review and buys little over one batch process.
- **D-DB1.** The full Debrief store batch (WP-5, 6.5 d) or the common-practice minimum: DB-1 `facets` plus the WP-6
  transaction fixes (about 3.5 d).
- **D-DB2.** DB-7 and DB-13: measure first (recommended), or keep as designed.
- **D-DB3.** DB-16 and DB-17: keep the per-item tool runs (recommended; the tools' own work dominates) or move to
  in-process Windows APIs (1.5 d).
- **D-DY1.** DY-3: bounded parallel `diskutil` (0.5 d, recommended) or DiskArbitration in process (1.5 d).
- **D-DY2.** DY-5 on macOS: `codesign` with a cache and the batch shape (1.0 d, recommended) or Security.framework in
  process (+1.0 d).
- **D-PC1.** PC-1: a pipe-protocol change for multi-runner History and Metrics (1.5 d) or one connection for the whole
  refresh with per-runner requests (0.5 d, common practice).
- **D-SP1.** SP-5: one browser with an isolated context per watch (changes Spotter ADR 0002, 2.5-3 d) or reuse one browser
  only within a "Check all" pass (about 1.5 d).
- **D-SP2.** SP-3: local coalescing (1 d) or adopt paddock-live (2.5 d).
- **D-CR5.** Keep `sc.exe` for service registration (recommended) or the in-process SCM API (2.5 d).
- **D-TD2.** Cache the Cargo graph by manifest blob ids (recommended) or only run the 25 `cargo metadata` calls in
  parallel.
- **Budget (ER-37).** The recommended set is about 45 agent-days against about 37.5 for the common-practice set. It goes
  beyond common practice by more than 1 day in total, so it needs the user's yes; the per-item gaps are each named above.

## Also found (not bulk, worth a look)

- Pitcrew `job-outcomes.json`: parallel per-runner read-modify-write; the last writer wins (`apps/pitcrew/app/src-tauri/src/gitea.rs:104-143`).
- Spotter avatar `write_atomic` uses `<key>.tmp` for both image and meta; two stores with the same address race
  (`crates/spotter-engine/src/avatar.rs:182-188`).
- Dyno: two smartctl loops are an unregistered copy (`dyno-platform/src/hardware.rs:374-440`,
  `dyno-platform/src/smart_reader.rs:37-75`), not in `docs/known-copies.toml` (rule 55).
- paddock-dev `RepoGit::run` (`tools/paddock-dev/src/pr/mod.rs:155-225`) runs git without a timeout.

All four are read from code and not reproduced.

## Recurring

The class is **per-item process starts or round trips where one batch or long-lived process would do**. Earlier
instances:
- ADR 0095 rows 1-2: 828 `cargo metadata` runs (as cited there; fixed in b804ac78a) and graph reads queueing on cargo's
  lock (fixed in 54f8ab5ce).
- Queue scenario tests: 6953 → 3576 git children (docs/benchmarks.md:1476-1515, 2026-11-15).
- The merge check: 33 git starts for a two-commit merge (a06109c92, perf/git-heavy-tests).
- Debrief bda5389d8: the indexer's per-segment autocommit UPDATE.
- Dyno's macOS disk counters moved from `ioreg` to IOKit, and `lsof` to libproc (Dyno ADR 0026).

This note is the redesign proposal the class needs (ER-21): G1-G4 as the shared mechanisms and the guard as the check. If
the user accepts it, it becomes an ADR.
