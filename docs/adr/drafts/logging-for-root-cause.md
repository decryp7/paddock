# 36. Logging for root cause: one log per process, typed start, phase, process, connect, job and error events, the bundle reads them all

- Status: Proposed; decisions taken 2026-10-31 (section "Decisions (user, 2026-10-31)"). Independent review round 1:
  changes requested and made in this revision.
- Date: 2026-10-31
- Related: rule 56 (logs explain every failure; on `docs/rule-logs-explain-failures`, `ef8f52349`), rule 48 (recorded
  measurements), rule 51 (slow means more parallelism or cheaper), rule 55 / ADR 0034 (one implementation per
  mechanism, C58), ER-6, ER-14 (no real user data), ER-17 to ER-20, ER-21 / rule 40, ER-22 / rule 42; PRINCIPLES "Logs
  must not contain user content by default"; ADR 0008 (long operations), ADR 0012 (app shell), ADR 0020 (shared
  executor), ADR 0024 (background work), ADR 0029 (the shell owns the start), ADR 0031 (patience: stall verdicts),
  ADR 0033 (processes by identity); plans X1 (paddock-log), X2 (paddock-diag), X3 (crash marker).
- Evidence: a read-only audit of `origin/main` at `cd4aa0afa` (2026-10-31). Paths are relative to the repo root. Counts
  are `grep` counts of non-test Rust (`info!|warn!|error!|debug!|trace!` and `eprintln!|println!`), raw lines. No user
  logs were read; the audit used code and test fixtures only.

## Context

**The goal (user, 2026-10-31).** From a Report-a-problem bundle alone, someone who cannot reproduce the problem finds
the root cause of a start failure, a slow or failed connect, a stuck or failed job, a crash, an update failure, or a
background-agent or service problem. Releases run on machines we cannot attach a debugger to; the bundle is all we get.
Rule 56 makes this binding; this ADR is the mechanism that makes it cheap and checked.

**What made it urgent.** Dyno at login takes about 30 s to connect to its agent. The question was whether the logs say
why. They do not (section "Dyno's 30 s login connect"), and the reasons are not Dyno's alone.

### The shared pieces are good, and mostly unused

- `crates/paddock-log` (plan X1) is the family log: CLEF JSON lines (`@t` UTC with microseconds), one file per process
  per day in `AppDirs::logs()` (`<id>.<day>.log` for the app, `<id>-<process>.<day>.log` for others), a day budget
  (20 MB, then errors only) and 7 kept days (`settings.rs`), level changeable at run time, `tracing` and `log` in one
  stream, enrichment on every line (app, process, version, commit, OS family, session id, pid; `install.rs`
  `Logging::new`), a redaction net by key (secret-like keys masked, content-like keys replaced by a size, values bounded;
  `redact.rs`), dedupe of **errors only** with breadcrumbs (`core.rs:67`), a panic hook on every thread with a crash
  marker for the next start (`panic.rs`), bounded child-stderr capture (`child.rs`) and `ipc_span` for
  `Window`/`OperationId`/`Command`.
- `crates/paddock-diag` (plan X2) is Report a problem: a zip the user previews and saves, built from sections; the
  built-ins are system facts, the settings file, the crash marker and a 2 MB window over the 4 newest log files named
  `<id>*.log` (`window.rs` `LOG_WINDOW`, `MAX_LOG_FILES`); every file passes a three-layer redactor (secret patterns in
  text, `secrets.rs`; personal data: user names, home folders, machine name, e-mail and IP addresses; paddock-log's field
  rules). It works with every background process down, because it reads files, not IPC.
- `crates/paddock-shell` installs paddock-log first in `run` (`logging.rs`) and wires Report a problem when the app calls
  `Builder::diagnostics` (`builder.rs:185`, `:568`).

**Who uses them** (`LogConfig::new` / `logging::install` in non-test code; no app below except Dyno lists `paddock-log`
or `paddock-diag` in any Cargo.toml):

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
   enriched), the architecture, the parent and the launch source. `with_commit` is never called outside tests, so
   `commit` is empty on every line. `system-info.json` has no OS version either (`paddock-diag/src/section.rs:116-124`).
2. **No phases with durations.** paddock-startup logs "Window ready" and the watchdog's verdicts
   (`paddock-startup/src/tauri.rs:311-370`), but no line says how long since the process started.
3. **Processes, connects and jobs log nothing in the shared crates.** `paddock-jobs` 0 log calls, `paddock-ipc` 0,
   `paddock-net` 0, `paddock-download` 0; `paddock-proc` 7 (warnings on refusal only).
4. **Errors shown to the user are not logged.** `paddock-commands` has no log call; where errors are logged, they are
   logged with `{e}` (Display) and lose the `source()` chain.
5. **No correlation.** `ipc_span` is never used in product code; `JobId` (`paddock-jobs/src/model.rs:11`) never reaches a
   log line; nothing joins an app line to the agent or service line it caused.
6. **Updates are mostly silent.** `paddock-updater` logs whether updates are available in this build and that an install
   at exit failed (`tauri.rs:377`, `:417`), without the cause.
7. **Config fallbacks are uneven.**
8. **Redaction at rest is by key only.** A secret, path or host inside a message, an error chain or a child's stderr is
   written to disk as is; secret patterns are only removed when the bundle is built (`paddock-diag/src/secrets.rs`). The
   apps with their own loggers have no net at all; Pitcrew writes runner output raw.

### The pattern (ER-21)

**A failure that only the user's machine shows, whose log either is not written, is written where the bundle does not
look, or says what failed but not why or after how long.** Instances: Dyno's agent writes to `/dev/null`; Dyno's own
lines go to a file no bundle reads; Pitcrew's bundle has no service log when the service is down and never has the app's;
Spotter and Pitwall have no bundle; the Pitcrew app's start failure goes only to stderr; five own loggers (copies under
rule 55); errors returned to the UI are lost in every app on `paddock-commands`. This ADR proposes one mechanism and
checks over every instance.

## Decision

### 1. One log per process, always paddock-log

Every process the family ships (app, agent, server, service, worker, plugin host, our own sidecars, a CLI when it runs
as a service) installs paddock-log as the first thing in `main`/`run`, with its process name, the panic hook and a crash
marker, into `AppDirs::logs()` of its app, so the file is `<id>-<process>.<day>.log` and the bundle finds it by prefix.
A process started by a service manager whose stderr is not ours (launchd without `StandardErrorPath`, Windows services)
depends on this.

- A service running as root/SYSTEM writes to a log folder the user can read (`%ProgramData%\<Name>\logs`,
  `/Library/Logs/<Name>`), and the app's diagnostics section reads that folder directly.
- Own loggers are copies of paddock-log under rule 55, registered in `docs/known-copies.toml` (mechanism
  `process-log`) and deleted before that app's next tag (decision U5; C58 holds the date):

  | Copy | Owner | `delete_by` |
  |---|---|---|
  | `apps/dyno/app/src-tauri/src/logging.rs`, `apps/dyno/crates/dyno-agent/src/instance.rs` `note` | dyno | `dyno-v1.0.0` (D1, D2) |
  | `apps/spotter/app/src-tauri/src/applog.rs` | spotter | `spotter-v<next>` |
  | `apps/pitcrew/app/src-tauri/src/applog.rs`, `apps/pitcrew/crates/pitcrew-service/src/logfile.rs` | pitcrew | `pitcrew-v<next>` |
  | `apps/debrief/app/src-tauri/src/logging.rs` (with its `clef.rs`), the panic hook in `lifecycle.rs:636` | debrief | `debrief-v<next>` |
  | `apps/pitwall/app/src-tauri/src/service/windows.rs:489-500` (the Windows service's own writer) | pitwall | `pitwall-v<next>` |

  `<next>` is the app's next release version at registration (C58's format, for example `delete_by = "dyno-v1.1.0"`).
- Child output that is not ours (runner output, a browser's stderr) goes through `spawn_child_capture`, bounded and
  scanned (section 6), into the parent's log under `Child = <name>`, at debug.

### 2. Typed events, one helper each, in paddock-log (`paddock_log::event`)

Thin helpers with fixed property names (the table in section 2.1), no new dependency, no Tauri.

```rust
/// Written once by `Logging::install` (and by `LogHandle::banner` for processes that learn their start reason later).
pub struct StartFacts { pub launch: Launch, pub parent_pid: Option<u32>, pub previous_run: Option<&'static str> }
pub enum Launch { User, LoginItem, RestoredAtLogin, AfterUpdate, ServiceManager, SpawnedBy(&'static str), Unknown }

/// A phase with a duration: "Phase started" at debug, "Phase finished" at info with ElapsedMs on `finish`, "Phase failed"
/// at warn with the error chain; dropped without either, "Phase ended without a result" at warn with ElapsedMs. Never a
/// timeout: it measures, it does not judge.
pub fn phase(name: &'static str) -> PhaseGuard;

/// A state machine's published phase changed: logs only on a change of the discriminant, with From, To, InPhaseMs and a
/// static cause. A flapping guard: more than `FLAP_CHANGES` (8) changes in `FLAP_WINDOW` (10 s) are written as one
/// "Phase flapping" line with the count and the states seen, until a state holds for a window.
pub struct Transitions<K: Copy + Eq + Debug> { .. }
impl<K: Copy + Eq + Debug> Transitions<K> { pub fn observe(&mut self, now: K, cause: &'static str); }

/// Errors: the chain (`source()`, at most 8 links, each bounded and scanned), the text shown and the next step offered.
pub fn error_shown(err: &(dyn std::error::Error + 'static), shown: &str, next_step: Option<&str>);
pub fn chain(err: &(dyn std::error::Error + 'static)) -> String; // "a: b: c", scanned
```

The cause of a transition is a `&'static str` naming the event kind (`"looked"`, `"connect failed"`), never `Debug`
output of an event (which can hold a path or a host).

Every line also carries **`UptimeMs`**, milliseconds since the log was installed. `commit` is filled from
`PADDOCK_BUILD_COMMIT` by `paddock-shell::logging::install` and by the same helper for non-shell processes.

`StartFacts` adds to the first line: OS name and version, kernel version, architecture, the parent pid and name, the
launch source, the log level, portable or debug build, and the previous run's verdict (clean or crashed, from
paddock-startup's marker). The OS version comes from a new `paddock_env::os::version()`, read in-process and bounded:
macOS `sysctlbyname("kern.osproductversion")`, Windows `RtlGetVersion`, Linux `/etc/os-release` `VERSION_ID`;
`unknown` on failure, never a spawned tool. Dyno's `dyno-platform` facts reader becomes a consumer.

**Dedupe (WP-1).** paddock-log dedupes only errors today (`core.rs:67`). WP-1 extends it to warn and to the typed connect
and stall events (keyed by source, event name and the static reason), with the existing "repeated n times" summary. Until
WP-1 lands, the Dyno lines of D5 and D7 log only when the reason changes, plus one summary line on success.

#### 2.1 Property names (one table; D3 to D7 use it now)

| Property | Meaning |
|---|---|
| `UptimeMs` | ms since the process's log was installed (every line) |
| `ElapsedMs` | duration of the phase or step the line ends |
| `SinceBeginMs` | ms since the supervisor or job began |
| `InPhaseMs` | ms spent in the phase a transition leaves |
| `From`, `To`, `Cause` | a transition's phases and its static cause |
| `Attempt`, `Attempts` | the number of this try; the total on success |
| `Reason` | a static reason (`no-discovery-file`, `refused`, `other-build`, `lock-held`) |
| `Pid`, `ParentPid`, `ParentName`, `ExitCode`, `Signal` | process facts |
| `Program` | a program's file name (never its folder or arguments at info) |
| `JobId`, `Kind`, `OperationId`, `Request` | correlation and the request's variant name |
| `ErrorCode`, `ErrorChain`, `Shown`, `NextStep` | an error, its chain, what the user saw and the step offered |
| `Launch`, `OsVersion`, `Arch` | start facts |

### 3. The shared crates log their own events, so apps get them for free

| Crate | Events | Level |
|---|---|---|
| `paddock-startup` / `paddock-shell` | the start banner; phases `start.plugins`, `start.window`, `start.ready` with `ElapsedMs`; the watchdog's verdicts (existing) | info |
| `paddock-proc` | `Process spawned` (`Program`, pid, start identity; arguments at debug, scanned, user paths through `path_hint`), `Process ready` (ms since spawn), `Process exited` (code or signal, ms alive; the stderr tail at debug, scanned), `Process stalled` with the ADR 0031 verdict and its signals, restarts and quarantine with the count | info; stalls warn |
| `paddock-ipc` / `paddock-service` client | `Connect attempt` (attempt, endpoint kind, fingerprint), `Connected` (ms, attempts), `Connect failed` (`Reason`: not found, refused, denied, version mismatch, cap reached; the chain), `Disconnected` (who closed, ms connected) | info; deduped (WP-1) |
| `paddock-jobs` | `Job queued` / `started` / `phase` / `cancel requested` / `finished` (state, ms, counts) with `JobId` and `Kind`; the work runs in a span with `JobId`; the subject only as a fingerprint | info; failures warn with chain |
| `paddock-commands` | every `Err` returned to the webview: `error_shown` with the command, window and code, inside `ipc_span` | warn |
| `paddock-updater` | check (channel, answer, latest version, HTTP status class), download (bytes, ms, digest ok), signature result, install steps, relaunch; failures with the chain | info; failures error |
| `paddock-settings`, `paddock-env` state files | loaded, missing (defaults), damaged (kept as `.bad`, defaults), newer (read-only) | info / warn |
| `paddock-service` | registration, status reads (answer, ms), repair steps (existing), host start and stop (existing) | info |

**Correlation across processes.** paddock-ipc frames gain an **optional** `op` field (the caller's `OperationId`),
absent when unset; old peers ignore it and new peers accept frames without it (round-trip tests both ways, old-to-new
and new-to-old). The receiving side opens a span with it. A job started for a request records the request's
`OperationId`.

### 4. Panics and crashes

The paddock-log panic hook and crash marker are on in every process (section 1). A native crash does not run the hook;
the next start finds the running marker without a clean exit and logs "previous run did not end cleanly"
(paddock-startup, `lifecycle.rs:384`). The diagnostics section lists the **names and dates** of the OS crash reports for
the app's processes (macOS `~/Library/Logs/DiagnosticReports/<process>-*.ips`, Windows WER `ReportArchive` folder names),
never their contents (decision U4).

### 5. The bundle reads every process, even with the background process down

- Every app calls `Builder::diagnostics`. App-specific bundles (Dyno's agent-written zip, Pitcrew's IPC-built zip)
  become **sections**: they ask the background process when it answers and otherwise read its files from disk and say so.
- **WP-6a:** the Logs window is 4 MB (decision U1), split in **per-process shares**: equal shares across the `<id>*`
  prefixes found, an unused share passed to the others, newest first, so a chatty process cannot push the agent's or
  service's log out. `MAX_LOG_FILES` follows from it.
- `system-info.json` adds OS version, kernel version, machine uptime and the app's install location kind.

### 6. What may be written (redaction at rest)

Logs stay on the user's machine; the bundle is the only thing that leaves it, after the user's preview, and is redacted
again. This section is the rule for what reaches the disk.

- **Never at any level:** file contents, record contents, prompts, queries, clipboard, secrets.
- **One secret scanner, applied before the write.** `redact_secrets` moves from `paddock-diag/src/secrets.rs` into
  paddock-log (one implementation, rule 55; paddock-diag calls it). Every text written to disk passes it: the message,
  string properties, `ErrorChain`, stderr tails, child output and command lines. WP-10 measures its cost per line.
- **Spawns:** `Program` (file name) at info; arguments at debug only, each scanned, user paths through `path_hint`.
- **Stderr tails and child output:** debug only, scanned.
- **Paths:** an app-owned path (its data, log, config folders, its binaries) is written as is; the bundle's personal
  layer replaces the home folder and user name. A path the user chose or a file the user owns is written as
  `paddock_log::path_hint` (extension, size, fingerprint; the file name only at debug).
- **Hosts and names:** loopback and the family's own update and service hosts as is; other hosts, repository names,
  machine names and account names as a fingerprint at info, plain at debug (decision U3).
- **Fingerprints are keyed.** `fingerprint` becomes a keyed hash with a per-install salt kept in the app's data folder,
  never put in the bundle, so a fingerprint cannot be reversed by hashing guesses; two lines of one install still match.
- **Debug is visible.** When the level is Debug or Trace, Report a problem's preview says so and that the log may hold
  file names and hosts.
- The default redactor gains `path`-like and `host`-like key rules and a test per rule; a test logs synthetic personal
  data and secrets through every typed helper and asserts what lands in the file.

### 7. The checks (C63–C65; renumber at acceptance if taken)

- **C63, every process logs.** Each shipped binary target (a Tauri `run`, a `main` of an agent, server, service, worker or
  plugin host; the list from each app's `paddock.ci.json`) calls `paddock_shell::logging::install` or
  `paddock_log::Logging::install` before its first spawn, connect or thread. Non-CLI product code has no
  `eprintln!`/`println!` (CLI output functions carry `// cli-output`). An own logger fails unless registered (C58).
- **C64, logged spawn, connect and job.** Not a new scanner: the apps' clippy `disallowed-methods` (Dyno's exists,
  `apps/dyno/clippy.toml:3`) gain `std::process::Command::new`, `UnixStream::connect`, the named-pipe opens and
  `JobRegistry::register` outside the shared helpers, with a reason pointing at the logged helper. Report-only first
  (a warning listed in the conformance report), an error once every app is on the helpers.
- **C65, every error shown is logged.** A `#[tauri::command]` returns paddock-commands' error type (which logs through
  `error_shown`), never `Result<_, String>`.
- **A runtime check in the start smoke** (paddock-e2e / each app's VM smoke): after a start, each process's newest log has
  the banner, `start.ready` with `ElapsedMs`, and for apps with a background process a `Connected` line; a fixture
  endpoint that refuses produces `Connect failed` with its `Reason`; a ceiling on start lines. Only the test's temporary
  log folder is read (ER-14).

## Parallelism and cost

- **Volume.** Steady state at info: under one line per second per process (no per-tick, per-sample, per-item or
  per-frame lines at info). A start: at most about 200 lines. A job: a fixed number of lines, never one per item. Look
  answers at debug only on a change or sampled (one in 50 unchanged looks), never on every 100 ms look. Repeats are
  deduped (WP-1), flapping transitions summarised.
- **Worst-case disk use.** Per process: (7 kept days + today) x 20 MB = 160 MB, plus the error lines written after the
  day's budget is spent (bounded by the error dedupe). A two-process app (Dyno): 320 MB at worst; a typical day is far
  below the budget. The budget and days are Settings (decision U1).
- **Hot paths stay cheap.** A disabled level costs an atomic load and formats nothing; `Transitions::observe` compares a
  `Copy` discriminant and allocates only on a change; `phase()` and `UptimeMs` read one `Instant`.
- **The secret scan has a cost.** It runs on every written line (section 6). WP-10 measures it with the rest.
- **The writer.** paddock-log writes synchronously under one mutex per process. **WP-10 comes before WP-3 and WP-4**: it
  measures the cost per line and per burst, with and without the scan (synthetic events, release build), records it in
  `docs/benchmarks.md` (rule 48), and moves the write to a bounded channel and one writer thread only if a line costs
  more than about 20 µs on the slowest CI machine or a burst blocks a caller. Shutdown flushes with a bounded wait.
- **Rotation.** Daily files per process; old days are removed at install and at the first write of a new day.
- **The bundle** reads a fixed window from the end of each file (a seek); collection runs as a job off the UI thread.

## Dyno's 30 s login connect: could today's logs explain it?

**No.** What exists (at `cd4aa0afa`):

- The app writes `"Dyno <v> starting (data folder: ...)"` (`apps/dyno/app/src-tauri/src/lib.rs:218`), `"started hidden
  (flag or login item)"` (`lib.rs:269`), `"starting the agent"` only when the app starts its own (`link/driver.rs:150`),
  `"still waiting: <message>"` on a stall (`driver.rs:254`), `"agent.json names an agent that did not answer"`
  (`driver.rs:209`) and `"connected to the agent"` (`driver.rs:245`). These go to Dyno's own
  `~/Library/Application Support/Dyno/logs/app.log` with **whole-second** stamps (`logging.rs:62-68`), and **no bundle
  contains it**.
- The supervisor's phase changes, each look's answer, "no discovery file" connect failures (`driver.rs:172-184`) and the
  waits between looks are not logged.
- The agent's stderr is discarded (no `StandardErrorPath` in
  `apps/dyno/app/src-tauri/macos/net.decryptology.dyno.agent.plist`; `/dev/null` when the app starts it). Its one
  exception, a lock refusal (`dyno-agent/src/lib.rs:132-135`), goes to `logs/agent.log` through `instance::note`
  (`instance.rs:73-91`), with **no timestamp** and **a line equal to the last one skipped**, so ten refusals 10 s apart
  leave one undated line.

Five explanations fit the code; none is verified, and today's logs cannot tell them apart:

1. **launchd throttling:** `KeepAlive` and `ThrottleInterval 10`; an agent that exits early (exit 3, another agent holds
   the data folder's lock, `dyno-agent/src/cli.rs:89`; or a failure) is started again after 10 s; three rounds are 30 s.
2. **The first look waited on `SMAppService.status`** (addressed on `dyno/login-blockers`, `2f3931a48`).
3. **A slow agent start** before the discovery file exists (`Running::start_os`: store open, bind).
4. **A paused agent or app** at login (addressed on `dyno/login-blockers`, `586a5190d`, ADR 0031).
5. **Connect retries** with the look pace between them.

The Dyno fix list (appendix) makes each leave a different trace.

## Gaps per app (audit 2026-10-31)

P1: without it the root cause cannot be found from the bundle. P2: it makes that slower or less certain. Spotter,
Pitcrew, Debrief and Pitwall counts come from two read-only sub-audits whose key claims were spot-checked; their line
references are secondary and must be re-read by whoever fixes them.

| Scope | P1 | P2 | The P1s in short |
|---|---|---|---|
| Shared crates | 4 | 4 | no start banner; jobs, ipc, proc events unlogged; errors returned to the UI unlogged; updater check, download and cause unlogged |
| Dyno | 5 | 6 | agent unlogged, no panic hook, stderr discarded; app's own lines in a second file with 1 s stamps; no bundle contains any log, and the bundle needs a connected agent; supervisor phases, looks and "no discovery file" unlogged; agent start steps and exits unlogged |
| Spotter | 5 | 4 | no panic hook; no bundle; update silent; command errors unlogged; renderer spawn, ready and watchdog kill unlogged |
| Pitcrew | 7 | 6 | no panic hook (service lost on Windows); bundle omits `app.log` and has no service log when the service is down; app start failure only on stderr; connect and backoff unlogged; update silent; command errors unlogged; the service's fatal IPC error not in its log |
| Debrief | 5 | 7 | workers have no logger or panic hook; the logger starts after the window and plugins; UI errors unlogged (33 `map_err(to_string)` sites); job failure reason unlogged; worker connect without attempts, durations or stall verdict |
| Pitwall | 3 | 3 | no log or panic hook in any process; no bundle; connect, update and command errors unlogged |
| Pacenotes, Sighter | 0 | 0 | no code yet; C63 holds them to paddock-log and paddock-diag |

## Work packages

| WP | What | Size | Owner |
|---|---|---|---|
| D1-D7 | Dyno's fix list (appendix) | S each, D6 about half a day | Dyno lead |
| WP-1 | `paddock_log::event`: `StartFacts`, `phase` (with "ended without a result"), `Transitions` (static cause, flapping guard), `error_shown`, `chain`, `UptimeMs`; dedupe extended to warn and the typed connect and stall events | M | paddock-log |
| WP-2 | `paddock_env::os::version()` (in-process) and the banner in `paddock-shell::logging::install`; `commit` from the build | S | shared |
| WP-3 | paddock-proc and paddock-ipc events; the optional `op` frame field with round-trip tests both ways | M | shared |
| WP-4 | paddock-jobs events and the `JobId` span; paddock-commands `error_shown` and `ipc_span` in `route` | M | shared |
| WP-5 | paddock-updater and paddock-settings events | S | shared |
| WP-6a | paddock-diag: 4 MB log window with per-process shares (in the Dyno 1.0 set) | S | paddock-diag |
| WP-6 | paddock-diag: OS facts, crash-report names, the Debug warning in the preview, sections that read files when IPC is down | M | paddock-diag |
| WP-7 | The secret scanner moved into paddock-log and applied before the write; path and host key rules; `path_hint`; keyed fingerprints with a per-install salt; the synthetic-data test | M | paddock-log |
| WP-8 | C63, C64 (clippy `disallowed-methods`), C65, report-only first; the start-smoke log assertions | M | conformance |
| WP-9 | Per app, before its tag: onto paddock-log and paddock-diag, own loggers deleted (section 1's table) | M-L per app | app leads |
| WP-10 | Measure the cost per line and per burst, with and without the scan; record in `docs/benchmarks.md`; writer thread only if over the bound | S | paddock-log |
| WP-11 | Merge `docs/rule-logs-explain-failures` (rule 56) first | S | steward |

## Order

1. **Dyno 1.0:** D1 to D7 and WP-6a.
2. WP-10 (the measurement every later package relies on).
3. WP-1, WP-2, WP-7.
4. WP-3, WP-4, WP-5.
5. WP-6.
6. WP-8, report-only.
7. WP-9 per app, in release order.

WP-11 is merged before any of them lands, so each change can cite rule 56.

**The minimum before any app's next tag:** every process installs paddock-log with the panic hook and a crash marker;
`Builder::diagnostics` with per-process log shares (WP-6a); the app's own logger deleted (C58 holds it to its
`delete_by`); no `eprintln!`/`println!` outside CLI output.

## Risks

- **Log noise hides the cause.** Mitigated by the volume rules, info vs debug, dedupe, the flapping guard and the start
  smoke's ceiling.
- **User content or a secret slips into a message string.** Mitigated by typed helpers (values as properties), the
  scanner before the write, `path_hint`, static causes, the synthetic-data test, and the bundle's personal layer.
- **The scan costs too much on a hot path.** WP-10 measures it before WP-3 and WP-4 add volume.
- **Churn in shared crates during releases (ER-1).** WP-1 to WP-5 add lines and helpers without changing behaviour.
- **Root/SYSTEM services' log folders** need care with permissions (no user-writable path a privileged process trusts).

## Decisions (user, 2026-10-31)

- **U1. Retention and bundle size:** 20 MB per day and process, 7 days kept; a 4 MB bundle log window split across
  processes (WP-6a).
- **U2. Default level:** Info by default, Debug from Settings for a support case.
- **U3. Hosts and names at info:** fingerprinted (keyed, section 6).
- **U4. OS crash reports:** by name and date only.
- **U5. Own loggers:** each app's own logger is replaced before that app's next tag.
- **Dyno D6:** in Dyno 1.0.

## Appendix: Dyno fix list before the 1.0 tag (a patch plan, not implemented)

Small and safe: no behaviour changes, only log lines and two wirings. Paths are relative to `apps/dyno/`. Property names
follow section 2.1. Lines marked **[login-blockers]** are being added on `dyno/login-blockers` (service look and
`SMAppService.status` timing, the discovery file, the lock probe, look durations, successful connects, the agent's start,
bind and `agent.json` times, the login Apple Event, connected to first data); they are listed so the review can check
them, not to be done twice, and they need D1 and D2 to land in a file the bundle reads. Until WP-1's dedupe exists, every
repeating line below is written **on change only**.

**D1. The agent logs through paddock-log** (`crates/dyno-agent`; adds `paddock-log`, `paddock-env` and `log` with `kv`).
- `src/cli.rs` `run()` (about line 227), first statement: install
  `Logging::new(LogConfig::new(&app_id, "agent", VERSION).with_log_dir(&logs).with_crash_marker(&CrashMarker::path_for(&data, "agent")))`,
  where `app_id` is built from Dyno's `APP_ID`/`APP_NAME` constants (shared with the app, not a new literal), `logs` and
  `data` are `AppDirs::for_app(&app_id)`'s `logs()` and `data()` (the folders paddock-diag's Logs and Crash sections
  read; the file is `dyno-agent.<day>.log`), or `dirs.logs()` and `dirs.root()` when `DYNO_DATA_DIR` is set (tests stay
  in their folder, ER-14). Only `run`; `status`, `once`, `bench` and `service` keep printing. The panic hook is on.
- Lines (info unless said): `Agent starting` with `ParentPid`, `ParentName` (launchd when the parent is pid 1),
  `OsVersion`, `Arch`; `Agent lock taken` with `ElapsedMs`, or warn `Agent refused` with `Reason = lock-held`, the
  holder's `Pid` and `ExitCode = 3` (`src/lib.rs:132-135`); `Agent store open` with `ElapsedMs` after
  `Shared::build_with` (`lib.rs:148`); `Agent endpoint bound` with `ElapsedMs` after `server::bind` (`lib.rs:156`)
  **[login-blockers]**; `Agent running` with total `ElapsedMs` (replaces the `eprintln!` at `cli.rs:233`); `Agent
  stopping` with a static `Reason` (signal, client-shutdown, failure) and `Agent stopped` with `ElapsedMs`; error `Agent
  could not start` with `ErrorChain` in `run_exit_code` (`cli.rs:210-222`).
- The 24 non-CLI `eprintln!("dyno-agent: ...")` become `log::warn!`/`log::error!` with the same text: `lib.rs:139`,
  `issues.rs:191`, `server.rs:61`, `:66`, `netwatch.rs:53`, `:72`, `sampler.rs:54`, `:180`, `alarms.rs:46`, `:58`,
  `agent.rs:193`, `:208`, `unusual.rs:56`, `:105`, `:209`, `hardware.rs:360`, `:397`, `bench.rs:157`,
  `extras/mod.rs:210`, `cli.rs:70`, `:74` (those two keep stderr as well: they run while ending the process).
- `src/server.rs`: info `Client connected` with the client's app version and protocol, info `Client disconnected` with
  `ElapsedMs`, info `Shutdown requested by a client`; a refused connection (`:66`) at warn, on change.
- `src/instance.rs:73` `note`: writes through `log::warn!`; `agent.log` stays only as the fallback when the log could not
  be installed, and goes with the rest of the copy (`delete_by = "dyno-v1.0.0"`, or registered for `dyno-v1.1.0` if the
  fallback is kept).

**D2. The app's own lines go to the same file as the shared crates' lines** (`app/src-tauri/src/logging.rs`).
- `info()` (`:53-60`) calls `log::info!(target: "dyno", "{message}")` instead of writing `app.log`; `init()` and the
  writer go. Microsecond UTC stamps, the session id and pid on every line, one file.

**D3. The link's phase changes** (`app/src-tauri/src/supervise/mod.rs`, `link/mod.rs`).
- `run_owner` takes an `on_step: impl FnMut(&M::Snapshot)` called after `publish` (`supervise/mod.rs:193`). Dyno passes
  a closure that logs info `Agent link phase` with `From`, `To` (the `Phase` variant name), `InPhaseMs` and
  `SinceBeginMs`, **only when the variant changes**, with the flapping guard of section 2 inlined until WP-1 (more than 8
  changes in 10 s become one summary line). A shown stall logs warn (the existing `still waiting` line, `driver.rs:254`,
  gains `SinceBeginMs`); a cleared stall logs info.

**D4. Looks, launches and the app's own child** (`app/src-tauri/src/link/driver.rs`, `link/ports.rs`).
- A look's answer at info when it differs from the previous answer, at debug otherwise only one in 50: the discovery file
  (none, this build, other build, its `Pid`), the lock (held, free, not probed), the service state and its read's
  `ElapsedMs`, the child (none, running, exited with `ExitCode`) **[login-blockers: service, discovery file, lock, look
  durations]**.
- `Launched` (`driver.rs:82`): info `Agent launched` with `Pid` and `ElapsedMs`, or error `Agent launch failed` with
  `ErrorChain`.
- `ports.rs:76-80`: when the agent this app started is found exited, warn once `The agent this app started exited` with
  `ExitCode` (3 means another agent holds the folder).

**D5. Connect attempts and closes** (`app/src-tauri/src/link/driver.rs`).
- `ConnectFailed` (`:172`, `:181`, `:209`): info `Agent connect failed` with `Reason` (`no-discovery-file`,
  `discovery-file-changed`, `refused`), `Attempt` and `SinceBeginMs`, written when the reason changes; the success line
  **[login-blockers]** carries `Attempts` and `SinceBeginMs`.
- `PeerClosed` (`:205`): info `Agent connection closed` with `ElapsedMs`.

**D6. Report a problem with logs, working with the agent down** (`app/src-tauri/src/lib.rs` `shell()`, about line 85;
needs WP-6a in the same set, or the agent's share can be pushed out of the 4-file window).
- `builder.diagnostics(..)` with paddock-diag's built-in sections (system facts, settings, the crash markers of `app` and
  `agent`, the log window over `dyno*.log` with per-process shares) and a Dyno section that reads `agent.log` from the
  data folder while it exists. The shell then also registers "Diagnostics > Logging". The agent's "Save bundle" stays for
  its data (inventory, recorder) and its README points to Report a problem for logs. Check the Help menu entry in the UI
  e2e.

**D7. Errors shown in the UI and the start line** (`app/src-tauri/src/link/mod.rs`, `lib.rs`).
- `AgentLink::call` (`:347-361`): on `Err`, warn `Agent request failed` with `Request` (the variant name only, never its
  fields), `ErrorCode`, `Shown` and, for `IpcError`, `ErrorChain`; on change of (`Request`, `ErrorCode`) until WP-1.
- Start line (`lib.rs:218`): add `OsVersion` and `Arch`, drop the data-folder path from the message. The login start
  decision (`lib.rs:269`) gains `Launch` and `UptimeMs` **[login-blockers: the login Apple Event]**.

**Tests (same change, synthetic only).** A `Logging::scoped` test per line group with a temporary folder and `FakeClock`:
the agent's start writes its start lines in order and leaves a crash marker path under the test folder; a held lock
writes the refusal with `ExitCode = 3`; the phase closure writes one line per variant change and none per tick (50 ticks,
no line), and a flapping run writes one summary; a failed `call` writes the variant name and no field values; repeated
connect failures with one reason write one line; D6's preview lists `logs/dyno-agent.<day>.log` with the agent not
running and a large app log beside it.

**What the bundle then shows for a 30 s login connect:** the app's start and login decision with `UptimeMs`; each changed
look with the service state and its read time, the discovery file and the lock; every phase change with its duration;
each connect failure reason and the attempts; from the agent, each start with its parent (launchd), its lock result and
exit code, and its store-open and bind times. Explanations 1 to 5 each leave a different trace.
