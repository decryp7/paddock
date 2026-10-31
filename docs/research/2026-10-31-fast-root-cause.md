# Finding root causes fast (research, 2026-10-31)

Question (user): how do we solve issues and find their root cause very quickly? Scope: the Paddock family (Tauri apps with a
Rust core and an Angular UI, background agents and services on macOS, Windows and Linux), one Gitea CI with one Mac
runner, and many AI agents fixing bugs in parallel. Status: research note, nothing decided. Sources were read on
2026-10-31 unless a date says otherwise; claims not checked against a primary source are marked *(unverified)*.

## Summary

Our slow diagnoses did not lack clever tools. They lacked **evidence at the moment of failure** and **a discipline that
rejects a guessed cause**. Of the five cases in the brief, four were first explained wrongly, and each was settled by a
measurement or a log line that could have existed from the start:

| Case (lessons.md) | First explanation | Real cause | What would have shortened it |
|---|---|---|---|
| UI spec timeouts (2026-10-31, `fix/panel-snapshot-spec-cost`) | CSS cost piling up | A leaked fake timer from an earlier spec in the same worker (`isolate: false`) | A per-test leak check (now in the test kit) and a **polluter bisect** over the files run before the victim |
| Dyno jobs test (2026-10-31 lesson) | "a flake" | A race in the issue scan | **Stress runs with a repeat count** and a hypothesis list that may not end in "flake" |
| Windows rustup crash in CI (`ci/setup-tools-hardening`) | Unknown: exit 127 only | A native crash whose NTSTATUS Git Bash maps to 127 | A wrapper that **logs the raw exit code** (`conformance/ci/run-native.mjs`, on that branch) |
| Dyno 30 s login connect (rule 56, branch `docs/rule-logs-explain-failures`) | Guessed from outside | Not visible: only failed connects were logged | **Phase spans with durations and success lines** |
| A harness that blocked its own event loop | Slow product | Synchronous work on the harness's loop | An **event-loop stall watchdog** in the harness (measured below) |

Earlier lessons say the same: four rounds of Windows CI were spent guessing at `net::ERR_ABORTED` while the answer sat in
Chrome logs CI had already kept as artifacts ("Read the failing environment's own logs before building a reproduction",
2026-10-30); the `dwState` 0 case was solved only after the product printed the raw value (2026-10-30).

So the ranking below puts **evidence capture and evidence-first triage** at the top, then the cheap mechanical search
methods (stress counts, bisect, order bisect), and the heavy tools (record and replay, deterministic simulation) lower:
they are powerful, but each fits few of our bugs.

## 1. Ranked practices and tools

"Time saved" is the expected agent and CI time saved per month on the bug mix in lessons.md (races and waits in
supervisors, test isolation, platform drift, CI tooling, native crashes), judged from the cases above, not measured. Cost
is engineering effort; S = under a day, M = a few days, L = a week or more.

| # | Practice or tool | Time saved | Cost | Fit for Paddock |
|---|---|---|---|---|
| 1 | Evidence first: one evidence bundle per failure, read before code | Very high | S–M | Every bug class; agents especially |
| 2 | Phase spans with duration and outcome in `paddock-log` (rule 56 made mechanical) | Very high | S (crate) + M (apps) | Starts, connects, jobs, supervisors |
| 3 | Hypothesis list with a falsifying measurement per item ("differential diagnosis") | High | Zero (skill text) | Every bug; stops "it's a flake" and "it's CSS" |
| 4 | Stress with a repeat count before and after the fix (nextest `--stress-count`, Playwright `--repeat-each`, Vitest loop) and flaky-as-failure | High | S | Races, waits, test isolation |
| 5 | Compare with the last green run automatically (commits, OS build, tool versions, runner, durations) | High | S–M | CI-only failures, platform drift (ER-26) |
| 6 | Order-dependence search: shuffled order with a printed seed, then a polluter bisect | High for UI specs | S | Vitest `isolate: false` bundles |
| 7 | Flight recorder: an in-memory ring of debug events dumped on error, panic, stall and Report a problem | High for field bugs | M | Users' machines, where we cannot attach a debugger |
| 8 | Raw exit codes, NTSTATUS and stderr tails from every wrapper; stall watchdogs in harnesses | Medium–high | S | CI tooling, harnesses, sidecars |
| 9 | Automated `git bisect run` with repeat counts for flakes, `--first-parent` over merge batches | Medium | S | Regressions with a known good commit |
| 10 | Native crash capture: out-of-process minidumps, OS crash reports collected locally, opt-in | Medium (rare, but otherwise blind) | M | DuckDB, WebView2/WebKit, FFI, aborts |
| 11 | Deterministic concurrency testing: model checks of step machines (Dyno ADR 0025 style), `shuttle`, `loom` for small primitives | Medium | M | Supervisors, pools, queues |
| 12 | Property-based tests with shrinking (`proptest` already used; `fast-check` for TS) | Medium | S per area | Parsers, rules, state machines |
| 13 | Sampling profilers for latency: `samply` (all three OS), Instruments, WPR/WPA | Medium for perf bugs | S | Slow imports, scans, starts |
| 14 | Failure classification and job summaries in CI | Medium | S–M | Triage of many red jobs |
| 15 | Short RCA record per bug, cause class tagged, feeding ER-21 | Medium over time | S | Recurrence |
| 16 | Record and replay: `rr` (Linux), Windows TTD (local VM only) | Low–medium | M | Rare native heisenbugs |
| 17 | Delta debugging of inputs (fuzz minimisers, ddmin) | Low (already covered by fuzz `tmin`) | S | Parser crashes |
| 18 | Deterministic network/fs simulation (`turmoil`) | Low today | L | Only tokio networking (Debrief, Pitwall, Spotter) |

### 1.1 Evidence first, one bundle per failure

**Practice.** Before reading code, collect what the failing environment already wrote, in one folder, and read it in a
fixed order: the failing step's error lines, the app's phase log around the failure, crash markers and OS crash reports,
the test's own output and trace, then what differs from the last green run. The lesson of 2026-10-30 is exactly this: the
CI artifacts held the answer while four rounds guessed.

**Paddock fit.** `paddock-diag` already builds a redacted bundle from sections (system facts, settings, crash marker, the
newest part of the log; `crates/paddock-diag/src/lib.rs`), and it is pure Rust with no Tauri dependency, so a command-line
tool can write the same report. What is missing is the **developer and CI side**: one command (`paddock-dev evidence
<run|test>` or `scripts/evidence.mjs`) that gathers, for a failed CI job or a local test, the job log's error lines, the
JUnit report (nextest already writes `junit.xml`, `crates/.config/nextest.toml`), artifacts (Chrome net logs, Playwright
traces), the app logs under the job's temp home, crash files, and a last-green diff (1.5), into
`<target>/evidence/<id>/` with a `README.md` index. Agents get the folder path, not the log.

**Why it saves the most time for agents.** Agents fix bugs in parallel and each starts with no memory. Code-first
diagnosis by a fresh agent is where guessing comes from. Published agent work points the same way: Agentless (Xia et al.,
arXiv 2407.01489, v2 2024-10-29) localises first and validates candidate patches with **reproduction tests**; SWT-bench
(Mündler et al., NeurIPS 2024, arXiv 2406.12952) found that generated reproduction tests can filter proposed fixes and
raise repair precision *(the size of the gain is from secondary summaries; not checked against the paper)*. Anthropic's
Claude Code best practices (code.claude.com/docs/en/best-practices) say to give the agent "a failing test it has to turn
green" and to ask for evidence ("the command it ran and its output"), not "all tests pass".

### 1.2 Phase spans with duration and outcome

**Practice.** Every start, connect, job and wait is a span; when the span closes, one line records its phase, duration and
outcome (success too), with correlation ids (job id, connection generation, child generation). This is what rule 56
asks for; the fastest way to make it routine is to make it free in the shared crate.

**Paddock today.** `paddock-log` writes CLEF JSON lines, bridges `tracing` and `log`, copies the fields of enclosing spans
onto every event (`ClefLayer::on_event`, `crates/paddock-log/src/core.rs`), and has `ipc_span` with `Window`,
`OperationId` and `Command` (`webview.rs`). It does **not** write a line when a span closes, so durations exist only where
someone logs them by hand. `tracing-subscriber`'s formatter can emit an event when a span is dropped (`FmtSpan::CLOSE`,
docs.rs, tracing-subscriber 0.3.23) *(the `time.busy`/`time.idle` fields are from memory, not the page)*; Paddock uses its
own layer, so the change is an `on_close` in `ClefLayer` for spans marked as phases (e.g. a `Phase` field), writing
`Phase`, `ElapsedMs` and `Outcome`.

**Correlation without a backend.** The W3C Trace Context `traceparent` shape (trace id, span id) is what OpenTelemetry
propagates (w3.org/TR/trace-context, *unverified, from memory*). We need only the idea: one id minted where work starts
(UI command, job, agent connect) and carried across IPC to the agent and its children, so one `grep OperationId=…` over
the app's and agent's files shows the whole flow. No collector, no network: compatible with the offline principle.

**Test.** A capture sink in `paddock-log` for tests (`paddock_log::testing::Capture`) so a fix asserts "this line exists
with this phase and outcome" (rule 56 asks for it "where practical"; this makes it practical).

### 1.3 Hypothesis list with a falsifying measurement

**Practice.** Differential diagnosis: write every plausible cause before acting, and for each the observation that would
rule it out; then make the cheapest such observation in the failing run. A cause is "confirmed" only after a run without it
stays green (lessons 2026-10-30). "Flake" is never a hypothesis; it is a symptom. "Measure first": the leaked-timer case
had a leading hypothesis (CSS cost) that one measurement inside the failing run disproved.

**Cost.** Zero: it is skill text and a section in the hand-off. It also gives reviewers something to check.

### 1.4 Stress counts and flaky-as-failure

**Tools and facts (primary docs).**
- **nextest** stress mode: `--stress-count=N`, `--stress-count=infinite`, `--stress-duration=90m`, since 0.9.103; it does
  not run the same test in parallel (nexte.st/docs/features/stress-tests). Flaky handling: a test that fails then passes on
  retry is FLAKY; `flaky-result = "fail"` (profile key, `--flaky-result fail`, `NEXTEST_FLAKY_RESULT=fail`) fails the run
  anyway and JUnit gets `<flakyFailure>`, since 0.9.131 (nexte.st/docs/features/retries).
- **Playwright**: `--repeat-each <N>`, `--fail-on-flaky-tests`, `--last-failed`, `--only-changed [ref]`
  (playwright.dev/docs/test-cli).
- **Vitest**: `--sequence.shuffle` and `--sequence.seed=<n>` (vitest.dev/config/sequence).

**Paddock today.** The crates' CI profile has `retries = 0` (rule 5). But five app Playwright configs retry in CI:
`apps/debrief/app/playwright.config.ts` (2, `trace: 'on-first-retry'`), `apps/pitwall` (2), `apps/dyno`, `apps/pitcrew`,
`apps/spotter` (1), while `ui/playwright.config.ts` says retries hide flakes and sets 0. A retried-and-passed test is
green today, which ER-12 and rule 5 forbid. Two consistent options (decision D2):
- (a) `retries: 0` everywhere; traces `retain-on-failure`.
- (b) `retries: 1` with `failOnFlakyTests: true` (CLI `--fail-on-flaky-tests`), and nextest `retries = 1` with
  `flaky-result = "fail"`: the run is still red, but the report says **FLAKY** and the retry captures a second trace,
  which is evidence. This is not "passes on re-run": it never turns red into green.

**Repeat counts.** To call a run "fixed" with confidence c for a failure rate p, the test must pass
n = ⌈ln(1−c)/ln(1−p)⌉ times in a row (computed here):

| Failure rate p | n for 95 % | n for 99 % |
|---|---|---|
| 50 % | 5 | 7 |
| 20 % | 14 | 21 |
| 10 % | 29 | 44 |
| 5 % | 59 | 90 |
| 2 % | 149 | 228 |

The bug-fix skill's "20–50" covers p ≥ 10 % at 95 %. For a rarer flake, first **raise p** (load, a single core, a gated
start as in the Dyno lesson "reproduce with a start gated by the test, not with load", order shuffling) until it fails
often, then count.

### 1.5 Compare with the last green run

**Practice.** For any red job, diff it automatically against the last green run of the same job on the same runner:
commit range, changed files, runner name, OS build and installed updates, toolchain versions (rustc, node, Chrome for
Testing), cache hits, and per-test durations. ER-26 exists because a Windows preview update (KB5127653) broke CI with no
code change; a diff that prints "OS build changed 28000.3351 → 28000.3365" saves the day it took.

**Paddock fit.** `scripts/ci-timings.mjs` already reads the Gitea Actions API (runs, jobs, step timestamps, logs). Each
job writes an environment fingerprint (a small JSON: OS build, toolchain versions, runner) as an artifact or log line;
`scripts/last-green.mjs` fetches both fingerprints and the commit range and prints the differences. Google reports that
about 84 % of pass→fail transitions they observed involved a flaky test (Micco, "Flaky Tests at Google and How We
Mitigate Them", Google Testing Blog, 2016-05-27), so "what else changed besides the code" is the first question, not the
last.

### 1.6 Order dependence: shuffle, then bisect the polluter

**Practice.** A "victim" passes alone and fails after a "polluter" unless a "cleaner" runs between them (Shi, Lam, Oei,
Xie, Marinov, iFixFlakies, ESEC/FSE 2019; the iDFlakies tool, github.com/UT-SE-Research/iDFlakies). Given a failing full
run, take the list of spec files that ran before the victim in its worker, and bisect it: run victim + first half; if it
fails, recurse there, else the other half. That is delta debugging (ddmin; Zeller and Hildebrandt, "Simplifying and
Isolating Failure-Inducing Input", IEEE TSE 28(2), 2002) applied to a test order, and it finds one polluter among 200
files in about 8 runs.

**Paddock fit.** The Angular builder runs Vitest with `isolate: false` (rule 49; `ui/projects/paddock-ui/testing/src/setup.ts`),
so this class recurs. The test kit now catches leaked timers per test; a `scripts/find-polluter.mjs` (Vitest file list
plus `--sequence.seed` from the failing run) catches the leaks the kit does not know about. Nightly: one shuffled run per
app with the seed printed, so order dependence is found by design, not by luck.

### 1.7 Flight recorder

**Practice.** Keep the last N debug-level events in memory, below the file's level, and write them out only when
something goes wrong (an error, a panic, a stall judgement from `paddock_proc::stall`, Report a problem). Field bugs then
come with the detail of a debug log without its disk cost or its privacy surface on every day.

**Paddock today.** `paddock-log` keeps breadcrumbs: 200 entries per window, 30 attached to each error, only for events
with an `Operation` property (`breadcrumbs.rs`). The recorder generalises this: one bounded ring per process (for example
4,096 events or 2 MB, whichever first), filled after the same `Redactor` runs, so it adds nothing sensitive; dumped as a
`flight-<time>.log` next to the day's file and picked up by `paddock-diag`'s log section. Cost: one layer and a setting.
Memory: bounded, about 2 MB per process.

### 1.8 Raw exit codes and stall watchdogs

**Raw codes.** Git Bash maps unknown NTSTATUS codes to exit 127 (Cygwin `pinfo::status_exit`, cited in
`conformance/ci/run-native.mjs` on `ci/setup-tools-hardening`), which is why the rustup crash was blind. Every wrapper of a
native program (CI steps, `paddock-proc` spawns, sidecars) logs the exact command, the raw code in decimal and hex with
its NTSTATUS name or signal, and the stderr tail. `paddock_log::spawn_child_capture` already captures stderr; the
NTSTATUS table belongs in `paddock-proc` too, so the product's own spawn failures name the crash.

**Event-loop watchdog (measured).** A harness that blocks its own event loop looks like a slow product. Measured here on
Node 24.22.0, macOS (Mac mini), synthetic, three runs each: a 300 ms synchronous block (`spawnSync('sleep', ['0.3'])` and
a busy loop) was **not** recorded by `perf_hooks.monitorEventLoopDelay({ resolution: 10 })` (max 10–12 ms in every run,
armed before the block), while a 10 ms `setInterval` that records its own lateness reported 296–306 ms each time. So the
harness watchdog should be the plain interval, not the histogram (Node docs: monitorEventLoopDelay "samples and reports
the event loop delay over time", added in v11.10.0; nodejs.org/api/perf_hooks.html). The watchdog logs a line naming the
stall and its length; it never fails a test by time (ADR 0031), it explains.

For Rust async code, `tokio-console` (MIT, 0.1.14) shows tasks whose polls take long; it needs `console-subscriber` in the
binary, so dev builds only. For threads and child processes, `paddock_proc::stall` (`StallCause::Spinning/Idle/Hung`) is
already the judgement; it should log its evidence (CPU time, last progress) each time it reports.

### 1.9 Automated bisect, also for flakes

`git bisect run <cmd>`: exit 0 good, 1–127 except 125 bad, 125 skip (untestable, e.g. build fails), anything else aborts;
`--first-parent` follows only first parents, so a merge commit is blamed instead of a broken commit inside a merged branch
(git-scm.com/docs/git-bisect). On main, merges are batches (`chore(merge): batch …`), so `--first-parent` finds the batch
in log₂(batches) steps, then a second bisect inside it finds the commit. For a flake, the script runs the test n times per
step (table in 1.4) and exits 1 on any failure; build failures exit 125. A `scripts/bisect-flaky.sh <test cmd> <n>`
with `CARGO_BUILD_JOBS=2` and a per-bisect `CARGO_TARGET_DIR` is a few lines. Cost per bisect: steps × n × test time, so
it is worth it once p is raised (1.4).

### 1.10 Native crash capture, local and opt-in

**What exists.** `paddock-log`'s panic hook and `CrashMarker` record Rust panics. Nothing records a native crash (segfault,
abort in DuckDB or a webview, stack overflow, Windows fast-fail). Debrief can already open minidumps
(`apps/debrief/app/src-tauri/src/dumps.rs`), so the family can read what it would write.

**Options.**
- **In-process handler + out-of-process writer**: Embark's `crash-handler` 0.8.1 (MIT OR Apache-2.0) catches the signal or
  exception; `minidumper` 0.11.0 (MIT OR Apache-2.0) is "an IPC client and server for creating minidumps for an external
  process", writing via `minidump-writer` 0.13.0 (MIT). Targets: x86_64 Linux, Windows MSVC, macOS; aarch64 Linux and
  macOS (github.com/EmbarkStudios/crash-handling). Windows aarch64 is not listed: a gap for Windows on Arm.
  Paddock's agents and services already have a supervisor, which can host the writer.
- **OS reports**: macOS writes crash reports to `~/Library/Logs/DiagnosticReports/` as `.ips` (JSON header line plus JSON
  body) whether or not the user shares analytics *(unverified: Apple's pages did not render for the fetcher; from
  memory)*. `paddock-diag` can add a section that copies only reports whose process name is the app's own, newest first,
  bounded. Windows WER `LocalDumps` writes local dumps (default `%LOCALAPPDATA%\CrashDumps`, `DumpCount` 10, `DumpType`
  1 = mini) but **needs administrator rights** under HKLM and is "not enabled by default"
  (learn.microsoft.com/windows/win32/wer/collecting-user-mode-dumps, updated 2025-03-11); the app may read existing
  dumps there, not configure it, except via the planned privileged helper. Linux: `coredumpctl` where systemd-coredump is
  present *(unverified)*.
- **CI**: on a crash, keep the dump or report as an artifact (1.1).

Privacy: a minidump holds stack memory, so it may hold user content. It stays on the machine, is listed in the Report a
problem preview, and is included only when the user ticks it (decision D4).

### 1.11 Deterministic concurrency testing

Dyno's supervisor needed five rounds of fixes until it became a pure `step(event) -> commands` machine with a bounded model
check of every interleaving, proven by making each historical bug a mutant (lessons 2026-10-30, Dyno ADR 0025). That
pattern is the most effective one we have for supervisors, and it needs no library.

- **Shared explorer**: lift Dyno's breadth-first checker (state bound, parent pointers for the trace) into
  `paddock-testkit` as a small generic `explore(machine, events, bound)`. Every supervisor or start judge
  (`paddock_proc::StartJudge` is already `Clone` for this) gets it for free.
- **`shuttle`** 0.9.9 (Apache-2.0, AWS Labs): random schedules of real threads and `std::sync`/tokio wrappers, reproducible
  from a seed, scales to larger tests than loom but proves nothing (github.com/awslabs/shuttle). Fits pools and queues
  (`paddock-jobs`, `paddock-download`'s work stealing).
- **`loom`** 0.7.2 (MIT): exhaustive interleavings of small primitives; only for a lock-free or hand-rolled sync type.
- **`turmoil`** 0.7.x (MIT, tokio-rs): single-threaded simulation of hosts, network and file system faults from a seed
  (github.com/tokio-rs/turmoil). Only tokio networking fits (Debrief, Pitwall, Spotter list tokio); Paddock's IPC is
  its own socket layer, so this is later at best.

### 1.12 Property-based tests with shrinking

`proptest` 1.12.0 (MIT OR Apache-2.0) is already in 36 manifests. Use it more for state machines (generated event
sequences into `step`, shrunk to the shortest failing sequence) and for parsers alongside the never-panics corpus. For
TypeScript domain-free utilities, `fast-check` (MIT *(licence unverified here)*) does the same; but most logic belongs in
Rust (AGENTS.md), so this is minor.

### 1.13 Sampling profilers for latency

- **samply** 0.13.1 (MIT OR Apache-2.0): macOS, Linux (perf events, on-CPU only), Windows; profiles saved locally
  (`~/.local/share/samply/profiles/`, `%LOCALAPPDATA%\samply\profiles\`); "All data is kept locally … until you choose to
  upload"; the viewer is the Firefox Profiler web page served against a local symbol server (github.com/mstange/samply),
  so the viewer needs network unless self-hosted *(offline use not stated)*.
- **Instruments / `xctrace`** (Apple, free with Xcode, macOS only) and **WPR/WPA** (Microsoft, free, Windows) for
  system-wide traces including waits and I/O. **DTrace** on macOS is limited by System Integrity Protection
  *(unverified)*; **eBPF** (`bpftrace`, `perf`) on Linux needs root and runs in the Linux VMs only.
- `tracing-chrome` 0.7.2 (MIT) writes spans as a Chrome trace for the Perfetto or `chrome://tracing` viewer: with
  phase spans (1.2), a slow start becomes a picture with no profiler at all.

Rule 48 applies: any number quoted is recorded in the app's `performance.md`.

### 1.14 Failure classification and job summaries in CI

Classify each failed job into infrastructure (runner offline, cache server, network), tool crash (raw code), build,
lint/type, test (name), and timeout, from known log signatures, and print one `::error::` line per job with the class,
the first error line and the next step (rule 56's CI half). Job summaries: Gitea issue 23721 (`$GITHUB_STEP_SUMMARY`)
was closed on 2026-06-08 by PR 37500, milestone **1.27.0**; runner support (gitea/runner PR 917) uploads only when the
server advertises it; neither side breaks older jobs (github.com/go-gitea/gitea/issues/23721). Which Gitea and runner
versions we run is not recorded in the repo (decision D6). Until then, a `summary.md` artifact per job does the same.

A **flake history**: keep each CI run's `junit.xml` (nextest) and Playwright JSON, append outcomes per test and commit to a
small JSON or SQLite file in the nightly job, and list tests with mixed outcomes on the same commit. Meta's "Probabilistic
Flakiness" (engineering.fb.com, 2020-12-10) argues for "how flaky", a score per test over time, not a yes/no; a simple
fail-rate per test per week is enough for our size.

### 1.15 RCA records and ER-21

The Google SRE book's postmortem chapter (sre.google/sre-book/postmortem-culture) asks for triggers set in advance,
blameless writing focused on contributing causes, preventive action items, and review ("An unreviewed postmortem might as
well never have existed"). A linear "5 whys" tends to stop at one cause; the incident-analysis literature prefers a
causal tree with several contributing causes (Allspaw, "The Infinite Hows", 2014 *(not re-read for this note)*). Our
lessons already read like small postmortems. Make them uniform and countable: each fix lesson gets **Symptom, First
hypothesis and what refuted it, Root cause, Contributing causes, Why the logs did or did not show it, Class**, where Class
is a short slug (`wait-on-time`, `leaked-test-state`, `reused-os-name`, `platform-drift`, `copy-drift`, …). Two lessons
with the same class trigger the recurring-bug ADR (rule 40 / ER-21) without anyone having to remember.

### 1.16 Record and replay

- **rr** (MIT, with a GPL-2.0 helper in `third-party/zen-pmu-workaround`; github.com/rr-debugger/rr): Linux kernel ≥ 4.7,
  Intel Nehalem+, some AMD Zen, some AArch64 including Apple M-series; needs hardware performance counters, which some
  VMs virtualise. Fits a native heisenbug in a Linux build. It does not run on macOS, and whether the Linux VMs on the Mac
  expose the counters is untested (worth one experiment). Pernosco is a commercial service on rr traces: excluded
  (uploads traces).
- **Windows TTD** (free Microsoft tool, not open source; learn.microsoft.com, updated 2025-11-05): user-mode only, needs
  elevation, about 10–20× slowdown, traces of several GB in minutes, and the trace "may contain personally identifiable
  … information". `TTD.exe` records without WinDbg, for automation. Local Windows VM only, never CI and never a user's
  machine. Electron-style apps can deadlock under it, which may apply to WebView2 *(not tested)*.

### 1.17 Delta debugging of inputs

ddmin reduces a failing input to a 1-minimal one (Zeller and Hildebrandt 2002: an 896-line HTML input to `<SELECT>` in 57
tests). Our fuzz jobs already minimise crashing inputs; the remaining use is order (1.6) and change sets (1.9).

## 2. Adoption plan

### Now (cheap; fits the current releases because it is skill text, scripts and one shared-crate addition)

| Item | Maps to | New? |
|---|---|---|
| N1. Bug-fix skill: evidence step, hypothesis table, repeat counts from the table, "confirmed only after a run without it" (diff in §3) | bug-fix skill, rules 5 and 56, ER-11/12 | Skill steps |
| N2. CI-triage skill: evidence bundle, last-green diff, classification line, flaky-is-a-bug with stress count (diff in §3) | ci-triage skill, rules 4 and 50 | Skill steps |
| N3. Playwright retry settings made consistent with rule 5 (D2) in the five app configs | rule 5, ER-12; `apps/*/app/playwright.config.ts` | Config fix |
| N4. `scripts/bisect-flaky.sh` and `scripts/find-polluter.mjs` | rules 3 and 49 | New scripts |
| N5. Land `run-native.mjs` (on `ci/setup-tools-hardening`) and use it for every native CI tool step | rule 56 CI half | Existing branch |
| N6. Lesson template with Class slug; tag the last 20 lessons | rule 8 (lessons), rule 40 | Doc format |
| N7. Event-loop interval watchdog in `@decryp7/paddock-e2e` and the Node CI scripts that run long | rule 56 (harnesses) | Shared helper |

### After the releases (one shared change each; rule 1 freezes shared code during a release)

| Item | Maps to | New? |
|---|---|---|
| A1. Phase spans: `on_close` lines with `Phase`, `ElapsedMs`, `Outcome` in `paddock-log`; a `Capture` test sink; ids carried over IPC | `paddock-log`, `paddock-ipc`, rule 56 | Crate feature; conformance check later |
| A2. Flight recorder ring in `paddock-log`, dumped on error, panic, stall and report | `paddock-log`, `paddock-diag`, `paddock-proc::stall` | Crate feature + setting |
| A3. Evidence bundle command for CI and local runs (`paddock-dev evidence` or `scripts/evidence.mjs`), reusing `paddock-diag` sections | `paddock-diag`, `tools/paddock-dev`, `scripts/ci-timings.mjs` | New tool |
| A4. Job environment fingerprint and `scripts/last-green.mjs` | `.gitea/actions`, ER-26 | New script; CI step |
| A5. NTSTATUS and signal names in `paddock-proc` spawn errors | `paddock-proc`, rule 53 wrappers | Crate change |
| A6. Nightly shuffled Vitest run with printed seed; flake history from JUnit | `nightly.yml`, rule 49, rule 50 | CI job |
| A7. Shared model explorer in `paddock-testkit` (from Dyno ADR 0025) | `paddock-testkit`, rule 40 | Crate feature |
| A8. Conformance check: a spawn, connect or job without a phase span (start with the shell's start path, C50/C51 neighbours) | conformance, rule 56 | New check (C-number from the integrator) |

### Later

| Item | Maps to | New? |
|---|---|---|
| L1. Native minidumps via `crash-handler` + `minidumper`, written by the existing supervisor; opt-in in Report a problem; macOS `.ips` and existing WER dumps as a `paddock-diag` section | `paddock-log`, `paddock-diag`, `paddock-service` | ADR (privacy, Windows Arm gap) |
| L2. `shuttle` tests for `paddock-jobs` pools and `paddock-download` work stealing | rule 13/14 code | Dev-dependency |
| L3. rr experiment in a Linux VM on the Mac; TTD on the local Windows VM, documented as a manual tool | local VMs (never CI) | Docs |
| L4. Job summaries once Gitea ≥ 1.27 and a matching runner are confirmed | rule 50 | CI |
| L5. Flake score per test over time and a short dashboard page | nightly | Tool |
| L6. `turmoil` for tokio networking, if a networking race recurs | Debrief, Pitwall, Spotter | Dev-dependency |

**Rules.** No new rule is needed for N1–N7: they implement rules 5, 49 and 56. Two candidate amendments for the user:
- Rule 5 (or rule 56): "A cause is written down as confirmed only after a run without it; a hypothesis list with the
  measurement that refuted each rejected cause goes in the fix's lesson or commit body."
- Rule 49 / ER-12 addition: "Every bundle with shared state between specs has a nightly shuffled run with a printed seed."

## 3. Proposed skill changes (diff-ready, not applied)

The skills live in `engineering-rules/skills/` (symlinked into `~/.claude/skills/`). The bug-fix skill cites "ER-27" for
"logs explain every failure"; RULES.md ends at ER-26 today, so either that rule is pending in engineering-rules or the
reference should be Paddock rule 56. The diffs keep ER-27 as written.

```diff
--- a/skills/bug-fix/SKILL.md
+++ b/skills/bug-fix/SKILL.md
@@ ## Steps
-0. **Trace the flow from the logs** ([ER-27](~/engineering-rules/RULES.md)): read the sequence of phases that led to the bug and follow the code for each step. If the logs cannot show the flow, add the logging first and reproduce again. Then reproduce with a failing test (steps 1-2) before any fix; a fix based on a guessed flow is not done.
+0. **Collect the evidence before reading code.** Gather what the failing environment already wrote: the failing step's
+   error lines, the app and agent logs around the failure (same `OperationId`), crash markers and OS crash reports, test
+   traces and artifacts, and what differs from the last green run (commits, OS build, toolchain, runner). Use the
+   project's evidence command when it has one. Check that a local reproduction can take the same code path (architecture,
+   features, sandbox) before building one.
+0a. **Trace the flow from the logs** ([ER-27](~/engineering-rules/RULES.md)): read the sequence of phases that led to the
+   bug and follow the code for each step. If the logs cannot show the flow, add the logging first and reproduce again.
+   Then reproduce with a failing test (steps 1-2) before any fix; a fix based on a guessed flow is not done.
+0b. **Write the hypothesis list.** Every plausible cause, and for each the observation that would rule it out. Make the
+   cheapest such observation in the failing run, not in a new setup. "Flake", "load" and "timing" are symptoms, not
+   causes. Keep the list; it goes in the commit body or lesson with what refuted each rejected cause.
 1. **Reproduce.** Get the smallest input or sequence that fails, on the platform where it fails. If you cannot reproduce it,
-   say so and gather evidence (logs, platform, load) rather than guessing a fix.
+   say so and gather evidence (logs, platform, load) rather than guessing a fix. For a rare failure, raise its rate first
+   (gate the step the race needs, run in a shuffled order, one core, other builds running) instead of looping blindly.
+   For a regression with a known good commit, bisect: `git bisect run` (exit 125 to skip unbuildable commits,
+   `--first-parent` across merge batches), with the test repeated n times per step for a flaky failure.
 2. **Write the failing regression test first** and watch it fail for the right reason (the message names the bug, not a typo).
-   Commit it with the fix, not after.
+   Its failure message must match the reported symptom. Keep the failing output for the hand-off. Commit it with the fix,
+   not after.
 3. **Find the root cause,** not the nearest line that makes the test pass. Ask: what invariant was violated, and where else could
    the same thing happen? If the answer is "a whole class of places", stop and use [recurring-bug-adr](~/engineering-rules/skills/recurring-bug-adr/SKILL.md).
+   A cause is confirmed only when a run without it stays green (remove or guard the cause, run the same reproduction).
@@
-5. **Stress the test.** Run the new test (and any test you touched) many times, 20-50, and under load (other builds running,
-   or CPU-bound neighbours, or a single core). A fix that passes once proves nothing for a race. In a loop:
-   `for i in $(seq 50); do <test cmd> || break; done` or the runner's repeat option.
+5. **Stress the test.** Run the new test (and any test you touched) n times in a row, under load (other builds running,
+   CPU-bound neighbours, or a single core). Choose n from the failure rate p you measured before the fix: n =
+   ceil(ln(0.05)/ln(1-p)) for 95 % confidence (p 50 %: 5, 20 %: 14, 10 %: 29, 5 %: 59, 2 %: 149). If p is lower than 2 %,
+   raise it (step 1) before counting. Use the runner's repeat option (`cargo nextest run --stress-count=N`,
+   `playwright test --repeat-each=N`) or `for i in $(seq N); do <test cmd> || break; done`. Report N and p.
-6. **Check the logs would have shown the cause** ([ER-27](~/engineering-rules/RULES.md)); if not, add the missing lines (and a test for them where practical).
+6. **Check the logs would have shown the cause** ([ER-27](~/engineering-rules/RULES.md)); if not, add the missing lines
+   (phase, duration, outcome, correlation id, raw exit code) and a test that asserts them where practical.
@@
-8. **Commit** with Conventional Commits `fix: ...`, no attribution. Add a lesson if it generalises.
+8. **Commit** with Conventional Commits `fix: ...`, no attribution. Add a lesson if it generalises, in the project's lesson
+   shape (symptom, first hypothesis and what refuted it, root cause, contributing causes, why the logs did or did not show
+   it, class). If another lesson has the same class, use recurring-bug-adr.

 ## Flaky tests: find the race
 - Ask what the test assumes about order, time, ports, files, locale, or a background task having finished.
+- Order: if it fails only in a full run, it may be a victim of an earlier test. Rerun with the failing run's order or seed
+  (`--sequence.seed`), then bisect the files that ran before it in the same worker until one polluter remains.
+- Harness: check that the harness itself did not stall (a synchronous call on its event loop, a blocked runtime thread).
+  A watchdog line or a profile settles it; a slow product and a stalled harness look the same.
 - Typical causes: waiting on a clock instead of a condition; a fake that behaves differently per OS (inherited socket flags,
   path separators, case-insensitive file systems); shared temp folders or ports between tests; a test that compares speed.
@@ ## Pitfalls
+- Fixing the leading hypothesis without the measurement that would refute it.
+- A reproduction that cannot take the failing code path (another CPU architecture, a feature or sandbox switched off).
```

```diff
--- a/skills/ci-triage/SKILL.md
+++ b/skills/ci-triage/SKILL.md
@@ ## Steps
 1. **Delegate the watching to the `scout` (Haiku)** ([roles](~/engineering-rules/prompts/spawn-by-role.md)). Brief it: run id or branch, wait
    until finished, report every job's result and, for failed jobs only, the failing step name and its error lines (not the log).
-   Read-only: it must not re-run, push or cancel anything. Never ask it whether a failure is real.
+   Also: the tests that were retried or reported flaky, the artifacts the failed jobs kept, the runner name, and the last
+   green run of each failed job (its run id and commit). If the project has an evidence or last-green command, the scout
+   runs it and reports the folder path and the differences it printed (OS build, toolchain, runner, commit range).
+   Read-only: it must not re-run, push or cancel anything. Never ask it whether a failure is real.
 2. **Group the failures by root cause.** Ten red jobs are often one broken step (a missing tool, a stale generated file, one
-   test). Write the groups down: cause, jobs affected, first error line.
+   test). Write the groups down: cause, jobs affected, first error line, and the class (infrastructure, tool crash with its
+   raw exit code, build, lint or type, test, timeout).
 3. **Classify each group yourself or hand the evidence to the `researcher`** (never the `implementer` or `scout`: telling a real
    failure from a flake needs judgement, and an unclear root cause is the researcher's job):
    - *Real failure*: reproduces locally, or the diff plausibly causes it.
    - *Inherited*: main was already red at the base commit; check the previous run of the base.
+   - *Environment drift*: nothing in the commit range explains it, and the last-green diff shows an OS update, a new
+     toolchain or a different runner. Check the runner's OS updates first ([ER-26](~/engineering-rules/RULES.md)); make the
+     product or tool print the raw value before guessing.
    - *Flake*: passes on re-run, differs by platform, timing or load. It is still a bug: use the [bug-fix](~/engineering-rules/skills/bug-fix/SKILL.md)
      skill and make it deterministic. A re-run is evidence, never the fix. Never add retries or skips.
    - *Infrastructure*: runner offline, cache server error, network. Say so; re-run once, report it.
+   Read the evidence the job already kept (artifacts, browser and app logs) before building a local reproduction, and
+   check that the local machine can take the same code path.
 4. **Reproduce and fix locally.** Re-run only the failed tests/steps on your machine, in the same configuration CI uses
-   (platform, features, release vs dev profile). Lint and type-check the touched files.
+   (platform, features, release vs dev profile). For a flake, measure its local failure rate with a repeat count and
+   stress the fix as the bug-fix skill says. Lint and type-check the touched files.
@@
 6. **Record the lesson** if it taught something new ([ER-6](~/engineering-rules/RULES.md)); if the same class of failure keeps coming
    back, run [recurring-bug-adr](~/engineering-rules/skills/recurring-bug-adr/SKILL.md).
+7. **Close the log gap.** If the job's own output could not name the cause (an exit code without its meaning, a step
+   without its phase), the fix also makes the step print one `::error::` line with the cause and the next step.

 ## Pitfalls
+- Four guesses at a CI-only failure while the answer sits in the artifacts the job already kept.
+- A local reproduction on a machine that cannot take the failing path (Arm vs x64, sandbox, feature flags).
```

## 4. Licences, fitness and what is local

All are tools or dev-dependencies unless marked; none sends data unless stated. Versions from `cargo info` on
2026-10-31.

| Tool | Licence | Ships in apps? | Platforms | Local / offline | Verdict |
|---|---|---|---|---|---|
| tracing-subscriber 0.3.23 | MIT | Yes (already) | All | Local | Use (already) |
| tracing-chrome 0.7.2 | MIT | Dev only | All | Local file | Allowed |
| tokio-console 0.1.14 / console-subscriber 0.5.0 | MIT | Dev builds only | All | Local socket | Allowed, dev only |
| crash-handler 0.8.1, minidumper 0.11.0 | MIT OR Apache-2.0 | Yes (later) | Win x64, macOS x64/arm64, Linux | Local files | Allowed; Windows Arm gap |
| minidump-writer 0.13.0, minidump 0.27.0 | MIT | Yes (later) | As above | Local | Allowed |
| proptest 1.12.0 | MIT OR Apache-2.0 | Dev | All | Local | Use (already) |
| shuttle 0.9.9 | Apache-2.0 | Dev | All | Local | Allowed |
| loom 0.7.2 | MIT | Dev | All | Local | Allowed |
| turmoil 0.7.x | MIT | Dev | All | Local | Allowed, later |
| cargo-nextest (stress ≥ 0.9.103, flaky-result ≥ 0.9.131) | Apache-2.0 OR MIT *(from memory)* | Tool | All | Local | Use (already) |
| cargo-mutants 27.1.0 | MIT | Tool | All | Local | Use (already, rule 5) |
| samply 0.13.1 | MIT OR Apache-2.0 | Tool | All | Records locally; viewer is a web page | Allowed; open profiles offline only if the viewer is self-hosted |
| Playwright, Vitest | Apache-2.0, MIT *(from memory)* | Dev | All | Local | Use (already) |
| fast-check | MIT *(unverified)* | Dev | All | Local | Allowed if needed |
| rr | MIT (+ GPL-2.0 helper in third-party, a separate program) | Tool, local Linux VM | Linux only | Local | Allowed as a tool; never linked |
| Windows TTD, WinDbg, WPR/WPA, Instruments | Microsoft / Apple proprietary, free | Tool, local only | Windows / macOS | Local | Allowed as tools; never in CI or on users' machines |
| Pernosco, hosted crash services | Commercial / hosted | — | — | Uploads traces | **Excluded** (local-first principle) |

Everything recommended runs offline. The only network touch is samply's default viewer page and installing tools, both on
a developer machine. Crash dumps and flight-recorder files can hold user content, so they stay local and enter a report
only by the user's choice (paddock-diag's preview already shows every file).

## 5. Decisions for the user

- **D1.** Adopt the "now" tier (skill diffs in §3, two scripts, lesson class slugs) without a new rule? Recommended: yes.
- **D2.** Playwright retries in five app configs contradict rule 5. Choose (a) retries 0 everywhere, or (b) one retry
  with `failOnFlakyTests` (and nextest `flaky-result = "fail"`), red either way but with a second trace. Recommended: (b).
- **D3.** Phase spans and the flight recorder in `paddock-log` after the current release (shared code, rule 1)?
- **D4.** Native minidumps: build them (later tier, needs an ADR) and include them in Report a problem only when the
  user ticks them? Windows on Arm has no support in the Embark crates.
- **D5.** Two rule amendments (§2): "confirmed only after a run without it" with a recorded hypothesis list; and a
  nightly shuffled run for shared-state spec bundles.
- **D6.** Which Gitea and act_runner versions run CI? Job summaries need Gitea ≥ 1.27 and a runner with the upload.
- **D7.** The bug-fix skill cites ER-27, which RULES.md does not have yet; add ER-27 to engineering-rules or cite
  Paddock rule 56.

## Sources (read 2026-10-31 unless noted)

- nextest stress tests: https://nexte.st/docs/features/stress-tests/ ; retries and flaky tests: https://nexte.st/docs/features/retries/
- Playwright CLI: https://playwright.dev/docs/test-cli
- Vitest sequence options: https://vitest.dev/config/sequence
- Node.js perf_hooks: https://nodejs.org/api/perf_hooks.html
- git bisect: https://git-scm.com/docs/git-bisect
- Gitea job summaries: https://github.com/go-gitea/gitea/issues/23721
- Embark crash handling: https://github.com/EmbarkStudios/crash-handling
- Windows WER LocalDumps: https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps (updated 2025-03-11)
- Windows TTD: https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/time-travel-debugging-overview (2025-11-05)
- rr: https://github.com/rr-debugger/rr and its LICENSE
- shuttle: https://github.com/awslabs/shuttle ; turmoil: https://github.com/tokio-rs/turmoil
- samply: https://github.com/mstange/samply
- tracing-subscriber FmtSpan: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.FmtSpan.html
- Google SRE book, Postmortem culture: https://sre.google/sre-book/postmortem-culture/
- J. Micco, Flaky Tests at Google and How We Mitigate Them (2016-05-27): https://testing.googleblog.com/2016/05/
- Meta, Probabilistic Flakiness (2020-12-10): https://engineering.fb.com/2020/12/10/developer-tools/probabilistic-flakiness/
- Shi et al., iFixFlakies (ESEC/FSE 2019); iDFlakies: https://github.com/UT-SE-Research/iDFlakies
- Zeller and Hildebrandt, Simplifying and Isolating Failure-Inducing Input (IEEE TSE 28(2), 2002): https://www.st.cs.uni-saarland.de/papers/tse2002/
- Xia et al., Agentless (arXiv 2407.01489, v2 2024-10-29): https://arxiv.org/abs/2407.01489
- Mündler et al., SWT-Bench (NeurIPS 2024, arXiv 2406.12952), via secondary summaries
- Claude Code best practices: https://code.claude.com/docs/en/best-practices
- *(Not re-read, cited from memory, marked in the text)*: W3C Trace Context https://www.w3.org/TR/trace-context/ ;
  Apple crash report docs https://developer.apple.com/documentation/xcode/acquiring-crash-reports-and-diagnostic-logs ;
  J. Allspaw, The Infinite Hows (2014).

## Measurement record

Event-loop watchdog, synthetic, no user data: Node v24.22.0 on the Mac mini (macOS), 2026-10-31, script in the agent's
scratchpad (not committed). `monitorEventLoopDelay({ resolution: 10 })`, enabled and given 50 ms before the block: max
10–12 ms for a 300 ms `spawnSync('sleep')` and a 300 ms busy loop (5 runs). A 10 ms `setInterval` recording its own
lateness: 296–306 ms (4 runs). Not a benchmark of the product, so not entered in docs/benchmarks.md (rule 48 covers
product numbers quoted elsewhere; add it there if the integrator disagrees).
