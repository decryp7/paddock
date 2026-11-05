# 55. Family logging: `EventId` and `run.*` on what Dyno shipped, one log reader, adoption per app

- Status: Proposed (2026-11-05). The user asked for this ADR (decision F3 of the readiness plan, 2026-11-05). Not yet independently
  reviewed.
- Date: 2026-11-05
- Amends: ADR 0036 (logging for root cause; Accepted 2026-10-31, on `origin/docs/adr-logging-root-cause` `021dc3c8d`, **not on main or
  on `dyno/wp-int`**): its decision U1 (retention), §2.1 (property names) and §3 (the `op` frame field). The rest of 0036 stands, and this
  ADR does not repeat it.
- Related: rule 56 and ER-27 (logs explain every failure; rule 56 is only on `origin/docs/rule-logs-explain-failures` `ef8f52349`), rule
  71 and ER-33 (a check needs a probe that can see), rule 42 and ER-22 (share first), rule 48 (recorded measurements), rule 51 and ER-20
  (slow means two questions), rule 55 and ADR 0034 (one implementation per mechanism), ER-14 and the confidential-logs rule (no real
  user logs anywhere), ER-21 and rule 40 (recurring bugs), ADR 0022 (`./dev`), ADR 0049 (wire versions), ADR 0052 (`paddock-jobs` WP-B1),
  ADR 0054 (the lifecycle suite; Proposed), the logging map `docs/research/2026-11-05-logging-map.md` (`origin/docs/logging-map`
  `517d7a9cf`), the plan `docs/plans/apps-readiness.md` (`origin/docs/apps-readiness-plan`).
- Evidence: code read at `origin/dyno/wp-int` `76c662c36` and the app branches named in "Adoption", 2026-11-05. Line numbers are at those
  heads. Docs only: nothing was built or run, because a Dyno release has the machine. **So every cost below is unmeasured, and every size
  and threshold is my judgement.** No user logs were read.

## Context

**The pattern (ER-21; logging map §1).** Eight incidents have one shape: the end of a run is not recorded, what the OS answered is dropped, or
a cause string is computed and not written. ADR 0036 made every process log. The map listed what such a log should say. The user chose on
2026-11-05 to have Dyno take ample logging in 1.1.0, so much of the map has since shipped on `dyno/wp-int`.

**What shipped, which this ADR starts from.**

| Piece | Where | State |
|---|---|---|
| 30 days, 100 MB per process, today's file never removed, Debug and Trace limited to half a day's budget | `paddock-log/src/settings.rs:80`, `files.rs` `TOTAL_CAP_BYTES` | Done; the user's numbers. Reaches every app with Dyno's landing |
| Previous run's verdict for a process without a session | `paddock_startup::run_marker` (`run_marker.rs:36-68`), used at `apps/dyno/app/src-tauri/src/lib.rs:603` and `dyno-agent/src/cli.rs:265-266` | Done. A clean verdict is a **debug** line, so at the default level "clean" and "not wired" look the same |
| Previous run's verdict with a session | `paddock-startup/src/lifecycle.rs:382-388` | Done; a clean verdict writes nothing |
| Start line | `paddock-shell/src/logging.rs:74-78` (`Starting <app>`: `Version`, `Commit`, `Os`, `Arch`, `Pid`, `Portable`, `BootSecs`, `Flags`); agent `cli.rs:250-257` (`ParentPid`, `ParentName`, `OsVersion`) | Done, without a start reason. `install` leaves `Commit` empty; only `install_with_commit` fills it |
| Exit lines | `lib.rs:616`, `:622`, `paddock-window/src/tauri/lifecycle.rs:21`, agent `cli.rs:279`, `:283` | Done, in Dyno's own `.run` callback, with no reason on the app's side and no counts |
| Failed start reaches the log | `lib.rs` `Err` arm of `builder.build`, before `eprintln!` | Done in Dyno; each app would copy it |
| External tools | `tool.run` in `paddock-proc/src/run.rs:317-331`: name, argument count, exit, ms; stderr tail on the first failure of a kind, at warn | Done for every `run_bounded` caller |
| Update and pause | `update.check`, `update.download` (`paddock-updater/src/updater.rs:529-535`, `:651-688`); `process.paused` (`paddock-startup/src/watchdog.rs:361`) | Done, with the id as a **prefix of the message** |
| User actions | `apps/dyno/app/src-tauri/src/breadcrumb.rs`, `commands.rs:48-69`: `Operation`, `OperationId` (`r<n>`, local to the app), `Kind`, `Changed`, `Ms`; `paddock_diag` `builtin::Trail` writes `breadcrumbs.json` | Trail shared, Dyno's request list Dyno's own |
| Report a problem | `paddock_diag`: 4 MiB log window shared per process (`window.rs:9`, `:77-81`), 16 MiB in all (`report.rs:30`), hostile-input tests, headless export (`export.rs`) | Done |
| OS crash reports | `apps/dyno/app/src-tauri/src/crash_reports.rs`: program and time only (ADR 0036 U4) | Done, in the app |

**What is still missing, for every app.**

1. **No stable id.** A reader or test must match message text. Dotted ids exist only as prefixes (`process.paused: ...`, `tool.run failed`), and the
   start and exit lines have none.
2. **The lifecycle wiring is one app's.** Dyno writes the marker and exit lines in its own callback. Spotter, Pitwall and Pitcrew call plain
   `install`, and their branches do not have `run_marker.rs` yet.
3. **Nobody reads it mechanically.** A check cannot tell a run that ended cleanly from a log with no ending. The map's rank 1 for keeping
   this true is a reader; `./dev` has none (`tools/paddock-dev/src/cli.rs`).
4. **Info and Warn have no rate limit.** Errors only are deduped (`paddock-log/src/core.rs`, `level == Error || fatal`).
5. **Evidence ages out.** At the map's about 0.7 MB a day, the 4 MiB window covers about 6 days of 30, and after the day's budget only errors
   are written, so a Debug session can lose the run's end.
6. **Jobs and slow calls are silent.** `paddock-jobs` has no log call and no `log` dependency. A slow success on IPC writes nothing:
   `link/mod.rs:462-471` logs failures only.
7. **Operations do not join across processes.** `OperationId` is `r<n>` in the app alone.

**Three vocabularies disagree.** ADR 0036 §2.1 says `ElapsedMs`, `Launch`, `OsVersion`. The map says `Ms`, `StartReason`, `OsBuild`. Shipped code uses
`Ms` about 84 times and `ElapsedMs` about 10 (a grep of property assignments, so approximate), plus `BootSecs` and `OsVersion`.

**A design error in ADR 0036.** §3 adds an optional `op` field to frames, "old peers ignore it". `paddock-ipc` frames are postcard
(`frame.rs:peek_head`), which has no field tags, and ADR 0049 says of postcard: "The only additive change is a variant appended at the end of
an enum", a field added is breaking. So that field would break the wire.

## Decision

### 1. `EventId` is a property, written through the `log` facade

```rust
log::info!(EventId = "job.end", JobId = id, Kind = kind, Outcome = "done", Ms = ms; "Job finished");
```

- A CLEF property named `EventId`, a lower-case dotted name, stable across versions. Messages stay free to change; **tests, checks and the
  reader read `EventId`, never message text.**
- No macro and no new dependency. `paddock-log` depends on `paddock-jobs` (`crates/paddock-log/Cargo.toml`), so `paddock-jobs` cannot call a
  `paddock_log` helper. Each crate uses the plain `log` facade with `kv`, as `paddock-updater` and `paddock-proc` already do. This
  does not need ADR 0036's clock-port move.
- Lines that have a prefix keep their words and gain the property: `tool.run`, `update.check`, `update.download`, `process.paused`.
- **Register only what the reader needs:** the `run.*` family, one writer module (§2). Any other family has one owning crate and a capture test
  in it. No register of all lines (the map's §6.9; YAGNI). The map's checklist §3 stays the guide for names, and a family gets its ids
  when its owner crate next changes for another reason, not in a sweep.

### 2. `run.*`: one typed writer in `paddock-startup`, wired by the shell

A new `paddock_startup::run_log`, beside `run_marker`, is the only code that writes `run.*`. Parse, don't validate: reasons are enums, so a
process cannot write a word the reader does not know. `paddock-startup` is already a dependency of `paddock-shell` and of Dyno's agent.

| `EventId` | Replaces (shipped line) | Level | Adds to what shipped |
|---|---|---|---|
| `run.start` | `Starting <app>`; `Agent starting` | I | `Launch` (ADR 0036's enum, from `Flags`, parent and service manager); `OsVersion` with the build (UBR on Windows; today's value has no build, map §1) |
| `run.previous` | `run_marker` and `Lifecycle` verdict lines | I clean or still running, W unclean | **Always written.** `Outcome`, `PrevPid`, `PrevStartedAt`, `PrevVersion` |
| `run.phase` | Link and agent phase lines stay; the shell adds its own | I | `Phase`, `Ms`, `Outcome` |
| `run.ready` | `Window main ready`; `Agent running` | I | `Ms` since the process began |
| `run.exit_begin` | `Exit requested`; `Agent stopping` | I | `Reason` enum, `Code`, `UptimeMs` |
| `run.exit` | `Exit`; `Agent stopped` | I | `Code`, `UptimeMs`, `Errors`, `Warns`, `LinesDropped`, `Suppressed`. Repeats flushed first. Absence means the run did not end |
| `run.start_failed` | `Dyno could not start` | Fatal | `Phase`, `ErrorChain`, `NextStep` |

- **The shell owns the wiring.** `paddock_shell`'s `on_run_event` (`builder.rs:952`) already sees `Ready` and `Exit`, and calls `end_run` there.
  It takes the marker, the exit lines and `flush_repeats`, so an app gets them by using the shell, and Dyno's callback (`lib.rs:598-625`) loses them. A
  session app keeps `Lifecycle`, which writes the same `run.previous`; `install` takes the commit from the build by default, and the
  commit-less form becomes test-only. Non-shell processes (agent, service, server, worker) call `run_log` directly, as Dyno's agent calls
  `run_marker` today.
- **Marker format.** `run_marker` gains a third token, the version. `parse` already splits on whitespace and ignores extra tokens, so a
  1.1.0 marker reads with no version, and a 1.1.0 process reads a new marker correctly.
- **Dropped from the map, cheaper without:** `SinceLoginMs` (no portable source), `PrevLastLineAt` (the reader gets it from the previous run's last
  line), `run.teardown` as an id (teardown lines stay as they are), `BootSecs` stays rather than `SinceBootMs`.

### 3. Property names: what shipped wins

Amends ADR 0036 §2.1. New lines use `Ms` for the duration of what the line ends. Existing `ElapsedMs` lines are not rewritten, and the reader
accepts both. `OsVersion` over `OsBuild`, `Launch` over `StartReason`, `OperationId` over the map's `Op`. `Why` is a bounded cause, `ErrorChain` a
source chain, `Code` a process exit code, `Exit` a tool's ending in words. The other names of 0036 §2.1 stand.

### 4. Retention: the shipped numbers replace ADR 0036 U1

30 days and 100 MB per process (user, 2026-11-05), not 7 days. No Windows dumps yet, and 0036 U4 stands: OS crash reports by name and time. The
map's `crash.os_report` with exception and frames is therefore **not** in this ADR. `crash_reports.rs` moves to `paddock_diag` so that every app
gets it (rule 42).

### 5. A run index: the evidence that outlives the window

Every `run.*` line, panic and Fatal is also appended to `<prefix>.runs.jsonl` beside the daily files, written by `paddock-log`.

- **At write time, not by a scan.** Scanning 30 days at report time reads up to 100 MB per process, for every report (see "Parallelism and cost").
  This appends about 10 lines a run.
- **Not limited by the day's budget or the rate limit**, so a Debug session that fills the budget keeps its `run.exit`.
- **Bounded.** At most 1 MiB; trimmed to the newest half at install only, never on the write path, by temp file and rename. A crash loop goes through
  install every time, so it stays bounded.
- **In the bundle:** `paddock_diag`'s `Logs` section adds it outside the 4 MiB window. It goes through the same redaction as the line it
  copies and again through the bundle's. The name does not end in `.log`, so the window and the day pruner must skip it; WP-1 tests both.

### 6. Rate limit on Info and Warn

In `paddock-log`, per call site (target, file and line from the record, since formatted messages defeat a message key): 20 lines a minute, then one
`log.suppressed` line with the count when the window ends and at exit.

- Never limited: `run.*`, panic, Fatal. Errors keep their dedupe. Debug has the half-budget.
- Breadcrumbs are taken before the limiter, so the trail stays whole.
- The key table is bounded (256 sites; past it, all others share one bucket). `FakeClock` makes the test deterministic.
- `LogSettings` gets an optional `rateLimitPerMinute`, merged over the default; 0 turns it off for a support case.

### 7. Correlation without a wire change

Amends ADR 0036 §3: **no `op` frame field.** The wire already has what is needed: `Frame::Call { id }`, chosen by the caller per direction
(`frame.rs:174-176`, `peer.rs:404`), and `Hello.pid`, so the callee knows the caller's process.

- `paddock-ipc` exposes the call id to both sides. The caller writes `OperationId = "<ConnectionId>.<CallId>"`; the callee writes its own
  `ConnectionId` and `CallId`.
- Both sides log the connection with `PeerPid` (the agent's `Client connected`, `server.rs:66`, gains it). The reader pairs connections by the two
  pids and the nearest time, then calls by id. It says `unjoined` rather than guess when that is not unique.
- A job started for a request writes that `OperationId` on `job.start`.
- Dyno's `r<n>` is replaced by this; the app's breadcrumb line then ties to the agent's and the helper's (the helper is on `paddock-ipc` too).
- **Other wires,** by ADR 0049's rules: Pitcrew's JSON protocol can add `op` with a serde default (additive). Pitwall's and Debrief's own
  wires decide at adoption; Debrief's store client is positional, so it starts without a join.

### 8. Slow and failed calls

`paddock-ipc` gets one helper the caller invokes with the request's name: `ipc.slow` (answered, over budget), `ipc.timeout`, `ipc.failed`, with `Name`, `Ms`,
`OperationId`, never fields. Budgets: 1 s for a UI call, 10 s for declared long ones; the map's numbers, not measured. `ipc.slow` is Info. It relies on §6 for bursts.
`ipc.summary` (the map's hourly counts) is left out until a case needs it.

### 9. Jobs lines

`paddock-jobs` writes `job.start` (`JobId`, `Kind`, `OperationId` if known), `job.phase` at each phase's end (`Phase`, `Ms`, `Items`), and `job.end` from a drop
guard, so a panic or early return still writes it (`Outcome`, `Ms`, `Items`, the chain on failure).

- Never `title` or `subject`, which can hold names. `paddock-jobs` cannot reach `fingerprint` (§1), and `Kind` is what a reader needs.
- Never per item. Dyno's ad hoc lines (map: `disk/centre.rs:267`, `helper_centre.rs:167`) go in the same change.
- Land before ADR 0052's WP-B1 where possible, so WP-B1 only changes `Outcome`'s source.

### 10. One reader: `./dev log-explain`

```text
./dev log-explain <bundle.zip | folder> [--json] [--expect <profile>]
```

- **Pure core:** a new crate `crates/paddock-log-read` with `serde_json` only: lines to runs, runs to findings. `tools/paddock-dev` is the thin CLI
  and adds the `zip` reader (`zip`, already in `paddock-diag`'s tree). No UI: Debrief opens CLEF for reading by eye.
- **Output,** per process and run: start reason, version, phases, ready, exit reason, previous verdict, level counts, then findings such as `MISSING
  run.exit` or `run.previous says unclean`, with a gap since the last line. The index is read first; day files fill in detail.
- **It says what it cannot see (rule 71).** A log cut by the window, a process with no file, a file in another format: `not measured`, never "clean".
  Dyno's helper lifecycle file and `helper.log` are plain text, so they are listed as unparsed until they move to CLEF (map #25).
- **Reading older logs.** Dyno 1.1.0 ships before any `EventId`, and Spotter may too. An alias table maps those exact shipped messages to `run.*`,
  marks findings `inferred`, and is deleted with Dyno 1.2's tag. This is the one place messages are matched.
- **A parser:** never panics on hostile input (C36 never-panics test), line size bounded at 64 KiB. Fixtures are invented (ER-14).
- **Contract test.** `paddock-log-read`'s dev-dependencies on `paddock-log` and `paddock-startup` write every `run_log` line to a temp folder through a
  real `Logging` and read them back, so writer and reader cannot drift.
- **`--expect`** names the sequence a VM step requires, for ADR 0054's L-steps. Before it counts, it must fail on a positive control: `kill -9` of the app
  in one run must report unclean (rule 71). A missing precondition reports `not measured`.

## Parallelism and cost (rule 51)

**Not measured:** no build was allowed. WP-1 and WP-3 measure with synthetic data and record in `docs/benchmarks.md` (rule 48) before these numbers are quoted.

| Operation | Input bound | More workers? | Cheaper? |
|---|---|---|---|
| Write a line | Caller's thread, a lock per sink (0036) | None proposed | ADR 0036's WP-10 is still open and now covers the limiter: over about 20 µs a line on the slowest CI machine, or a blocked burst, moves the write to one writer thread |
| Rate limiter | One bounded table per line | No | A hash and a table entry; no per-message text |
| Run index | About 10 lines a run, 1 MiB | No | Written at the source instead of scanned; trim only at install |
| Report a problem | 16 MiB (`report.rs:30`) plus 1 MiB an index per process; already a job | No change | The index is what spares a 30-day scan |
| **Reader, bundle** | At most 16 MiB, about 60,000 lines at the map's 281 B | Serial first: small input, one user | Index first; substring test for `"EventId"` and level before a full JSON parse of day files. Target: under 1 s; check it |
| **Reader, folder** | Up to 100 MB per process plus today's file: about 1.1 million lines for three processes | One worker per file, `--threads auto` (cores minus one) only if WP-3 measures over 2 s serial; results sorted by process and `@t`, so output is deterministic | The same; `--since` to limit days; progress per file on stderr with Cancel by Ctrl-C (ER-18), and a read-only tool leaves no state |

Volume: the map's model gives about 2,400 lines and 0.7 MB a day with its additions [judgement from measured line sizes in VM logs]. `live update` is about 80% of a
quiet day; WP-D5 thins it to one line per 10 minutes.

## Work packages

Sizes in agent-days, my judgement. **Shared changes wait for the Dyno tag and then land on the days the coordinator names (decision F6).** A
branch may hold them before that.

| WP | What | Size | Kind | When |
|---|---|---|---|---|
| WP-0a | Land ADR 0036, rule 56, the logging map and this ADR on main (docs only) | 0.25 | Docs | Any time |
| WP-0b | Register in `docs/known-copies.toml`: own loggers (ADR 0036's `process-log`, never registered), Dyno's helper lifecycle file. `dyno/wp-int` changes this file, so after its landing | 0.25 | Shared | After the tag |
| WP-1 | `paddock-log`: run index, rate limiter and setting, run counters, shared capture helper for tests, cost measurement (0036 WP-10) | 2 to 3 | Shared | After the tag |
| WP-2 | `paddock_startup::run_log`, marker version; shell wiring in `on_run_event`, default commit, `paddock_env::os::version` with the build (0036 WP-2); ids on `tool.run`, `update.*`, `process.paused` | 2 to 3 | Shared | After the tag; needs WP-1's counters for `run.exit` only |
| WP-3 | `paddock-log-read`, `./dev log-explain`, alias table, contract test, reader measurement | 3 to 4 | Shared (tools, new crate) | Branch any time, in the code slot; lands after WP-2 |
| WP-4 | `--expect`, positive control, first VM profile | 1 to 1.5 | Shared (tools) | After WP-3; the L-steps need ADR 0054's suite |
| WP-5 | `paddock-jobs` lines, with Dyno's ad hoc lines removed in the same change (rule 42) | 1 to 1.5 | Shared | After the tag, before ADR 0052 WP-B1 |
| WP-6 | `paddock-ipc`: call id to both sides, `PeerPid`, `ipc.*` helper | 1.5 to 2 | Shared | After the tag |
| WP-7 | `crash_reports.rs` to `paddock_diag`; index in the `Logs` section | 1 | Shared | After the tag |
| WP-8 | Level lint, report-only: a failure word in an Info line, one suppression comment with a reason. Starting point on `dyno/wp-int`: 15 single-line hits (grep; the map counted 35 earlier). Next free check number | 0.5 | Shared | Any free window |

**Dyno 1.1.1** (the items already listed for it, on the shared packages above; nothing before the 1.1.0 tag):

| WP | What | Size | Kind | Needs |
|---|---|---|---|---|
| WP-D1 | Correlation ids: app, agent and helper write `OperationId`, `ConnectionId`, `CallId`; `r<n>` goes | 0.5 to 1 | App-only | WP-6 |
| WP-D2 | `ipc.slow` and `ipc.timeout` in `AgentLink::call` and the agent's and helper's handlers | 0.25 to 0.5 | App-only | WP-6 |
| WP-D3 | Rate limit: nothing to write. Check the VM logs for what it suppresses | 0.25 | App-only | WP-1 |
| WP-D4 | Jobs lines: delivered by WP-5; check the bundle shows a disk scan's `job.*` | 0.25 | App-only | WP-5 |
| WP-D5 | Onto `run_log`: delete the lines from `lib.rs`'s callback and `cli.rs`; thin `live update`; back off the refused agent's 10 s repeat (see Risks) | 0.5 to 1 | App-only | WP-2 |

### Adoption, per app

An app gets the retention, window and `tool.run` by merging Dyno. The rest needs the following. Each app's row is checked in its release
branch's tag round by `./dev log-explain` when WP-3 has landed.

| App (order) | State, read at | To do | Size | Kind |
|---|---|---|---|---|
| **Debrief 1.2,** main focus: the first adopter of the shared packages | `debrief/1.2-lean` `fdf577eb7`: own `logging.rs`, `clef.rs`, plugin API `clef.rs`, about 350 log lines, 190 `eprintln!`; own `lifecycle.rs` (`delete_by = debrief-v1.2.0`, `run-lifecycle`); workers without a logger (0036) | `paddock-log` in every process, `run_log` through the shell (M-packages) | 3 to 5 | App-only, on the shared WPs |
| **Spotter 1.0,** tags when ready | `spotter/1.0-prep` `e98e37cf5`: `install` (`lib.rs:381`), `paddock_diag` wired, `applog.rs` still used (12 references); no `run_marker` | **Tag obligation (0036 U5):** `applog.rs` out and `install_with_commit`. `run.*` if WP-2 has landed by its freeze; otherwise it ships on today's lines, which the alias table reads, and takes `run.*` in its next release | 0.75 | App-only |
| **Pitwall 1.1** | `pitwall/1.1-final` `b7151a57f`: `install` (`lib.rs:392`), no `paddock_diag`, `eprintln!` in the app and server; `pitwall-server` has no `paddock-log` dependency | Logger in the server, `run_log`, the bundle. Its plan's owner places these; this ADR changes no Pitwall package | 1.5 to 2.5 | App-only |
| **Pitcrew 1.1** | `steward/pitcrew-1.1-prep` `40be30b04`: `install` (`lib.rs:252`), `applog.rs`, `pitcrew-service/src/logfile.rs`; the service has no `paddock-log` dependency | Both own loggers out, service on `paddock-log` in a folder the user can read (0036 §1), the runner's last lines kept, `op` on its JSON wire | 2 to 3 | App-only |
| **Pacenotes, Sighter, Recce** | No shipped process | `scripts/new-app.mjs` template uses the shell's wiring and `run_log` | 0.25 once | Shared (template) |

Shared total: about 12 to 16. Dyno 1.1.1: about 2 to 3.

## Order

1. WP-0a now. WP-3 on a branch when the code slot is free; the waiver sweep comes first.
2. Dyno tag, then its landing on main.
3. WP-1, WP-2, WP-5, WP-6, WP-7 on the named days: Debrief's work needs them, and Pitwall's after that. Then WP-3's landing, WP-4.
4. Dyno 1.1.1's packages; each app's row at its turn.

## Consequences and risks

- **Text-matching tests get an exit.** Probes that read start lines (`dyno-agent/tests/term.rs`, per the map) move to `EventId` when they next change.
- **False confidence.** The reader reports what lines say; a line saying "exit" is only as good as what wrote it. Rule 71's positive control and `not measured` are the guard.
- **Privacy.** `run.*` holds no paths or names: enums, numbers, versions. `Why` and `ErrorChain` pass 0036 §6's scan. The index lasts longer than a day file (up to 1 MiB of history), so its fields are an allow-list in `run_log`, tested with invented data.
- **The refused agent can evict the index.** A process refused every 10 s (lessons, 2026-11-05) writes about 2 lines per try: 1 MiB is gone in a few hours. WP-D5's backoff is the fix; each such app needs the same.
- **Second reader of the marker file:** none new; `run_marker` stays the only one.
- **Shared-change collisions.** WP-2 touches `paddock-shell` and `paddock-startup`, which Spotter's and Pitwall's branches change too (plan §1). They follow by merge.
- **The limiter hides a repeat that matters.** The first 20 lines a minute and the count are kept; 0 turns it off.
- **ADR 0036 is Accepted but not on main,** and its citations of rule 56 are too. WP-0a closes both. Its `delete_by` table names `dyno-v1.0.0`, and its C63 to C65 are unbuilt; I found no use of those numbers on main. Those stay 0036's own work.

## Alternatives considered

- **Id as a message prefix** (as `process.paused:`): no property to filter, and a reword breaks it. Rejected, kept only as words.
- **`paddock_log::event!` macro or typed helpers for every family:** needs the clock-port move first, and adds a layer for a string. Rejected; only `run.*` is typed, because it has several writers and a reader.
- **`op` in the frame (ADR 0036 §3):** breaking under postcard (ADR 0049). **A new frame variant carrying it:** additive by ADR 0049's rule, but the sender must first know the peer has it, for a join the existing ids give. Rejected.
- **Build the index by scanning at report time:** no new file, but up to 100 MB per process each time, and it loses what the budget dropped. Rejected.
- **`tracing` spans for runs, OpenTelemetry:** spans only where a scope wants a duration (map §2); no collector on a personal machine. Rejected.
- **Reader inside each app:** a family log tool belongs with `./dev`, and in-app reading has no asked-for use. Rejected for now; the pure crate leaves the door open.

## Decisions

No user decision is pending. The user's, applied here: F3 (write this ADR), 30 days with a 100 MB per-process cap, no Windows dumps yet, Dyno's ample logging in 1.1.0, ADR 0036 U2 to U5.

For the reviewer and coordinator: the two amendments of an Accepted ADR (§4 of this ADR for U1, §7 for 0036 §3); whether the one-code-agent cap holds after the Dyno tag.

## Rollback

Lines and a side file. `rateLimitPerMinute = 0` turns the limiter off. The index can stop being written without touching day files; the reader then says `not measured` for what it held. Nothing here changes a wire or a stored format except the marker's extra token, which older code ignores.
