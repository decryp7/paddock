# Logging for root cause: one log per process, typed start, phase, process, connect, job and error events, the bundle reads them all

- Status: Draft (2026-10-31), for the user's decisions at the end and an independent review. Not accepted.
- Date: 2026-10-31
- Related: ER-6, ER-14 (no real user data), ER-17 to ER-20 (long work, phases, never time out, parallelism and cost),
  ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rule 42 (share first), rule 51 (slow means more
  parallelism or cheaper), rule 55 / ADR 0034 (one implementation per mechanism); PRINCIPLES "Logs must not contain user
  content by default"; ADR 0008 (long operations), ADR 0012 (app shell), ADR 0020 (shared executor), ADR 0024 (background
  work), ADR 0029 (the shell owns the start), ADR 0031 (patience: stall verdicts), ADR 0033 (processes by identity);
  plans X1 (paddock-log), X2 (paddock-diag, Report a problem), X3 (crash marker).
- Evidence: a read-only audit of `origin/main` at `cd4aa0afa` (2026-10-31). Paths below are relative to the repo root.
  Counts are `grep` counts of non-test Rust (`info!|warn!|error!|debug!|trace!` and `eprintln!|println!`), raw lines.
  No user logs were read; the audit used code and test fixtures only.

## Context

**The goal (user, 2026-11-01).** From a Report-a-problem bundle alone, someone who cannot reproduce the problem finds
the root cause of a start failure, a slow or failed connect, a stuck or failed job, a crash, an update failure, or a
background-agent or service problem. Releases run on machines we cannot attach a debugger to; the bundle is all we get.

**What made it urgent.** Dyno at login takes about 30 s to connect to its agent. The question was whether the logs say
why. They do not (section "Dyno's 30 s login connect" below), and the reasons are not Dyno's alone.

### The shared pieces are good, and mostly unused

- `crates/paddock-log` (plan X1) is the family log: CLEF JSON lines (`@t` UTC with microseconds), one file per process
  per day in `AppDirs::logs()` (`<id>.<day>.log` for the app, `<id>-<process>.<day>.log` for others), a day budget
  (20 MB, then errors only) and 7 kept days (`settings.rs`, decision D2), level changeable at run time, `tracing` and
  `log` in one stream, enrichment on every line (app, process, version, commit, OS family, session id, pid;
  `install.rs` `Logging::new`), a redaction net (secret-like keys masked, content-like keys replaced by a size, values
  bounded; `redact.rs`), error dedupe with breadcrumbs, a panic hook on every thread with a crash marker for the next
  start (`panic.rs`), bounded child-stderr capture (`child.rs`) and `ipc_span` for `Window`/`OperationId`/`Command`.
- `crates/paddock-diag` (plan X2) is Report a problem: a zip the user previews and saves, built from sections; the
  built-ins are system facts, the settings file, the crash marker and a 2 MB window over the 4 newest log files named
  `<id>*.log` (`window.rs` `LOG_WINDOW`, `MAX_LOG_FILES`); every file passes a three-layer redactor (secrets, personal
  data: user names, home folders, machine name, e-mail and IP addresses; and paddock-log's field rules). It works with
  every background process down, because it reads files, not IPC.
- `crates/paddock-shell` installs paddock-log first in `run` (`logging.rs`) and wires Report a problem when the app calls
  `Builder::diagnostics` (`builder.rs:185`, `:568`).

**Who uses them** (`LogConfig::new` / `logging::install` in non-test code):

| Process | Log | In a Report-a-problem bundle? |
|---|---|---|
| Dyno app | paddock-log (shared crates' lines only) **and** its own `logging.rs` (`app.log`, Unix seconds, 256 KB) for all of Dyno's own lines | No: Dyno does not call `Builder::diagnostics`; its own bundle (agent-written) has no logs |
| Dyno agent | 0 log calls, 34 `eprintln!`; stderr is `/dev/null` when the app starts it (`link/ports.rs:119-121`) and when launchd starts it (the bundled plist has no `StandardErrorPath`). Only lock refusals reach `logs/agent.log` (`instance.rs:73`), **without a timestamp and with repeats suppressed** | No |
| Spotter app | own `applog.rs` (`app.log`, plain text, 512 KB) | No bundle at all |
| Pitcrew app / service / runners | own `applog.rs`; service `logfile.rs` (`service.log`); runner output raw | Own zip via the service's IPC; `app.log` never included; nothing from disk when the service is down |
| Debrief app | its own `tracing` + CLEF + `tracing-appender` stack (`app/src-tauri/src/logging.rs:347-366`), installed inside `setup()` **after** the main window is created (`lib.rs:629` vs `:640`); own panic hook (`lifecycle.rs:636`); children's stderr in `logs/children/<name>.log` (`debrief-ipc` `child_log.rs`) | Own bundle (`activity/report.rs`): 2 MB of the 2 newest files, child logs, OS version; redacts secrets but not paths, hosts or IPs |
| Debrief store, AI and plugin workers | no logger, no panic hook: their `log::` calls (store side about 47) are no-ops; only stderr survives, via the parent's child capture | via child logs |
| Pitwall app and server | none: 0 log calls; about 19 `eprintln!` in the app, 12 in the server; the server's launchd plist has no `StandardErrorPath`; a Windows-service-only writer (`service/windows.rs:489-500`); an in-memory `EventLog` of 2000 events never persisted | No bundle |
| Pacenotes, Sighter | no code yet; the release plan names paddock-log and paddock-diag | n/a |
| shell-demo, workbench-demo | paddock-log and paddock-diag | Yes |

### What even a paddock-log user does not get today

1. **No start banner.** Nothing writes once, at start, the OS version (only the family `macos`/`windows`/`linux` is
   enriched), the architecture, the parent and the launch source (user, login item, restored session, relaunch after an
   update, launchd/systemd/SCM, spawned by the app). `with_commit` is never called outside tests, so `commit` is empty on
   every line. `system-info.json` in the bundle has no OS version either (`paddock-diag/src/section.rs:116-124`).
2. **No phases with durations.** paddock-startup logs "Window ready" and the watchdog's verdicts
   (`paddock-startup/src/tauri.rs:311-370`), but no line says how long since the process started, and no phase says
   when it began. A 30 s gap between two lines is visible only at the resolution of the file and never attributed.
3. **Processes, connects and jobs log nothing in the shared crates.** `paddock-jobs` 0 log calls, `paddock-ipc` 0,
   `paddock-net` 0, `paddock-download` 0; `paddock-proc` 7 (only warnings on refusal). A spawn, its ready, its exit
   code, a stall verdict, a connect attempt, a job's start, phases, cancel and result leave no line unless each app
   writes one, and most do not.
4. **Errors shown to the user are not logged.** `paddock-commands` has no log call; Tauri commands return
   `Err(CommandError)` to the webview and the cause is gone. Where errors are logged, they are logged with `{e}`
   (Display) and lose the `source()` chain.
5. **No correlation.** `ipc_span` is never used in product code, so `OperationId` is never set; jobs have a `JobId`
   (`paddock-jobs/src/model.rs:11`) that never reaches a log line; nothing joins an app line to the agent or service line
   it caused.
6. **Updates are mostly silent.** `paddock-updater` logs whether updates are available in this build and that an install
   at exit failed (`tauri.rs:377`, `:417`), without the cause; the check, its answer, the download, the signature check
   and the install steps are not logged.
7. **Config fallbacks are uneven.** paddock-settings logs 4 lines; most apps do not say "settings missing, defaults" or
   "settings damaged, kept as .bad, defaults".
8. **Redaction at rest is by key only.** paddock-log's net masks secret- and content-named keys; a path, host or name in a
   message or an unlisted key is written as is and removed only when the bundle is built (paddock-diag's personal layer).
   That is the family's design today (logs stay on the user's machine; the bundle is what leaves it), but it is not
   written down as a rule, and the apps with their own loggers have no net at all.

### The pattern (ER-21)

The same bug recurs across the family: **a failure that only the user's machine shows, whose log either is not written,
is written where the bundle does not look, or says what failed but not why or after how long.** Instances found in this
audit: Dyno's agent writes to `/dev/null`; Dyno's own lines go to a file no bundle reads; Pitcrew's bundle has no
service log when the service is down and never has the app's; Spotter has no bundle; the Pitcrew app's start failure
goes only to stderr; three apps have their own logger (copies under rule 55); errors returned to the UI are lost in
every app on `paddock-commands`. Fixing these one at a time does not stop the next app from repeating them, so this ADR
proposes one mechanism and checks over every instance.

## Decision (proposed)

### 1. One log per process, always paddock-log

Every process the family ships (app, agent, server, service, worker, plugin host, sidecar that is ours, CLI when it runs
as a service) installs paddock-log as the first thing in `main`/`run`, with its process name, into `AppDirs::logs()` of
its app, so the file is `<id>-<process>.<day>.log` and the bundle finds it by prefix. A process started by a service
manager whose stderr is not ours (launchd without `StandardErrorPath`, Windows services) depends on this; its own panic
hook replaces the stderr it does not have.

- A service running as root/SYSTEM writes to a log folder the user can read (`%ProgramData%\<Name>\logs`,
  `/Library/Logs/<Name>`) with the same file names, and the app's diagnostics section reads that folder directly.
- Own loggers (`apps/dyno/app/src-tauri/src/logging.rs`, `apps/spotter/app/src-tauri/src/applog.rs`,
  `apps/pitcrew/app/src-tauri/src/applog.rs`, `apps/pitcrew/crates/pitcrew-service/src/logfile.rs`) are copies of
  paddock-log under rule 55: they are registered in `docs/known-copies.toml` (mechanism `process-log`) with a
  `delete_by` tag and replaced by paddock-log; until then their `info()` forwards to `log::info!` so the line reaches
  the shared file.
- Child output that is not ours (runner output, a browser's stderr) goes through `spawn_child_capture`, bounded, into
  the parent's log under `Child = <name>`, at debug by default (it can contain user content, section 6).

### 2. Typed events, one helper each, in paddock-log (`paddock_log::event`)

paddock-log gains a small module of typed helpers. They are thin: each formats one CLEF event with fixed property
names, so a reader (a person, Debrief, a test) can filter on them. No new dependency; no Tauri.

```rust
/// Written once by `Logging::install` (and by `LogHandle::banner` for processes that learn their start reason later).
pub struct StartFacts { pub launch: Launch, pub parent_pid: Option<u32>, pub previous_run: Option<&'static str> }
pub enum Launch { User, LoginItem, RestoredAtLogin, AfterUpdate, ServiceManager, SpawnedBy(&'static str), Unknown }

/// A phase with a duration: "Phase started" at debug when entered, "Phase finished" at info with ElapsedMs when dropped
/// or `finish`ed, "Phase failed" at warn with the error chain. Never a timeout: it measures, it does not judge.
pub fn phase(name: &'static str) -> PhaseGuard;

/// A state machine's published phase changed: logs only on a change of the discriminant, with From, To, InPhaseMs and
/// the event that caused it. Cheap enough to call on every step (a comparison, no allocation, until it changes).
pub struct Transitions<K: Copy + Eq + Debug> { .. }
impl<K: Copy + Eq + Debug> Transitions<K> { pub fn observe(&mut self, now: K, cause: &dyn Debug); }

/// Errors: the chain (`source()`, at most 8 links, each bounded), the text shown to the user and the next step offered.
pub fn error_shown(err: &(dyn std::error::Error + 'static), shown: &str, next_step: Option<&str>);
pub fn chain(err: &(dyn std::error::Error + 'static)) -> String; // "a: b: c", for `{}` in existing lines
```

Every line also carries **`UptimeMs`**, milliseconds since the log was installed (one `Instant` read per event), so a
gap between two lines in the same process is exact and readable without parsing two timestamps, and start phases line
up with no extra code. `commit` is filled from `PADDOCK_BUILD_COMMIT` by `paddock-shell::logging::install` and by the
same helper for non-shell processes.

`StartFacts` adds to the enrichment of the first line: OS name and version, kernel version, architecture, the parent pid
and name, the launch source, the log level, whether the build is portable or debug, and the previous run's verdict
(clean or crashed, from paddock-startup's marker). The OS version comes from a new `paddock_env::os::version()`
(macOS `sysctl kern.osproductversion`, Windows `RtlGetVersion`, Linux `/etc/os-release` `VERSION_ID`, all bounded and
infallible: `unknown` on failure); Dyno's `dyno-platform` facts reader is the reference and becomes a consumer.

### 3. The shared crates log their own events, so apps get them for free

| Crate | Events (fixed names) | Level |
|---|---|---|
| `paddock-startup` / `paddock-shell` | the start banner; phases `start.plugins`, `start.window`, `start.ready` with `ElapsedMs`; the watchdog's verdicts (existing) | info |
| `paddock-proc` | `Process spawned` (program file name, not path; pid; start identity), `Process ready` (ms since spawn), `Process exited` (code or signal, ms alive, the last stderr lines), `Process stalled` with the ADR 0031 verdict and its signals (ticks, CPU, run state), restarts and quarantine with the count | info; stalls warn |
| `paddock-ipc` / `paddock-service` client | `Connect attempt` (n, endpoint kind, fingerprint of the endpoint), `Connected` (ms, attempts), `Connect failed` (error kind: not found, refused, denied, version mismatch, timeout by a cap; the chain), `Disconnected` (who closed, ms connected); reconnect backoff once per change, not per try | info; repeats deduped by paddock-log |
| `paddock-jobs` | `Job queued` / `started` / `phase` / `cancel requested` / `finished` (state, ms, counts), each with `JobId` and `Kind`; the job's work runs inside a span with `JobId`, so every line it writes carries it; the subject is written only as a fingerprint | info; failures warn with chain |
| `paddock-commands` | every `Err` returned to the webview: `error_shown` with the command, the window and the code; inside `ipc_span`, so `OperationId` correlates it with the UI's log line | warn |
| `paddock-updater` | check (channel, answer, latest version, HTTP status class), download (bytes, ms, digest ok), signature result, install steps, relaunch; each failure with its chain | info; failures error |
| `paddock-settings`, `paddock-env` state files | loaded, missing (defaults), damaged (kept as `.bad`, defaults), newer (read-only) | info / warn |
| `paddock-service` | registration, status reads (the answer, ms), repair steps (existing), the service host's start and stop (existing) | info |

**Correlation across processes.** paddock-ipc frames gain an optional `op` field (the caller's `OperationId`); the
receiving side opens a span with it, so the agent's or service's lines for a request carry the same id as the app's.
Job ids are per process; a job started on behalf of a request records the request's `OperationId`.

### 4. Panics and crashes

The paddock-log panic hook is on in every process (section 1 gives it to the processes that lack it). A native crash
(SIGSEGV, an abort in a C library) does not run the hook; the next start then finds the running marker without a clean
exit and logs "previous run did not end cleanly" (paddock-startup does this for apps today, `lifecycle.rs:384`). The
diagnostics section additionally lists the **names and dates** (never the contents) of the OS crash reports for the
app's processes (macOS `~/Library/Logs/DiagnosticReports/<process>-*.ips`, Windows WER `ReportArchive` folder names),
so a reader knows a native crash happened and the user can attach the file themselves (decision U4).

### 5. The bundle reads every process, even with the background process down

- Every app calls `Builder::diagnostics`; its Report a problem is paddock-diag's. App-specific bundles (Dyno's
  agent-written zip, Pitcrew's IPC-built zip) become **sections** of it: the section asks the background process when it
  answers and otherwise reads its files from disk and says so in a note.
- The Logs section takes a **share per process**: the window (decision U1) is split across the `<id>*` prefixes found,
  newest first, so a chatty process cannot push the agent's or service's log out. `MAX_LOG_FILES` follows from it.
- `system-info.json` adds the OS version, kernel version, uptime of the machine and the app's install location kind
  (`/Applications`, translocated, portable), never serial numbers.

### 6. What may be written (redaction rules, written down)

Logs stay on the user's machine; the bundle is the only thing that leaves it, after the user's preview, and is redacted.
Both layers stay; this ADR writes the rule for the first:

- **Never at any level:** file contents, record contents, prompts, queries, clipboard, secrets (existing net).
- **Paths:** a path the app owns (its data, log, config folders, its own binaries) is written as is; the bundle's
  personal layer replaces the home folder and user name. A path the user chose or a file the user owns is written as
  `paddock_log::path_hint` (the extension, the size and a fingerprint; with Debug level, the file name). 
- **Hosts and names:** loopback and the family's own update and service hosts as is; other hosts, repository names,
  machine names and account names as a fingerprint at info, plain at debug (decision U3).
- **Child output that is not ours** (runner output, a browser's stderr): debug only, bounded.
- The default redactor gains `path`-like and `host`-like key rules and a test per rule; a conformance test logs
  synthetic personal data through every typed helper and asserts what lands in the file.

### 7. The checks (proposed numbers; the next free ones when this is accepted)

- **C59, every process logs.** Each binary target of `apps/*` and `crates/*` that is shipped (a Tauri `run`, a `main`
  of an agent, server, service, worker or plugin host; the list comes from each app's `paddock.ci.json`) calls
  `paddock_shell::logging::install` or `paddock_log::Logging::install` before its first spawn, connect or thread. Non-CLI
  product code has no `eprintln!`/`println!` (CLI output functions carry `// cli-output`). Own loggers fail the check
  unless registered in `docs/known-copies.toml`.
- **C60, every start, spawn, connect and job goes through a logged helper.** `std::process::Command::new`,
  `UnixStream::connect`, named-pipe opens and `JobRegistry::register` outside the shared crates are findings unless the
  line is marked with the phase it logs (`// logged: <phase>`); the shared crates' helpers log by construction.
- **C61, every error shown is logged.** A `#[tauri::command]` returns paddock-commands' error type (which logs through
  `error_shown`), never `Result<_, String>`; an error converted to a user message outside it is a finding.
- **A runtime check in the start smoke** (paddock-e2e / each app's VM smoke): after a start, the newest log of each
  process has the banner, `start.ready` with `ElapsedMs`, and for apps with a background process a `Connected` line; a
  forced connect failure (a fixture endpoint that refuses) produces `Connect failed` with its kind. The checks read only
  the test's own temporary log folder (ER-14).

## Parallelism and cost

- **Volume.** Steady state at info: under one line per second per process (no per-tick, per-sample, per-item or
  per-frame lines at info; those are debug or trace). A start: at most about 200 lines. A job: a fixed number (queued,
  started, one per phase, finished), never one per item; progress counts go in the `finished` line. A stall or a
  reconnect loop is deduped by paddock-log (once per minute, then "repeated n times"). The day budget (20 MB, then errors
  only) is the backstop, not the plan.
- **Hot paths stay cheap.** A disabled level costs an atomic load (`log::max_level` / the `tracing` filter) and formats
  nothing; fields are passed as values, not pre-formatted strings. `Transitions::observe` compares a `Copy` discriminant
  and allocates only on a change, so it can sit in a 100 ms supervisor loop. `phase()` reads one `Instant`. `UptimeMs`
  is one `Instant` subtraction per event.
- **The writer.** paddock-log writes synchronously under one mutex per process today. That is fine at the volume above;
  WP-10 measures the cost per line (synthetic events, release build, rule 48) and moves the file write to a bounded
  channel and one writer thread only if a line costs more than about 20 µs on the slowest CI machine or a burst blocks a
  caller. Shutdown flushes the channel (bounded wait, ADR 0031's shutdown cap).
- **Rotation.** Daily files per process, 20 MB per day and process (then errors only), 7 old days kept (decision U1
  may change the values). Old days are removed at install and at the first write of a new day, never in a hot path.
- **The bundle.** It reads a fixed window from the end of each file (a seek), so its cost does not grow with the log;
  collection runs as a job off the UI thread (existing).

## Dyno's 30 s login connect: could today's logs explain it?

**No.** What exists (at `cd4aa0afa`):

- The app writes `"Dyno <v> starting (data folder: ...)"` (`apps/dyno/app/src-tauri/src/lib.rs:218`), `"started hidden
  (flag or login item)"` (`lib.rs:269`), `"starting the agent"` only when the app starts its own
  (`link/driver.rs:150`), `"still waiting: <message>"` when the start judge stalls (`driver.rs:254`), `"agent.json names an
  agent that did not answer"` on a refused connect (`driver.rs:209`) and `"connected to the agent"` (`driver.rs:245`).
  These go to Dyno's own `~/Library/Application Support/Dyno/logs/app.log` with **whole-second** stamps
  (`logging.rs:62-68`), not to paddock-log's file, and **no bundle contains either file**.
- The supervisor's phase changes, each look's answer (discovery file present, lock held, service state), "no discovery
  file" connect failures (`driver.rs:179-184`) and the waits between looks are not logged.
- The agent logs almost nothing: the bundled launchd plist (`apps/dyno/app/src-tauri/macos/net.decryptology.dyno.agent.plist`)
  has no `StandardErrorPath`, so its `eprintln!` lines (its start with pid, failures, panics in threads without
  `catch_unwind`) are discarded; when the app starts it, stderr is `/dev/null`. The one exception, a lock refusal
  (`dyno-agent/src/lib.rs:132-135`), goes to `logs/agent.log` through `instance::note` (`instance.rs:73-91`), which writes
  **no timestamp** and **skips a line equal to the last one**, so ten refusals 10 s apart leave one undated line.

So a user's bundle shows the 30 s at best as two lines a whole number of seconds apart, in a file it does not contain,
and cannot tell these apart (all consistent with the code; none verified):

1. **launchd throttling:** the plist has `KeepAlive` and `ThrottleInterval 10`; an agent that exits early (exit 3,
   another agent holds the data folder's lock, `dyno-agent/src/cli.rs:89`; or a failure) is started again after 10 s;
   three such rounds are 30 s. Only the agent's own log could show its exits and their codes.
2. **The first look waited on `SMAppService.status`** (addressed on `dyno/login-blockers`, `2f3931a48`).
3. **A slow agent start** before the discovery file exists (store open, migrations, first sample in
   `Running::start_os`): no timings are logged.
4. **The agent or the app paused** by App Tamer or macOS at login (addressed on `dyno/login-blockers`, `586a5190d`,
   ADR 0031).
5. **Connect retries:** repeated "no discovery file" or refused connects with the look pace between them.

The lines in the Dyno fix list (the brief's report, WP-D1 to WP-D7) separate all five.

## Gaps per app (audit 2026-10-31)

P1: without it the root cause of a start failure, slow or failed connect, stuck or failed job, crash, update failure or
background-process problem cannot be found from the bundle. P2: it makes that slower or less certain. Spotter, Pitcrew,
Debrief and Pitwall counts come from two read-only sub-audits whose key claims were spot-checked (no paddock-log or
paddock-diag dependency in any of the four, Pitwall's plist, Debrief's late logger); the line references there are
secondary and must be re-read by whoever fixes them.

| Scope | P1 | P2 | The P1s in short |
|---|---|---|---|
| Shared crates | 4 | 4 | no start banner (OS version, launch source, commit); jobs, ipc, proc events unlogged; errors returned to the UI unlogged; updater check/download/cause unlogged. P2: no `UptimeMs`/phases, no correlation, Display-only errors, redaction rule unwritten |
| Dyno | 5 | 6 | agent unlogged, no panic hook, stderr discarded; app's own lines in a second file with 1 s stamps; no bundle contains any log, and the bundle needs a connected agent; supervisor phases, looks and "no discovery file" unlogged; agent start steps and exits unlogged |
| Spotter | 5 | 4 | no panic hook; no bundle; update silent; command errors unlogged; renderer spawn, ready and watchdog kill unlogged |
| Pitcrew | 7 | 6 | no panic hook (service lost on Windows); bundle omits `app.log` and has no service log when the service is down; app start failure only on stderr; connect and backoff unlogged; update silent; command errors unlogged; the service's fatal IPC error not in its log |
| Debrief | 5 | 7 | workers have no logger or panic hook; the logger starts after the window and plugins; UI errors unlogged (33 `map_err(to_string)` sites); job failure reason unlogged; worker connect without attempts, durations or stall verdict |
| Pitwall | 3 | 3 | no log or panic hook in any process; no bundle; connect, update and command errors unlogged |
| Pacenotes, Sighter | 0 | 0 | no code yet; their plans name paddock-log and paddock-diag, and C59 holds them to it |

## Work packages

Dyno first (it releases next), then the shared mechanism, then each app before its tag.

| WP | What | Size | Owner |
|---|---|---|---|
| D1-D7 | Dyno's fix list (the report that comes with this draft): agent on paddock-log, app lines into paddock-log, transition and look logging, connect/exit lines, a bundle with logs | S each, about 2 days total | Dyno lead |
| WP-1 | `paddock_log::event`: `StartFacts`, `phase`, `Transitions`, `error_shown`, `chain`, `UptimeMs`; tests with `FakeClock` | M | paddock-log |
| WP-2 | `paddock_env::os::version()` and the banner in `paddock-shell::logging::install`; `commit` from the build | S | shared |
| WP-3 | paddock-proc and paddock-ipc events (spawn, ready, exit, stall verdict, connect attempt/outcome) | M | shared |
| WP-4 | paddock-jobs events and the `JobId` span; paddock-commands `error_shown` and `ipc_span` in `route` | M | shared |
| WP-5 | paddock-updater and paddock-settings events | S | shared |
| WP-6 | paddock-diag: per-process log shares, OS facts, crash-report names, sections for background processes that read files when IPC is down | M | paddock-diag |
| WP-7 | Redactor key rules for paths and hosts, `path_hint`, the synthetic-data conformance test | S | paddock-log |
| WP-8 | C59, C60, C61 and the start-smoke log assertions | M | conformance |
| WP-9 | Per app, before its tag: Spotter, Pitcrew (app, service, runners), Debrief, Pitwall onto paddock-log and paddock-diag; own loggers registered then removed | M-L per app | app leads |
| WP-10 | Measure the cost per line and per burst (rule 48); move to a writer thread only if over the bound | S | paddock-log |
| WP-11 | Rule 56 (proposed wording below) into `docs/rules.md` and every app's `AGENTS.md` copy, lesson in `docs/lessons.md` | S | steward |

**Proposed rule 56.** *Every process logs through paddock-log, and the bundle explains failures on its own.* Every
start, spawn, connect, job, state change and failure is logged with a timestamp, its phase, its duration and its cause
chain; every error shown to the user is logged with what was shown; Report a problem contains every process's log even
when the background process is down; logs never contain user content (section 6). New features land with their log
lines. *Why:* releases run where we cannot attach a debugger; Dyno's 30 s login connect left no line that said why.

## Risks

- **Log noise hides the cause.** Mitigated by the volume rules, info vs debug, and the dedupe; the start smoke asserts a
  ceiling on start lines.
- **User content slips into a message string.** The key rules cannot see inside a formatted message. Mitigated by typed
  helpers (values as properties, not text), `path_hint`, the synthetic-data test, and the bundle's personal layer.
- **Churn in shared crates during releases (ER-1).** WP-1 to WP-5 add lines and helpers without changing behaviour; each
  is its own branch, and an app adopts the shared events by upgrading, not by code changes.
- **Root/SYSTEM services' log folders.** A user-readable log folder for a privileged service needs care with
  permissions (no user-writable path a privileged process trusts); Pitcrew's design note must cover it.

## Decisions for the user

- **U1. Retention and bundle size.** Keep 20 MB per day and process, 7 days (D2)? The bundle's log window: 2 MB today;
  proposed 4 MB split across processes (a Dyno bundle then has about 2 MB app and 2 MB agent).
- **U2. Default level.** Info everywhere (proposed), with Debug for a support case from Settings; or Debug for the first
  week after an update.
- **U3. Hosts and names at info.** Fingerprint (proposed) or plain, given the bundle redacts IPs and machine names anyway.
- **U4. OS crash reports.** List only their names and dates (proposed), or offer them as optional bundle files the user
  ticks.
- **U5. Own loggers.** Replace before each app's next tag (proposed), or register with `delete_by` one release later.

## Appendix: Dyno fix list before the 1.0 tag (a patch plan, not implemented)

Small and safe: no behaviour changes, only log lines and two wirings. Paths are relative to `apps/dyno/`. Lines marked
**[login-blockers]** are being added on `dyno/login-blockers` (service look and `SMAppService.status` timing, the
discovery file, the lock probe, look durations, successful connects, the agent's start, bind and `agent.json` times, the
login Apple Event, connected to first data). They are listed so the review can check them, not to be done twice. They
need D1 and D2 to land in a file the bundle reads.

**D1. The agent logs through paddock-log** (`crates/dyno-agent`; adds `paddock-log`, `paddock-env` and `log` with `kv`).
- `src/cli.rs` `run()` (about line 227), first statement: install `Logging::new(LogConfig::new(&AppId::new("dyno",
  "Dyno")?, "agent", VERSION).with_log_dir(..))`, where the folder is `AppDirs::for_app(..).logs()` (the app's folder,
  `~/Library/Logs/Dyno`, so the file is `dyno-agent.<day>.log` and the bundle's `dyno*` prefix finds it), or
  `dirs.logs()` when `DYNO_DATA_DIR` is set (tests stay in their folder, ER-14). Only `run`; `status`, `once`, `bench`
  and `service` keep printing. This also gives the agent the panic hook and a crash marker.
- Lines (info unless said): `Agent starting` with `ParentPid`, `ParentName` (launchd when the parent is pid 1),
  `OsVersion`, `Arch`; `Agent lock taken` with `ElapsedMs`, or warn `Agent refused: another agent holds the data folder`
  with the holder's pid and `ExitCode = 3` (`src/lib.rs:132-135`); `Agent store open` with `ElapsedMs` after
  `Shared::build_with` (`lib.rs:148`); `Agent endpoint bound` with `ElapsedMs` after `server::bind` (`lib.rs:156`)
  **[login-blockers: bind and agent.json times]**; `Agent running` with total `ElapsedMs` (replaces the `eprintln!` at
  `cli.rs:233`); `Agent stopping` with the reason (signal, a client's Shutdown, a failure) and `Agent stopped` with
  `ElapsedMs`; error `Agent could not start` with the chain in `run_exit_code` (`cli.rs:210-222`).
- The 24 non-CLI `eprintln!("dyno-agent: ...")` become `log::warn!`/`log::error!` with the same text: `lib.rs:139`,
  `issues.rs:191`, `server.rs:61`, `:66`, `netwatch.rs:53`, `:72`, `sampler.rs:54`, `:180`, `alarms.rs:46`, `:58`,
  `agent.rs:193`, `:208`, `unusual.rs:56`, `:105`, `:209`, `hardware.rs:360`, `:397`, `bench.rs:157`,
  `extras/mod.rs:210`, `cli.rs:70`, `:74` (those two keep stderr as well: they run while ending the process).
- `src/server.rs`: info `Client connected` with the client's app version and protocol after the hello, info `Client
  disconnected` with `ConnectedMs`, info `Shutdown requested by a client`; a refused connection (`:66`) at warn.
- `src/instance.rs:73` `note`: also `log::warn!` the same line, so a refusal is in the dated log; `agent.log` stays as
  the fallback for a failed log install.

**D2. The app's own lines go to the same file as the shared crates' lines** (`app/src-tauri/src/logging.rs`).
- `info()` (`:53-60`) calls `log::info!(target: "dyno", "{message}")` instead of writing `app.log`: microsecond UTC
  stamps instead of whole seconds, the session id and pid on every line, one file. `init()` and the writer go;
  `app.log` is no longer written (rule 55: one implementation; no register entry needed once deleted).

**D3. The link's phase changes** (`app/src-tauri/src/supervise/mod.rs`, `link/mod.rs`).
- `run_owner` takes an `on_step: impl FnMut(&M::Snapshot)` called after `publish` (`supervise/mod.rs:193`); Dyno passes
  a closure that logs info `Agent link phase` with `From`, `To` (the `Phase` variant name), `InPhaseMs` and
  `SinceBeginMs`, **only when the variant changes** (ticks every 100 ms never log). A shown stall logs warn with its
  kind (the existing `still waiting` line, `driver.rs:254`, gains `SinceBeginMs`); a cleared stall logs info.

**D4. Looks, launches and the app's own child** (`app/src-tauri/src/link/driver.rs`, `link/ports.rs`).
- Each look's answer at debug; at info when it differs from the previous answer: the discovery file (none, this build,
  another build, its pid), the lock (held, free, not probed), the service state and how long its read took, the child
  (none, running, exited with code), `LookMs` **[login-blockers: service, discovery file, lock, look durations]**.
- `Launched` (`driver.rs:82`): info `Agent launched` with pid and `LaunchMs`, or error `Agent launch failed` with the
  chain (today only "starting the agent" before the launch, `:150`).
- `ports.rs:76-80`: when the agent this app started is found exited, warn once `The agent this app started exited` with
  its exit code (3 means another agent holds the folder).

**D5. Connect attempts and closes** (`app/src-tauri/src/link/driver.rs`).
- `ConnectFailed` for "There is no discovery file." (`:172`) and "The discovery file changed." (`:181`): info with an
  attempt count and `SinceBeginMs`, written when the reason changes and then as one summary line on success ("connected
  after n attempts, m ms"); `:209` gains the attempt count and `ConnectMs`; the success line
  **[login-blockers: successful connects]** carries attempts and `SinceBeginMs`.
- `PeerClosed` (`:205`): info `Agent connection closed` with `ConnectedMs`.

**D6. Report a problem with logs, working with the agent down** (`app/src-tauri/src/lib.rs` `shell()`, about line 85).
- `builder.diagnostics(..)` with paddock-diag's built-in sections (system facts, settings, the crash markers, the log
  window over `dyno*.log`, which now includes `dyno-agent.<day>.log`) and a Dyno section that reads `agent.log` from
  the data folder. The shell then also registers the "Diagnostics > Logging" settings section (level to Debug for a
  support case). The agent's own "Save bundle" stays for its data (inventory, recorder) and its README points to
  Report a problem for logs. Check the Help menu entry in the UI e2e; this is the largest item (about half a day).

**D7. Errors shown in the UI** (`app/src-tauri/src/link/mod.rs`).
- `AgentLink::call` (`:347-361`): on `Err`, warn `Agent request failed` with the request's variant name only (never its
  fields: a request can carry a path), the error code, the message shown and, for `IpcError`, the chain. One place
  covers every command that goes to the agent.
- Start banner (`lib.rs:218`): add OS version and architecture, and drop the data-folder path from the message (the app's
  folder is known; the bundle redacts the home anyway). The login start decision (`lib.rs:269`) gains `UptimeMs`
  **[login-blockers: the login Apple Event]**.

**Tests (same change, ER-11 style, synthetic only).** A `Logging::scoped` test per line group with a temporary folder and
`FakeClock`: the agent's start writes the five start lines in order; a held lock writes the refusal with `ExitCode = 3`;
the link's phase closure writes one line per variant change and none per tick (a run of 50 ticks writes no line); a
failed `call` writes the variant name and no field values; D6's preview lists `logs/dyno-agent.<day>.log` with the
agent not running.

**What the bundle then shows for a 30 s login connect:** the app's start and login decision with `UptimeMs`; each look
with the service state and its read time, the discovery file and the lock; every phase change with its duration; each
connect attempt and why it failed; from the agent, each start with its parent (launchd), its lock result and exit code,
and its store-open and bind times. Hypotheses 1 to 5 above each leave a different trace.
