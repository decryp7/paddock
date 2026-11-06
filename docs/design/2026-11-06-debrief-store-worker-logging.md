# Debrief store worker logging (ER-27 gap)

- Status: Proposed design note (2026-11-06). Not reviewed. Nothing was built or run (the build lock was busy); every claim
  below is from code read with `git show` on the refs named, and every cost is unmeasured.
- Refs read: `origin/debrief/1.2-int` `852b35ac0` (the code; line numbers are at this head unless said), `origin/main`
  `87c9b8449` (Dyno's agent), ADR 0036 on `origin/docs/adr-logging-root-cause` `021dc3c8d` (Accepted, not on main), ADR 0055
  on `origin/docs/adr-family-logging` `a03173156` (Proposed), `origin/fix/debrief-report-privacy` (Debrief's report P1s,
  unreviewed), the local, unpushed branch `debrief/import-search` `ec6c0bba1` (the index step summaries).
- Related: ER-27 and rule 56 (rule 56 is only on `origin/docs/rule-logs-explain-failures`), ER-22 / rule 42 (share first),
  rule 55 / ADR 0034 (one implementation per mechanism), ER-11 (failing test first), ER-14 (no real user data), ER-20 /
  rule 51.

## The gap (verified)

The store worker installs no logger, so every `log::` record in the worker process goes to `log`'s no-op default.

- `apps/debrief/crates/debrief-store-worker/src/main.rs` and `src/lib.rs:13-33` (`main`) set the allocator, read the memory
  mode and tuning, start the plugins and call `server::run`. No `set_logger`, no `set_boxed_logger`, no `paddock_log`. A grep
  of `apps/debrief` for logger installs finds only the app's (`app/src-tauri/src/logging.rs:360`) and a test one.
- The worker's `Cargo.toml` depends on `log` but on no logger crate.
- What is lost: 61 `log::` calls (non-test, by grep) in `debrief-store-duck/src`, `debrief-store-worker/src` and `debrief-store-client/src`,
  among them `fts.rs:826` "full-text indexing failed", `fts.rs:808` "full-text index merges failed", `pos.rs:52` the
  time-order rebuild attempts, `lib.rs:440-451` and `:738` the import timings (debug), `tail.rs` live-tail failures,
  `plugins.rs:55` "plugin not installed in the store worker", plus the `debrief-core` ingest and parser lines the worker runs
  (`ingest/pipeline.rs`, `parallel.rs`, `parser/set.rs`, ...). On `debrief/import-search` (local, `ec6c0bba1`) the new index
  step summaries (`fts.rs` `step_log`, info for the first and every 20th step) and "full-text index step abandoned" are lost
  the same way.
- What does survive: the worker's standard error. The client starts it with `stderr` piped when `child_log` is set
  (`debrief-ipc/src/process.rs:358`; the app sets it, `app/src-tauri/src/engine/mod.rs:82`), and `debrief_ipc::child_log`
  writes each stderr line as a CLEF line into `<logs>/children/store-worker.log` (`child_log.rs:521-560`): the parent's read
  time as `@t`, a level guessed from words in the text (`level_of_output`, `:509`), plus the parent's own Started / Stopped /
  Crashed lines with pid and exit code or signal. Report a problem includes the tail of that file
  (`activity/report/children.rs`, 256 KiB per child, 2 MiB in all). So the worker's `eprintln!`s (`lib.rs:18`, `:31`,
  `debrief-store-client/src/protocol.rs:254`, `:260`), Rust's default panic message and native aborts reach the bundle;
  its `log::` lines do not.

ADR 0036 already named this ("Debrief store, AI and plugin workers: no logger, no panic hook: their `log::` calls (store side
about 47) are no-ops", its table of processes) and decided the mechanism (its §1). ADR 0055's adoption table repeats it for
Debrief 1.2 ("`paddock-log` in every process"). This note is the concrete package for the store worker.

Two related findings, out of this package's scope:

- **Plugins the worker starts lose their stderr.** The worker never calls `child_log::set_dir` (only the app does,
  `app/src-tauri/src/lib.rs:646`), so the child logs of the importer plugins it starts (`debrief-plugin-host/src/process.rs:91`)
  are kept in memory only (`RING_BYTES`) and die with the worker. The built-in plugin binary's stderr is `Stdio::null()`
  (`debrief-plugin-host/src/builtins.rs:68`).
- **Dyno's helper has the same gap** on `origin/main`: `dyno-helper/src/server.rs:64-92` calls `log::info!`/`warn!`, and the
  crate depends on `log` but installs no logger.

## 1. Where the worker's logs go

**Recommendation: its own daily paddock-log file, `debrief-store.<day>.log`, in the app's log folder (the folder the app's
`debrief.<day>.log` is in), which the bundle reads with a per-process share. Keep the existing stderr capture as the second
channel for what is not ours or not yet logged (native aborts, the allocator, Rust's own panic text, start failures before
the log exists).** Not over the store IPC.

| | A. Own file (paddock-log) | B. Forward over the store IPC | C. Logger that writes CLEF to stderr (captured into `children/store-worker.log`) |
|---|---|---|---|
| Worker dies (panic, abort, the memory cap's kill, OOM) | Lines written before the death are on disk: paddock-log writes each line with `write_all` on an unbuffered append `File` under a mutex (`paddock-log/src/files.rs:107`, `:170-176`), so a kill loses nothing written; the panic hook writes the panic at Error with a bounded backtrace before the process ends (`panic.rs:1-5`) | Lines not yet framed or not yet read by the app are lost; nothing can be sent before the connection exists (start-up, the library check, the bind) or after it is gone (shutdown, a peer that closed) | Lines already in the pipe are read by the parent after the death, but only while the parent runs |
| The parent is stalled or paused (the user's App Tamer stops processes, see "Webview paused startup") | Not affected | Log frames back up behind the parent; the worker blocks or drops | The pipe fills (64 KiB on macOS and Linux) and the worker's next stderr write blocks, inside whatever lock it holds (the FTS step holds the store write lock, `fts.rs` `run`) |
| Timestamps | Written at the source, UTC, microseconds (`paddock-log/src/clef.rs:43`), the same format as the app's (`app/src-tauri/src/clef.rs:64`) | Source time if sent along; otherwise receipt time | Parent's read time, milliseconds (`child_log.rs` `rfc3339`): skewed by the pipe and wrong when the parent was paused |
| Structure | CLEF with properties (`ProcessId`, `SessionId`, `Process`, `Version`, typed values) | Needs a new frame kind | A CLEF line nested as text in the parent's `@m`; level guessed from words |
| Size bound | Day budget (errors pass after it), days kept, both from Diagnostics settings | Bounded by the app's file | 1 MiB per session, oldest half dropped, 3 sessions (`child_log.rs:26-29`), shared with all stderr |
| Wire change | None | The store protocol is positional (ADR 0055 §7); a new message kind needs ADR 0049's wire version rules | None |
| Family mechanism | ADR 0036 §1 ("Every process ... installs paddock-log as the first thing in `main`"), Dyno's agent does it | None | None (ADR 0036 §1 reserves stderr capture for "child output that is not ours") |

A wins on every row the brief names. B also couples logging to the one connection whose failures the log must explain. C
is what exists for non-`log` output, and stays for that.

## 2. What to reuse (rule 42)

- **`paddock_log::Logging` / `LogConfig`** (`crates/paddock-log/src/install.rs`), exactly as ADR 0036 §1 prescribes:
  `LogConfig::new(&AppId::new("debrief", "Debrief")?, "store", VERSION).with_log_dir(&dir).with_settings(..).with_crash_marker(..)`.
  The file is `<id>-<process>.<day>.log` (`install.rs:41-48`), here `debrief-store.<day>.log`. One file per process per day,
  the day budget, days kept, enrichment (`Process`, `ProcessId`, `SessionId`, `Version`, `Commit`, `Os`), error dedupe, the
  default `Redactor`, the panic hook (chains the previous hook, so stderr and therefore the child log still get the text).
- **The template:** Dyno's agent, `apps/dyno/crates/dyno-agent/src/logging.rs` on `origin/main` (its `install`, `elapsed_ms`,
  `parent_pid`, and the test that the file and marker land where the bundle reads). The worker's module is a 60-line copy of
  its shape, not of a mechanism: the mechanism is paddock-log.
- **`paddock_diag::window::log_window(dir, prefix, budget)`** (`crates/paddock-diag/src/window.rs:61`) for the bundle: it groups
  `<prefix>*.log` by process (`debrief`, `debrief-store`) and gives each an equal share, so a chatty app cannot push the
  worker's file out (ADR 0036 WP-6a). Debrief already gains `paddock-diag`, `paddock-log` and `paddock-env` in its lockfile on
  `origin/fix/debrief-report-privacy`; this package lands after it (or adds the same lines).
- **`paddock_log::DailyFiles`** (`files.rs:57`) as the writer under Debrief's own app `FileSink`, replacing
  `tracing_appender::rolling` (see "The app's pruning would delete the worker's files" below). Same file names
  (`debrief.<day>.log`), so nothing that reads them changes, and it removes one of the pieces ADR 0036 lists as a copy.
- **Not invented:** no new IPC message, no new file format, no general rate limiter (ADR 0055 §6 proposes one in paddock-log;
  until it lands, the two loops that can repeat use a local "log on change" guard, below), no new correlation field on the
  positional wire (ADR 0055 §7).

### The app's pruning would delete the worker's files (verified in the dependency's source)

Debrief's app log uses `tracing_appender::rolling` with `filename_prefix("debrief")`, `filename_suffix("log")` and
`max_log_files(keep_files + 1)` (`app/src-tauri/src/logging.rs:68-75`). In `tracing-appender` 0.2.5 (the version in
`apps/debrief/Cargo.lock`), `prune_old_logs` (`src/rolling.rs:655-690`) keeps every file that starts with the prefix and ends
with the suffix, and parses no date when a prefix is set. So `debrief-store.<day>.log` counts as one of the app's files: the
app deletes worker days, or worker days push the app's own days out. The bundle's window has the same flaw:
`activity/report.rs:157-197` takes the two newest files whose names start with `debrief`, so a worker file can take the
app's second slot. `diagnostics::prune_old_logs` (`diagnostics.rs:131-155`) also matches by prefix, but by age only, which
is the retention we want for both. paddock-log's own pruning matches `<prefix>.<day>.log` exactly (`files.rs:129-147`,
`parse_day`, `:217`), so moving the app's sink onto `DailyFiles` fixes the first flaw and `log_window` the second.

## 3. Levels, rate limits and the lines

**Level and limits come from the app.** The worker reads the user's Diagnostics settings through the app: the level,
`max_file_mb` and `keep_files` (`diagnostics.rs` `LogLimits`) and the log folder go on its command line (see the work
package). Its environment is cleared to `KEPT_ENV` (`debrief-ipc/src/process.rs:28`), so it cannot find the folder itself,
and the app's value (portable mode, a test folder) is the single source anyway. Default Info (ADR 0036 U2). A level change
in Settings applies to the worker at its next start (decision D2 below).

**Rate limits.** paddock-log dedupes only errors (`core.rs:67-73`, once a minute with a "repeated n times" summary); warn is
not limited, and ADR 0055 §6's per-site limiter is not built. Two loops can repeat a warning: the FTS loop retries a failed
or blocked turn after 500 ms (`turns.rs:20` `BLOCKED_RETRY`), so a lasting cause (disk full, a lock held) writes "full-text
indexing failed" up to 120 times a minute per store; the time-order rebuild retries per attempt (`pos.rs:52`). Each gets a
small "log on change" state in its loop: the first failure at warn with its chain, repeats of the same cause counted, and one
line when the cause changes or the loop recovers ("recovered after n failures in m s"). The day budget (errors pass) bounds
the rest. When ADR 0055 §6 lands, the guards stay (they say "recovered", which a limiter cannot).

**Property names** follow ADR 0055 §3 (`Ms`, `Why`, `ErrorChain`, `OperationId`) and §1 (`EventId`, lower-case dotted).
`store.*` ids are owned by `debrief-store-duck` and `debrief-store-worker`, each with a capture test (ADR 0055 §1's rule for
families outside `run.*`). The worker does not write `run.*` until `paddock_startup::run_log` exists (ADR 0055 §2); its start
and exit lines use the same property names so the switch is a rename.

### Lines to keep, raise or add

So that an idle-CPU or import problem reads off a bundle. All without record content, query text, file contents or the
user's file names (paths below).

| Where (1.2-int) | Today | Change | Level |
|---|---|---|---|
| `debrief-store-worker/src/lib.rs` `main` | nothing | **Add** `store.worker.start`: `Version`, `ParentPid`, `MemoryMode`, `MemoryCapMb`, the tuning numbers (`StoreTuning` fields), `AllocatorArenas`, `LogLevel`, `OsVersion`, `Arch` | Info |
| `server::run`, after the connect | nothing | **Add** `store.worker.ready`: `Ms` since start (process start to connected), `PeerPid` | Info |
| `server::run` end, `lib.rs:31` | `eprintln!` only | **Add** `store.worker.exit`: `Why` (`peer-closed`, `stdin-closed`, `connect-failed`, `error`), `UptimeMs`, `Code`; on error also `ErrorChain`, and keep the `eprintln!` (it runs while the process ends, as Dyno's `cli.rs:70`, `:74` do) | Info; Error on failure |
| Start, after install | nothing | **Add** `store.worker.previous_panic` when its crash marker is present: the marker's `At`, `Pid`, `Location`, `Thread` (not its message, which may name data); then clear the marker | Warn |
| Store open and close (`DuckStore` open in `server.rs` dispatch) | nothing | **Add** `store.open`: store `fingerprint`, `Ms`, segment count, segments waiting for the index, data version; `store.close`: `Ms` | Info |
| `lib.rs:429-451`, `:738` | debug timings, no summary | **Raise** to one `store.import` per import: `Outcome` (`done`, `cancelled`, `failed`), sources, events, bytes, `ParseMs`, `FinishMs`, `MaintainMs`, `CompactMs`, `PosMs`, total `Ms`, segments written and replaced; keep the debug detail | Info; Warn on failure with `ErrorChain` |
| `lib.rs:440-442` compaction | info / warn | **Keep**; add `Ms` | as is |
| `fts.rs:826` "full-text indexing failed" | warn, every 500 ms retry | **Keep** at warn, behind the log-on-change guard (`store.fts.failed`, `store.fts.recovered` with `Failures`, `Ms`); the failing segment's file name is store-internal, written as is | Warn / Info |
| `fts.rs:808` merges failed | warn | **Keep** | Warn |
| `fts.rs` `step_log` (import-search) | info first and every 20th, debug the rest | **Keep**; give it `EventId = store.fts.step` and properties for its numbers | as is |
| FTS loop, idle transition | nothing | **Add** `store.fts.idle` when a run of steps ends and the writer is dropped: steps, segments, documents, busy `Ms`, wall `Ms`, and the wake causes counted (import, retry, backstop) | Info |
| `pos.rs:48-63` time-order rebuild | info / warn per attempt | **Keep**; add `Ms` and the attempt to the success line; repeats behind the guard | as is |
| Catalogue loop (`catalogue.rs:264`, `:288`) | warn | **Keep**; add a `store.catalogue.built` with `Ms` and rows | Info |
| Worker, every 10 minutes while connected | nothing | **Add** `store.activity`: process CPU ms since the last line, resident MB, and per loop (FTS, catalogue, positions, tail) the turns worked, idle and blocked since the last line. This is the line that answers "Debrief uses CPU while idle": a loop that turns without work shows as counts with no step lines | Info (6 an hour) |
| `server.rs` dispatch | nothing | **Add** `store.request.slow`: the request's variant name only, `Ms`, when over 1 s (10 s for imports, exports and rebuilds); every request at Debug | Info / Debug |
| `server.rs:183-189` caught panic | returned as an error | The panic hook already writes it (Fatal, backtrace); **add** the request's variant name on the returned error's line | Error |
| `plugins.rs:55`, `:161`, `:166` | error / warn | **Keep** (plugin ids are not user content) | as is |
| `hits.rs:81` `dir.display()`, `lib.rs:842` `left.display()` | the full path in the message | **Change**: the path relative to the store folder; the store folder itself as `fingerprint` | as is |
| `tail.rs:391` "live tail could not read an appended file" | chain may name the user's file | **Change**: the source as its id and a `fingerprint` of the path at Info; the chain with the path at Debug only | Warn |
| `lib.rs:132`, `:142` DuckDB memory limit | warn / error | **Keep** | as is |

**DuckDB error chains and content.** A DuckDB error can quote a value it could not convert. Lines on paths that read record
values (`store.import` failure, `tail.rs`, `skew.rs`) write the error's first segment (DuckDB's class, for example
"Conversion Error") as `Why` at warn and the whole chain at Debug; the FTS, IO, catalogue and checkpoint chains are written at
warn as they are, through the default `Redactor` (secrets, bounded values). Decision D3.

**Correlation.** Every line carries the worker's `ProcessId` and `SessionId`. The parent's child log has `Started
store-worker (pid N, run R)` (`child_log.rs:524`), and the worker's start line has `ParentPid`, so a reader joins app run,
worker run and worker lines by pid and time. Per-request joins wait for ADR 0055 §7's decision for Debrief's positional
wire; `store.request.slow` names the request so it can be matched by time to the app's command line.

## Parallelism and cost (rule 51)

Logging is not a bulk operation, but the import and index loops are hot paths. Every added line is per import, per store open,
per idle transition or per 10 minutes, never per event or per segment, except the existing debug lines. A disabled level costs
an atomic load (ADR 0036 "Hot paths stay cheap"). The writer is synchronous under one mutex per process; ADR 0036 WP-10 (cost
per line, before any writer thread) is still open, and this package adds about 10 lines an import, so it does not need it
first. `store.activity` reads the process's CPU time once per 10 minutes. The bundle's window is a seek per file.

## 4. Work package

**WP-SL1: the store worker logs to its own file and the bundle reads it.** Size: M, about 2.5 to 3 days
(unmeasured judgement). App-only; touches no shared crate. Lands after `fix/debrief-report-privacy` (its paddock-diag,
paddock-log and paddock-env lockfile lines) on `debrief/1.2-int`.

Files:

- `apps/debrief/crates/debrief-store-worker/Cargo.toml`: `paddock-log`, `paddock-env` (path dependencies, as
  `paddock-proc` is today), `log` with `kv`.
- `apps/debrief/crates/debrief-store-worker/src/logging.rs` (new, the Dyno agent's shape): `install(args) -> Option<LogHandle>`,
  `elapsed_ms`, `parent_pid`; the crash marker at `<data>/crash-store.json` via `CrashMarker::path_for` when the app passes
  the data folder; on install failure one `eprintln!` (reaches the child log) and the worker runs on.
- `apps/debrief/crates/debrief-store-worker/src/lib.rs`: `logging::install` first in `main` (after `--self-test`, which
  stays stdout only), the start, ready, exit and previous-panic lines; `src/server.rs`: ready, exit reason, slow requests,
  the request name on a caught panic, `store.activity`.
- `apps/debrief/crates/debrief-store-client/src/protocol.rs`: `LogArgs { dir, data_dir, level, max_file_mb, keep_files }`
  with `args()` and `from_args()` (`--log-dir=`, `--log-data-dir=`, `--log-level=`, `--log-budget-mb=`, `--log-keep=`) next to
  `MemoryMode` and `StoreTuning`; unknown or bad values are ignored with a stderr note, as `--tuning=` is (`:254-260`).
  Arguments rather than environment, because the worker passes its environment on to the plugins it starts
  (`client.rs` `WorkerConfig::env`).
- `apps/debrief/crates/debrief-store-client/src/client.rs`: `WorkerConfig.log: Option<LogArgs>` (default `None`: the CLI and
  tests unchanged); `WorkerLaunch::args` adds them.
- `apps/debrief/app/src-tauri/src/engine/mod.rs` (`WorkerHost::new`, `:63-86`) and its caller: pass `ctx.log`, the data folder
  and `diag.log`.
- `apps/debrief/crates/debrief-store-duck/src/{lib.rs,fts.rs,pos.rs,tail.rs,hits.rs,catalogue.rs}` and `src/turns.rs`: the
  table's changes, the two log-on-change guards, the loop counters for `store.activity`.
- `apps/debrief/app/src-tauri/src/logging.rs`: `FileSink` writes through `paddock_log::DailyFiles` instead of
  `tracing_appender::rolling` (same names); drop `tracing-appender` from `app/src-tauri/Cargo.toml` if nothing else uses it.
- `apps/debrief/app/src-tauri/src/activity/report.rs` (`log_files`, `log_window`, `:155-197`, `:376-391`): the window becomes
  `paddock_diag::window::log_window(log_dir, "debrief", LOG_WINDOW)` with `LOG_WINDOW` 4 MiB (ADR 0036 U1); each file keeps its
  name under `logs/`, the worker's summary says "Store worker's log (recent part)"; `activity_lines` reads only the app's
  files. The report also lists `crash-store.json` when present.
- `apps/debrief/docs/` (the app's logging or diagnostics page, if one lists the log files): add the worker's file.

Tests (ER-11: the first two are written first and fail on 1.2-int):

1. **A store-worker log line reaches the bundle** (`app/src-tauri/src/activity/report.rs` tests, no process): a temporary log
   folder with a 5 MiB `debrief.<today>.log`, an older `debrief.<day>.log`, and a small `debrief-store.<today>.log` holding an
   invented `{"EventId":"store.fts.failed",...}` line; `all_files(&inputs, "")` contains `logs/debrief-store.<today>.log` with
   that line, and the app's newest file is still there. Fails today: the window takes the two newest `debrief*` files, and the
   5 MiB app file uses the whole 2 MiB.
2. **The app's pruning keeps the worker's days** (`app/src-tauri/src/logging.rs` tests): ten `debrief-store.<day>.log` files and
   `keep_files = 2`; building the app's sink and writing one line leaves all ten. Fails today through `tracing-appender`'s
   prefix-only pruning.
3. **End to end, with the real worker** (`debrief-store-worker/tests/it/worker.rs`, beside the existing spawn tests, temporary
   folders only, ER-14): start the worker with `WorkerConfig.log` naming a temp folder, open a store with invented events, and
   assert the newest `debrief-store.*.log` has `store.worker.start`, `store.open` with `Ms`, `ProcessId` equal to the child's
   pid, and `Process = "store"`; then feed the collected folder to the report's window (test 1's helper) and find the line in
   the bundle's file list. The same test kills the worker (SIGKILL / `TerminateProcess`, through `paddock_proc`) after
   `store.open` and asserts the line is still in the file and the child log says `Crashed`.
4. **Panic**: a test-only request (behind the worker's existing test support) that panics; the file has the Fatal line with
   `Location`, the next start writes `store.worker.previous_panic` and removes the marker.
5. **Log on change**: a `FakeClock`ed unit test of the FTS guard: 100 identical failures write one warn and, on recovery, one
   `store.fts.recovered` with `Failures = 100`.
6. **No content** (ER-14, invented data): an import of invented records with a marker string in a field and a file named
   `secret-name.log`, with a value DuckDB cannot convert; the worker's file never contains the marker or the file name at Info.
7. `LogArgs` round trip and bad values ignored (`protocol.rs` tests, beside `StoreTuning`'s).

Checks to run (ER-7): `debrief-store-worker`, `debrief-store-client`, `debrief-store-duck`, the app's `activity::report` and
`logging` tests; `cargo xwin check` of the worker (the Windows kill path); Linux in the local container for test 3.

## Risks

- **Two writers' days in one folder.** Covered by tests 1 and 2; any other reader of `debrief*` files (the Logs window in the UI,
  if it lists by prefix) needs the same check. Not searched.
- **Content in error chains** (D3). The table's rule is a judgement; test 6 is the guard.
- **Panic hook on contained panics.** paddock-log's hook writes the crash marker on every panic, also one `catch_unwind` at
  `server.rs:183` contains, so "previous run panicked" can mean a request panicked and the worker went on. The line says
  `previous_panic`, not "crashed"; the parent's child log is the authority on whether the process died. A shared fix (a marker
  only when the panic is not caught, or a cleared marker on clean exit) belongs to paddock-log, not here.
- **Disk.** One more file per day: at most `max_file_mb` (Debrief's default 10 MB) times `keep_files + 1`, the same as the app's.

## Decisions for the user

- **D1. Own file, not IPC** (recommended above). Needs nothing but agreement; it is ADR 0036 §1.
- **D2. Level changes at run time.** Recommended: the worker takes the level at start; Settings says a level change reaches
  the store worker at the next Debrief start. Alternative: an additive `SetLogLevel` request on the store protocol (small, but
  a wire change under ADR 0049).
- **D3. DuckDB chains on value-reading paths at Debug only** (class at warn), as in the table. Alternative: the full chain at
  warn through the Redactor, accepting that a value can appear.
- **D4. The worker's plugins' stderr** (the first related finding): a separate package, either the worker's own child log folder
  or the plugin host on paddock-log (ADR 0036 §1). Not sized here.

## Could not settle

- The CPU-time source for `store.activity`: `sysinfo` is a workspace dependency (the worker's dev-dependencies use it); whether
  its process CPU-time API fits, or `getrusage` / `GetProcessTimes` are simpler, is for the implementer to check.
- Whether the Debrief UI's own log viewer (if any) lists files by the `debrief` prefix and would then mix the worker's lines in.
- ADR 0055 is Proposed: if review changes `EventId` or the property names, the `store.*` ids here follow it.
