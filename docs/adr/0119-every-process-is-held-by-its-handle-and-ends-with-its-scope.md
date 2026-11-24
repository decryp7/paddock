# 0119. Every process is held by its handle and ends with its scope: start, hold, watch, end for tests, tools and agents

- Status: Accepted 2026-11-23 (user, relayed by the coordinator). D1 = B, the complete option (now 11.75 days with D5); D2 =
  keep 0112, 0118 and 0119 separate; D3 = the blocking agent hook; D4 = measure in WP-9, then (a) or (b) as laid out; D5 = ban
  `output()` and `status()` too, so every child runs through a `Scope` with a bound (WP-14 to WP-16 added, about +3 days); D6 = the
  order as recommended; D7 = the rule is number 97 (91 to 96 are taken by the rules batch on `docs/rules-from-memory-2026-11-23`).
  Proposed earlier the same day (ab8faf338); the direction was the user's (approved 2026-11-23, moved earlier in the queue).
- Date: 2026-11-23
- Related: **ADR 0118** (in design on `feat/test-results-artifact`: one CI wrapper, `./dev ci run` / `./dev ci step`, that every
  step and command runs through, and one job record per job; the user's alignment of 2026-11-23 makes it CI's process owner, §7),
  **ER-51** (start, hold, watch, end), ER-32 (a real completion signal, always bounded), ER-14 (tests leave nothing
  behind), ER-21 / rule 40 (a recurring class gets a redesign), ER-22 / rule 42 (share first), ER-27 (logs explain every failure),
  ER-33 (positive controls), ER-37 / rule 76 (common practice and cost), ER-41 (new checks report-only first), ER-46 (lists only
  shrink); rule 53 and ADR 0033 (processes by identity), rule 89 (every test run goes through the bounded runner), ADR 0031 (waits
  judged on progress, not wall clocks), ADR 0076 (owned background work; the orphan guardian), ADR 0103 (the test `Reaper`),
  **ADR 0112** (every CI job is a fixture that owns its processes; accepted 2026-11-21, not on main).
- Number: 0119. The brief asked for 0112 if free. It is not: `docs/adr/0112-job-owns-its-processes.md` is on
  `origin/docs/adr-job-owns-processes` (4a06ef26b, Accepted 2026-11-21) and on `origin/docs/adr-audit-2026-11-22` (02f8be6d1, the
  copy to main). Searched on 2026-11-23 in every `origin` branch (`git ls-tree` of `docs/adr/` and `git grep "ADR 01xx"` on each)
  and in every local worktree under `<dev-targets>/wt`: files 0109 to 0117 are taken, and **0118 is cited** (no file yet) by the
  uncommitted job-record work in the worktree of `feat/test-results-artifact` (`tools/paddock-dev/src/test_results.rs`: "One
  structured test-results file per CI test job (ADR 0118)"); 0119 and 0120 appear nowhere. This ADR **builds on 0112 and 0118**:
  0112 owns the CI job as a fixture (its job action and post-step net, its folder and log bundle), 0118 owns the one CI wrapper and
  its job record (§7); this ADR owns the one in-process mechanism every tool, test, agent and that wrapper uses, the checks, and the
  migration.

Evidence heads read (ADR 0091), re-read right before the commit: `origin/main` d04af7b46, `origin/train/2026-11-23-graph`
64e01dd84 (this branch's base), `origin/fix/proc-tree-pid-reuse` 682208725, `origin/docs/adr-job-owns-processes` 4a06ef26b,
`origin/docs/adr-audit-2026-11-22` 02f8be6d1, `origin/feat/lessons-check-and-job-cleanup` 9dfa4e6dc, and the **local-only** branch
`feat/adr0112-wp1-owner-trait` b9970817d (31 commits that build most of ADR 0112; `git ls-remote origin` has no such branch on
2026-11-23, so nothing on it is on any train). Shipped code: `pitcrew-service`'s `launch::graceful_stop(pid: u32, ..)` and
`kill_tree(pid: u32)` signal a bare pid's group (`killpg`) at the tag `pitcrew-v1.0.1` (read at the tag,
`apps/pitcrew/crates/pitcrew-service/src/launch.rs:253` and `:294`). `git tag --contains 682208725` is empty: the pid-reuse fix
ships with no app yet.

## Context: the class

The class is "something ends but does not end what it started", the class of ADR 0112, now outside CI jobs: a test, a tool, an
agent's shell. ADR 0112 handles the job; inside the job, and on the developer machine where there is no job, each starter still
decides by itself whether it keeps a handle.

### Incidents (each checked)

| When | What | Evidence (checked 2026-11-23) |
| --- | --- | --- |
| 2026-11-14 | A VM lock test failed partway and left a detached `slot hold`; it failed nine tests after it. | run 8099, job 38394 "VM tooling tests (macOS)": failed outside any test (`./dev ci triage 8099`); lesson "Tests summed their steps" |
| 2026-11-23 | Pitcrew's supervision harness ended other jobs' processes on the shared Windows host: within 8.5 s a Dyno test, a Pitcrew test, the crate job's nextest and Pitcrew's nextest ended with exit 1 and no output. It followed parent pids without a start-time check and ended each pid through `sysinfo::Process::kill` (`taskkill /PID /F`); a file-wide `#![allow(clippy::disallowed_methods)]` hid the ADR 0033 ban. | run 8674: `app pitcrew (windows-latest)` "a nextest run ended without its summary", `crate (windows-latest)` failed outside any test (`./dev ci triage 8674`); cause and fix in 682208725 (`origin/fix/proc-tree-pid-reuse`, identified, not reproduced) |

### Reports (not counted as incidents: no run or commit shows them)

| When | What | Source |
| --- | --- | --- |
| 2026-11-22/23 | Test-spawned `ci slot hold --max 3` processes were orphaned (ppid 1) after timed-out test runs, four times or more. | docs/lessons.md (train), "Long test hangs came from plain `cargo test`"; rule 89. No run: they were local runs. |
| 2026-11-23 | Agents started processes with a plain `&` and could not end them later (pids 17976, 20391). | the coordinator's brief |
| 2026-11-03/04, again 2026-11-23 | Background waiters stacked on one event. | ER-32 (36 waiters, coordinator log 2026-10-29-overnight.md); the coordinator's brief |
| 2026-11-22 | 2.2 TiB of worktrees and build folders nobody removed; some build folders inside a checkout. | ER-51 |

### Why each happened, mapped to the gap

1. **A detached start has no declared end.** `paddock_proc::spawn_detached` makes a child that outlives its starter on purpose
   (the slot holder must survive its step). Its only bound today is "the owner folder went" plus a 3.5 h cap
   (`tools/paddock-dev/src/slots.rs`, `DEFAULT_MAX_HOLD_SECS`). A test's owner folder is a `TempDir`, which a killed test never
   drops, so its holder lives for hours. nextest ends a timed-out test's process group (Unix) or job object (Windows) (sources
   below), but the holder leads its own group on purpose, so it escapes. The fix "the holder also ends when its starter's
   `ProcessId` has ended" exists (ADR 0112 WP-7, d31c74d94) **only on the local branch**.
2. **A process start returns a `std::process::Child`, which does nothing on drop.** Clippy bans `Command::new` outside
   `paddock_proc::command` (`lint/clippy.toml`), so every start is built by paddock-proc, but `.spawn()` then hands the caller a
   plain `Child`. Whether that child is held is up to each caller: on the train there are 164 `.spawn()` sites in 94 Rust files
   (crates 65, tools 27, apps 72 sites; 55 of them in tests). C126 rule C flags the test ones (report-only, 36 sites); nothing
   flags the others.
3. **Bare-pid ends survive behind allows.** C53 flags a blanket `#![allow(clippy::disallowed_methods)]`, but seven files carry one
   under dated C53 waivers to 2026-12-31 (`paddock.waivers.toml`: `supervision.rs`, `fake_runner.rs` and five
   `tools/paddock-dev/tests/it/*.rs`). The run 8674 harness was one of them. `graceful_stop(pid: u32, ..)` still signals a bare
   pid's group after the fix branch (`origin/fix/proc-tree-pid-reuse`, `launch.rs:268`, under `#[expect(.., reason = "ADR 0033
   WP-6 ..")]`), and it shipped in `pitcrew-v1.0.1`.
4. **An agent's shell has no owner.** A Bash call that ends with `&` leaves a process whose `$!` dies with that shell. The
   kill guard (`.claude/hooks/kill-guard.py`, blocking) then refuses a later `kill <pid>` outside the agent's worktree, correctly
   (rule 53), so nobody can end it. The guard's own next step already says "start a long process with the Bash tool's
   run_in_background so the TaskStop tool can end it"; nothing steers the start.
5. **A wait can be re-made but not re-attached.** `./dev wait --pid <pid>` reads the start time when the wait begins
   (`tools/paddock-dev/src/wait.rs`, the `--pid` probe), so a wait started after the pid was handed on waits on a stranger.
   The waiter guard (`.claude/hooks/no-pgrep-wait.py`) only warns about a second wait on one target.
6. **A unit's folders have no owner record that removal reads.** `./dev disk` cleans only folders with a `.paddock-owner.toml`
   record (`tools/paddock-dev/src/disk/mod.rs`); build folders an agent makes by hand have none.

### What exists, and stays the one implementation (rule 55)

| Mechanism | What it does | Where |
| --- | --- | --- |
| `ProcessId` (pid plus start time), `holder`, `presence` | identity; a pid handed on is "not ours" | `crates/paddock-proc/src/identity.rs`, origin/main |
| `kill_tree`, `end`, `ask`, `terminate` | end by identity; the only signal site is `sys.rs` (C53's `IMPLEMENTATION`) | `crates/paddock-proc/src/tree.rs`, origin/main |
| `Supervised`, `Registry`, `end_all`, `Session` | one held child whose tree ends on drop; all at shutdown | `crates/paddock-proc/src/supervised.rs`, `session.rs`, origin/main |
| The orphan guardian | when the owner dies by SIGKILL or a crash, its guarded children's groups end (Unix); Windows uses kill-on-close jobs | `crates/paddock-proc/src/guardian.rs`, ADR 0076, origin/main |
| `own::look`, `own_group`, `KillOnClose`, child subreaper | what a process can prove it started | `crates/paddock-proc/src/own.rs`, origin/main |
| `run_bounded`, `run_waited*` | run to completion with a bound and an output cap | `crates/paddock-proc/src/run.rs`, `waited.rs`, origin/main |
| `Owned` (wait with a bound, result, end), `own::Unit`, `members` | the owner trait and the pure membership rule | `crates/paddock-proc/src/owner.rs`, **local only** (d481d65d5) |
| Slot holder bound to its starter | `acquire --starter-pid`; the holder ends when the starter has ended | d31c74d94, **local only** |
| The job action, ledger, log bundle | a CI job is a fixture | ADR 0112 WP-3 to WP-16, **local only** |
| `Reaper` (Rust and Node) | a test holds what it spawned | `crates/paddock-testkit-start/src/reap.rs`, `scripts/test-kit.mjs`, origin/main |
| C53 (bare-pid kills), C126 rule C (unguarded spawn in tests) | checks | `tools/paddock-dev/src/check/c53.rs` (in force), `conformance/src/checks/C126-test-steps.mjs` (report-only) |
| C159 (refuse ADR 0033 entries under a file-wide allow) | the approved file-wide kill-lint ban | id reserved for `feat/conformance-guards-2026-11-23-kill-allow` (`refs/paddock/check-ids/C159`); the branch is not on origin, a prerequisite |
| Kill guard, waiter guard | agent hooks (block / warn) | `.claude/hooks/kill-guard.py`, `no-pgrep-wait.py`, origin/main |

Nothing here is missing by much. What is missing is **one place every start goes through** that returns a held value, and a
check that makes it the only place.

## Decision

### 1. One mechanism: `paddock_proc::Scope`, the fixture in code

A `Scope` is a unit of work's prepare, execute and clean-up in one value, the in-process form of ADR 0112's job fixture. It is
built on what exists, not beside it:

- **Prepare** (`Scope::new(name)`): makes the unit's folder (under the caller's temp root, or a folder the caller names), an
  `own::Unit` with the scope's start time, and the means that end everything later: on Unix the orphan guardian registration
  (ADR 0076) and a process group per child; on Windows one kill-on-close job (`own::KillOnClose`) that every child of the scope
  joins (nested jobs, Windows 8 and later, so it works inside the runner's job). Children get the folder as their working folder
  and `TMPDIR`/`TMP`/`TEMP` unless the caller says otherwise (ER-51's one folder).
- **Execute**: `scope.start(cmd) -> Held` and `scope.run(cmd, limits) -> Output` (the existing `run_bounded`). `Held` keeps the
  `Child` and its `ProcessId`, records the start in the unit, and offers `Owned`: `wait(bound)`, `result()`, `end()`.
  Amended 2026-11-24 (WP-1 review, P2-4): `wait` takes the `Owned` trait's bound (ADR 0112), as `Owned` is defined, and ends at
  the child's exit, a cancel or the bound. A held child is long-lived and has no progress signal of its own to judge; a start
  that needs judging on progress (ADR 0031) runs `StartWatch`/`Patience` on the child's identity (`Held::process`), and the
  bound of a run to completion is `Scope::run`'s `Limits` (WP-14 makes it progress-judged). A cancel token (`scope.cancel()`) ends every
  wait in the scope with `Waited::Cancelled`.
- **Clean-up** (`Drop`, also `scope.finish()` for the outcome): asks each live member to stop, waits the scope's grace (one named
  constant, measured in WP-1 before it is chosen, ER-12), then ends the rest by identity through `kill_tree`; removes the folder
  unless it holds declared outputs; logs one summary line per member (pid, start time, program, how it ended, ER-27).

| How the scope ends | What ends its processes |
| --- | --- |
| Normally, an error, `?` | `Drop` |
| A panic that unwinds | `Drop` during unwinding |
| A panic with `panic = "abort"`, SIGKILL, a crash, OOM | Unix: the guardian (its pipe closes); Windows: the job handle closes (kill-on-close) |
| A cancel (`scope.cancel()`, a tool's Ctrl+C handler) | the cancel ends the waits; `Drop` ends the processes |
| A test timeout | nextest ends the test's group or job; children of the scope are in it or in the guardian's care; detached ones by their lifetime (2) |
| A CI step's command fails, times out or is cancelled | the CI wrapper's scope (§7), which lists what it ended in the job record |
| A CI job cancel or timeout past the wrapper | the job action's post step (ADR 0112), then the runner |

`Supervised` stays for one long-lived child with a memory cap; it becomes what `Scope::start` returns when a cap is asked for
(one implementation, rule 55). The test `Reaper` becomes a `Scope` in test mode (it already holds an `own::Unit` on the local
branch). Tokio: `Scope` offers `start_async` only where a caller needs it (YAGNI); it holds tokio's `Child` the same way.

### 2. Outliving the scope needs a declared lifetime

`spawn_detached` becomes private to paddock-proc. The one public way to start something that outlives its starter is
`scope.start_detached(cmd, lifetime)` where `lifetime` is required:

- `Lifetime::Starter(ProcessId)`: ends when that process has ended (the slot holder's `--starter-pid`, d31c74d94, generalised);
- `Lifetime::Folder(path)`: ends when the folder is gone (today's holder rule, for a CI job's folder that the runner removes);
- `Lifetime::Max(duration)`: always present as the last bound (today's 3.5 h cap).

The detached child carries the check itself (it is our own binary: the slot holder, a test stand-in), so there is no supervisor
process. A program we do not own cannot be started detached; it is started held. The handle of a detached start is still
recorded (3), so `./dev held end` can end it early.

### 3. Re-attachable handles

- A handle has one text form, `<pid>:<start µs>` (`ProcessId`'s `Display` and `FromStr`), the same on every OS.
- A scope that is asked to (`Scope::with_ledger`) appends one JSON line per start to `<unit folder>/held.jsonl`: handle, program
  with secrets masked (paddock-log's `Redactor`), working folder, lifetime, start time. This is the same line shape as ADR 0112's
  job ledger, so a CI job's ledger and an agent's are read by one reader.
- `./dev wait --pid <pid>:<start>` accepts the handle and waits on exactly that process (a bare pid still works, as today).
- `./dev held list|wait|end --unit <folder>` reads a ledger and acts by identity after the table proves membership (rule 53: the
  ledger is a hint, the live identity decides).

### 4. The checks (the CI wrapper is §7)

1. **Clippy bans every unheld or unbounded start and wait** outside paddock-proc (D5, accepted): `std::process::Command::spawn`,
   `Command::output`, `Command::status`, `Child::wait` and `Child::wait_with_output` (and tokio's, `allow-invalid`), with the reason
   "start through `paddock_proc::Scope` with a bound (ADR 0119)". `Scope::run(cmd, limits)` requires a bound (`Limits`, judged on
   progress, ADR 0031) and is the one replacement for `output()` and `status()`; `Held::wait(bound)` replaces `Child::wait`. Clippy is exact where a text check is not. A justified site carries
   `#[expect(clippy::disallowed_methods, reason = "ADR 0119: ...")]`; the list of such sites is generated and only shrinks
   (ER-46). Because a Clippy ban blocks at once, it lands **with** the migration (6), not before: that is how ER-41's
   "report-only first" is met for a compiler lint (the migration's count is the report).
2. **No file-wide allow of `disallowed_methods`**: the approved kill-lint ban, C159, built separately
   (`feat/conformance-guards-2026-11-23-kill-allow`). This ADR depends on it and removes the seven C53 waivers in its migration;
   a file-wide allow would hide check 1 exactly as it hid the bare-pid kill in run 8674.
3. **Scripts and workflows**: ADR 0112 WP-6's handle check (a `&` whose `$!` is not saved, `pgrep`, `pkill`, `ps | grep`),
   extended to Node files outside tests: `spawn`/`fork` with `detached: true` and no kit `Reaper`. C126 rule C keeps the tests.
4. **Agents**: a blocking `background-guard` hook (5).

Each check lands with its positive control (ER-33): a fixture with the shape is red, the same fixture through `Scope` is green.

### 5. Agents' background shell work

The agent harness already gives a held background start: the Bash tool's `run_in_background` returns a task id, notifies on
exit, and `TaskStop` ends it. That is the common practice and the cheapest fix. This ADR:

- adds `.claude/hooks/background-guard.py` (on `lib/shellscan.py`, like the kill guard): it refuses a top-level command that ends
  in `&` without a `wait` or a `$!` use in the same command, and `nohup`, `setsid`, `disown`. The message names the next step:
  `run_in_background`, or `./dev run --hold` (below). Blocking, because it guards state, not text (ER-36: the kill guard blocks for
  the same reason). D3 offers warn-only.
- measures first (WP-9) whether the harness ends an agent's background tasks when the agent ends. If it does, the hook is the
  whole agent part (D4 a).
- if it does not, adds `./dev run --hold -- <cmd>`: a `Scope` whose unit folder is the worktree's own
  (`<dev-targets>/held/<worktree name>`, outside the checkout, ER-14), with a ledger; it prints the handle and returns.
  `./dev held list|end` read that ledger, and `./dev worktree remove` and `clean` end a worktree's held processes by identity
  before they remove it (D4 b).

Stacked waiters: the waiter guard keeps its warnings; `./dev wait` with a handle (3) makes a second wait on the same process
visible as the same handle, which the guard then names.

### 6. Migration

By area, one implementer each, in parallel after WP-1 and WP-2 (one package per area, each owning its files):

| Area | Sites (`.spawn()` on the train) | Into |
| --- | --- | --- |
| `crates/` (outside paddock-proc) | 65, of which 26 in tests | `Scope::start` / `run`; tests through the `Reaper` (a `Scope`) |
| `tools/paddock-dev` | 27, of which 18 in tests | the same; the slot holder onto `start_detached(Lifetime::Starter ..)` |
| `apps/` | 72, of which 11 in tests | the same; Pitcrew's runner and tool trees hold `Held` |
| shell (`&` lines: 16 in 10 files; `$!` uses: 34) | ADR 0112 WP-6's check lists them | `./dev run --hold`, or kept with `$!` and a trap |
| file-wide allows | 7 C53 waivers | per-site `#[expect]` with a reason, or removed |

**`graceful_stop` by bare pid** (the follow-up from fix/proc-tree-pid-reuse): paddock-proc gets `ask_group(leader: ProcessId)`:
SIGTERM to the group only while its leader is still the process named (as `kill_tree` signals a group today); on Windows the
Ctrl+Break helper takes the handle text and re-checks the identity before `GenerateConsoleCtrlEvent`. Pitcrew's `RunnerProcess`
gives `id() -> ProcessId` instead of `pid() -> u32`; `graceful_stop` and `kill_tree` take it; the `#[expect(.. "ADR 0033 WP-6")]`
lines go. Failing test first (ER-11): a runner whose pid is handed on to a stand-in; the stop must reach nobody. It ships with the
next Pitcrew tag (pitcrew-v1.0.1 has the bare-pid form).

**Build folders**: `./dev worktree add` writes the owner record (`.paddock-owner.toml`, the disk tool's source of truth) for each
build folder it names; `worktree remove` removes them with the worktree unless locked (ER-51). WP-11 first reads what
`worktree remove` and `disk clean` do today and only adds what is missing.

### 7. CI: the one wrapper is the process owner (user, 2026-11-23)

CI gets no process mechanism of its own here. Every CI step and command runs through **one wrapper**, `./dev ci run` (today's gate
executor, ADR 0038 §1) and a new `./dev ci step` for steps that are not a project's phase, which writes **one job record per job**.
That design is ADR 0118's (being built on `feat/test-results-artifact`, first as the job's test-results file, widened by the user to
a job record). For processes, the wrapper is the owner:

- it opens one `Scope` per job (prepare at the first step, the job's folder as the unit folder) and runs each command with
  `scope.start` or `scope.run`, so it **holds the handles**;
- it **bounds the runs**: each command's bound comes from the declared data the wrapper already reads (`paddock.ci.json`), judged
  on progress (ADR 0031), never a bare wall clock;
- on a failing command, a bound reached, or a cancel (the runner's SIGINT/SIGTERM on Unix, Ctrl+Break on Windows, both turned
  into `scope.cancel()`), it **ends everything** the step started through the scope, before the step returns;
- it **lists the processes it ended** in the job record: handle (`pid:start`), program with secrets masked, step, how it ended
  (asked, ended, already gone, not ours) and what it could not see. The record is the one place a reader looks; no second ledger
  file is written in CI (`held.jsonl` of §3 is for agents and local tools, with the same line shape).

ADR 0112's job action stays as the net under the wrapper, not beside it: its post step ends what escaped every wrapper call (a
`setsid` child, a node action's process, a runner crash) and adds its findings to the same job record. Its `BASH_ENV` group
ledger is then needed only for steps that do not yet run through the wrapper; when the migration of workflows onto `./dev ci step`
is complete, the ledger is a candidate to retire (an amendment to ADR 0112, decided then, not here). The job objects on Windows and
the process groups on Unix are as ADR 0112 reads them from the runner's source; the wrapper's scope adds its own kill-on-close job
(nested) and groups inside them.

## Common practice (ER-37)

| Software or platform | Practice | Source (read 2026-11-23) | Our cost to match |
| --- | --- | --- | --- |
| tokio `JoinSet` | "When the `JoinSet` is dropped, all tasks in the `JoinSet` are immediately aborted"; `shutdown()` aborts and waits. Tasks are held by the set, not found again. | [docs.rs tokio JoinSet](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html) | `Scope` is this shape for processes: 1.5 d (WP-1) |
| Go `os/exec` | `CommandContext`: the context "is used to interrupt the process (by calling cmd.Cancel or os.Process.Kill) if the context becomes done before the command completes"; `WaitDelay` "bounds the time spent waiting" then "shuts down the child process and/or its I/O pipes". | [pkg.go.dev os/exec](https://pkg.go.dev/os/exec) | the cancel token and the grace in WP-1 |
| systemd | `systemd-run --scope`: "the processes of the command are managed by the service manager similarly to normal services"; a unit's default `KillMode=control-group` kills "all remaining processes in the control group" (ADR 0112's table). | [systemd-run](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html), [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html) | Linux only and needs a user manager; macOS has no cgroups. Not adopted: the guardian is our cross-OS form |
| Windows job objects | "Operations performed on a job object affect all processes associated with the job"; children join by default; with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` "closing the last job object handle terminates all associated processes". | [Job Objects, Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) | exists (`KillOnClose`); WP-1 uses one per scope |
| cargo-nextest | A process group per test; on timeout SIGTERM "to the process group", 10 s grace, then SIGKILL; on Windows "job objects to kill the test process and all its descendants". Leaky tests (a child holding stdout or stderr) are detected, and pass unless `result = "fail"`. | [nextest slow tests](https://nexte.st/docs/features/slow-tests/), [leaky tests](https://nexte.st/docs/features/leaky-tests/) | already ours (rule 89); detached children escape it, hence (2) |
| GitHub and Gitea runners | job-end kill by `RUNNER_TRACKING_ID`, per-step groups, a Windows job per job | ADR 0112's table | ADR 0112 |
| Agent harness | `run_in_background` returns a task id; `TaskStop` ends it; exit is notified | the Bash tool's own contract in this session; whether tasks end with their agent is **not verified** (WP-9 measures it) | the hook only: 0.5 d |

What is common: hold the child in a value whose drop or cancel ends it (JoinSet, `CommandContext`, `kill_on_drop`), and a
kernel unit for the tree (cgroup, job object, process group). What goes beyond it here: the ledger and `./dev held` (re-attaching
from another process), declared lifetimes for detached children, and the Clippy ban with a migration of every site. The reason:
our starters are many short-lived tools and agents on shared hosts, so a handle often must be found again by a later process (a
CI step, the coordinator), and a stale pid there ended other jobs' work (run 8674).

## Options and costs

Agent-days, raw.

| Option | What | Days |
| --- | --- | --- |
| A, common practice | Clippy ban on `spawn` and migration of the 164 sites onto the existing `Supervised` / `run_bounded` (kill on drop, guardian); `ask_group` and `graceful_stop` by `ProcessId`; the background-guard hook; rule text. No `Scope`, no ledger, no lifetimes, no `./dev held`. | 4.0 |
| **B (accepted, D1)** | A, plus `Scope` (one folder, one job, cancel, summary), `start_detached` with a required lifetime, handle text, ledger, `./dev wait` on a handle and `./dev held`, the Node and shell checks, build-folder ownership, and the CI wrapper as the process owner (§7), plus the ban on `output()`, `status()` and unbounded waits with its migration (D5) | **11.75** |
| B with D4 (b) | B plus `./dev run --hold` and worktree removal ending held processes | 12.5 |

B (including WP-13, the CI wrapper's scope, §7, and WP-14 to WP-16 for D5) is 11.75 days, 7.75 over A, above ER-37's 1-day line; the user gave the yes with both costs shown (D1 and D5, 2026-11-23).

## Work packages

Each owns its files. `crates/paddock-proc/src/lib.rs`, `lint/clippy.toml` and `.claude/settings.json` are shared integration
points (one declared line each, via the integrator). Prerequisites: **P1** `feat/adr0112-wp1-owner-trait` (b9970817d), rebased on the graph train (`train/2026-11-23-graph`) when the build
starts, then pushed and on a train (it holds `Owned`, `own::Unit` and the starter-bound holder; today it is on no remote); **P2** `fix/proc-tree-pid-reuse`
(682208725) landed; **P3** C159 landed.

| WP | What | Owns | Days | Needs |
| --- | --- | --- | --- | --- |
| WP-1 | `Scope` and `Held`: prepare (folder, `Unit`, guardian or per-scope `KillOnClose`), start, run, cancel, drop with a measured grace, summary log. Failing tests first: a panic inside a scope ends its child; an owner ended by SIGKILL leaves nothing (guardian); a Windows child ends when the scope's job closes; a cancel ends a wait; positive control: a child started outside the scope survives and is reported. | `crates/paddock-proc/src/scope.rs` (new), `crates/paddock-proc/tests/it/scope.rs`, one line in `lib.rs`, `docs/known-copies.toml` (Supervised and Reaper as views of Scope) | 1.5 | P1 |
| WP-2 | `start_detached` with `Lifetime`; `spawn_detached` private; the slot holder on `Lifetime::Starter` (test from d31c74d94: a starter ends, its holder follows). | `crates/paddock-proc/src/detach.rs`, `crates/paddock-slots/src/hold.rs`, `tools/paddock-dev/src/slots.rs`, their tests | 0.5 | WP-1 |
| WP-3 | Handle text, `held.jsonl` ledger (one reader with ADR 0112's), `./dev wait --pid <pid>:<start>`, `./dev held list\|wait\|end`. | `crates/paddock-proc/src/identity.rs` (Display/FromStr), `crates/paddock-proc/src/ledger.rs` (new), `tools/paddock-dev/src/wait.rs`, `tools/paddock-dev/src/held.rs` (new), tests | 0.75 | WP-1 |
| WP-4 | `ask_group(ProcessId)`; Pitcrew `RunnerProcess::id`, `graceful_stop` and `kill_tree` by identity, the Ctrl+Break helper re-checks; failing test first (pid handed on). | `crates/paddock-proc/src/tree.rs`, `apps/pitcrew/crates/pitcrew-service/src/{launch,supervisor}.rs`, `platform/win_session.rs`, their tests | 0.75 | P2 |
| WP-5 | Clippy ban on `Command::spawn`; the generated, shrink-only list of `#[expect]` sites and its growth check against the merge base. | `lint/clippy.toml` (one line), the list generator in `tools/paddock-dev/src/check/` (id from `./dev check new-id`) | 0.25 | WP-6 to WP-8 ready to merge together |
| WP-6 | Migration: `crates/` (65 sites) and the seven file-wide allows' crates. | the crates' files | 0.75 | WP-1, WP-2, P3 |
| WP-7 | Migration: `tools/paddock-dev` (27 sites). | its files | 0.5 | WP-1 to WP-3 |
| WP-8 | Migration: `apps/` (72 sites). | the apps' files | 1.0 | WP-1, WP-4 |
| WP-9 | Agents: measure whether the harness ends background tasks with their agent (a stand-in heartbeat after the agent ends); the `background-guard` hook with its `.test.sh` (positive control: `sleep 600 &` refused; `sleep 1 & wait` allowed). D4 (b) adds `./dev run --hold` and worktree removal ending held processes (+0.75). | `.claude/hooks/background-guard.{py,sh,test.sh}` (new), one line in `.claude/settings.json`; for D4 (b) `tools/paddock-dev/src/held.rs`, `worktree/ops.rs` | 0.5 (1.25) | WP-3 for (b) |
| WP-10 | Node: the handle check of ADR 0112 WP-6 extended to `detached: true` outside tests. | that check's file and fixtures | 0.25 | ADR 0112 WP-6 |
| WP-11 | Build folders: read what `worktree remove` and `disk clean` do today; owner records for build folders `worktree add` names; removal with the worktree. | `tools/paddock-dev/src/worktree/ops.rs`, `disk/record.rs`, tests | 0.5 | none |
| WP-13 | The CI wrapper (ADR 0118's `./dev ci run` / `ci step`) runs every command in the job's `Scope`, maps the runner's cancel signals to `scope.cancel()`, and writes the ended processes into the job record. Failing test first: a step whose command leaves a child and then fails; the child must be ended and listed. Positive control: the same step outside the wrapper leaves the child, and ADR 0112's net reports it. | the wrapper's files on the ADR 0118 branch (`tools/paddock-dev/src/ci_run/`, the job-record writer), their tests | 0.5 | WP-1; ADR 0118's job record |
| WP-14 | `Scope::run` requires `Limits` (no unbounded form); `paddock_testkit::TestCommand` runs through it, so its callers' `output()` and `status()` move with one change; the Clippy ban of D5 (`Command::output`, `status`, `Child::wait`, `wait_with_output`) with its generated, shrink-only `#[expect]` list. Failing test first: a child that never exits under `output()` hangs the test; through `Scope::run` it ends at its bound and is reported. | `crates/paddock-proc/src/scope.rs`, `crates/paddock-testkit/src/` (TestCommand), `lint/clippy.toml` (one line), the WP-5 list generator | 0.5 | WP-1, WP-5 |
| WP-15 | Migration D5: `crates/` and `tools/paddock-dev`. A text count on the train (files that build a process, `.output()`/`.status()`): crates 89 sites in 24 files (67 in tests), tools 183 in 54 (161 in tests); most test sites move with `TestCommand` (WP-14). | those files | 1.5 | WP-14 |
| WP-16 | Migration D5: `apps/` (146 sites in 49 files, 108 in tests, same count; some may be HTTP `status()` and drop out when Clippy reads them). | the apps' files | 1.0 | WP-14 |
| WP-12 | Docs: the rule (D7), AGENTS.md's working-practice line, this ADR's status, ADR 0103 and 0076 amendments (Reaper and guardian as parts of `Scope`). | `docs/rules.md`, `AGENTS.md`, `docs/adr/amendments/` | 0.25 | WP-1 |

Total B: 11.0 (WP-1 to WP-16, WP-9 at its (a) size) plus 0.75 for the fix rounds of the migration batches' reviews = **11.75**;
with D4 (b) **12.5**. WP-14 to WP-16 are D5's +3.0 days. WP-4, WP-9 (hook) and WP-11 can start at once; WP-6 to WP-8 run in parallel after WP-1 and WP-2.

Parallelism and cost (ER-20): a scope adds no process on Windows (one job object per scope) and one guardian per owner process on
Unix, as `Supervised` does today; the ledger is one appended line per start. The summary reads the table once per scope end
(`own::look`), not per member.

## Decisions (accepted, user, 2026-11-23)

Each decision below keeps its options as offered; the line "Accepted" records the user's choice.

- **D1. Scope of the fix.** Accepted (user, 2026-11-23): (b), B, now 11.75 days with D5. (a) Common practice, A: 4.0 days. (b) **Recommended**: B, the approved design, 8.75 days before D5 (4.75 more,
  above ER-37's 1-day line). Why (b): re-attaching by handle and declared lifetimes are what the incidents lacked (the orphaned
  holders, the run 8674 stale pids), and A leaves them to each caller.
- **D2. Number.** Accepted (user, 2026-11-23): keep the three separate. This ADR is 0119 because 0112 is taken by the accepted CI-job ADR and 0118 by the CI wrapper and job record.
  **Recommended**: keep the three; 0112 the job's net, 0118 the CI wrapper and record, 0119 the one process mechanism under both
  and everything outside CI. Alternative: fold this text into 0112 as an amendment (no cost difference).
- **D3. The background guard.** Accepted (user, 2026-11-23): (a), blocking. (a) **Recommended**: blocking (it guards state, like the kill guard). (b) Common practice for
  text hooks here: warn-only (ER-36).
- **D4. Agents' held work.** Accepted (user, 2026-11-23): measure in WP-9, then (a) or (b) as laid out. (a) Common practice: `run_in_background` plus the hook, 0.5 day. (b) Also `./dev run --hold` and
  worktree removal ending held processes, +0.75 day. **Recommended**: (a) if WP-9 measures that the harness ends background tasks
  with their agent, otherwise (b).
- **D5. `output()` and `status()`.** Accepted (user, 2026-11-23): (b), ban them too; every child runs through a `Scope` with a bound (WP-14 to WP-16, +3.0 days). (a) **Recommended**: stay allowed (they hold the child; a text count finds 881 `.output()`/`.status()` matches in 267 Rust files, some of them not processes). (b) Ban them
  too and move every site to `run_bounded`, about +3 days.
- **D6. Order.** Accepted (user, 2026-11-23): as recommended. **Recommended**: P1 (push the local 0112 branch), WP-4 and WP-2 first (they fix the two checked incidents' shapes
  and the shipped Pitcrew stop), then the rest after the current release train ("release before tooling", 2026-11-20).
  Alternative: all at once.
- **D7. The rule.** Accepted (user, 2026-11-23): number 97, because 91 to 96 are taken by the rules batch on
  `docs/rules-from-memory-2026-11-23` (not on origin on 2026-11-23, so not read here). ADR 0112 proposed "new Paddock rule 90" for start, hold, watch, end, but rule 90 on the train is now "a landing
  may rest on a narrowed run". **Recommended** (number superseded by the accepted 97): one rule, number 91, holding 0112's text widened to this ADR's scope, landed with
  WP-12; ADR 0112's copy is corrected when it lands.

## Consequences

- Every process a Paddock tool, test or app starts is held by a value whose end ends it, on every path the table in 1 lists.
- A child that must outlive its starter says until when; nothing lives on by accident for 3.5 hours.
- A later process (a CI step, the coordinator, an agent's next turn) can wait on or end a process by its handle, never by a search.
- Escapes stay possible on Unix: a program we start that calls `setsid` leaves its group, so neither the group signal nor the
  guardian reaches it (Windows jobs have no such escape: breakaway is not allowed); the scope's summary reports what it could not see (as ADR 0112's job summary does).
- Amended 2026-11-24 (WP-1 review round 2): on Unix a guarded child starts inside a group the guardian already holds (the keeper
  leads it and is registered first), so an owner killed at any point of a start leaves no unguarded child. A `pre_exec` gate
  was not used: std's `spawn` waits for the child's exec, so the owner could not register while the child waits. On Windows a
  child joins its jobs only after `CreateProcess` returned (no `CREATE_SUSPENDED` start yet): a grandchild started in that
  moment, or an owner killed in it, leaves a process outside the jobs. A suspended start (create suspended, join the jobs,
  resume) closes it and is left for the migration's Windows work.
- Migration touches about 164 sites in 94 files; reviews of those batches are mechanical.

## Status of the mechanisms (ADR 0096)

| Mechanism | Where |
| --- | --- |
| `ProcessId`, `kill_tree`, `end`, `ask`, `terminate`, `Supervised`, `Registry`, guardian, `own::{look, KillOnClose}`, `run_bounded`, `spawn_detached` | `origin/main` (d04af7b46), `crates/paddock-proc` |
| Clippy `Command::new` and kill bans; C53 | `origin/main`, `lint/clippy.toml`, `tools/paddock-dev/src/check/c53.rs` |
| C126 rule C | `origin/main`, report-only |
| Kill guard, waiter guard | `origin/main`, `.claude/hooks/` |
| `./dev wait --pid` (pid only) | `origin/main`, `tools/paddock-dev/src/wait.rs` |
| One-read table entries, logged ends, `processes_inside`, Pitcrew harness by identity | `origin/fix/proc-tree-pid-reuse` 682208725, prerequisite P2 |
| `Owned`, `own::Unit`, starter-bound slot holder, ADR 0112's job action and ledger | local `feat/adr0112-wp1-owner-trait` b9970817d, not on origin: prerequisite P1 |
| C159, the file-wide kill-allow guard | id reserved (`refs/paddock/check-ids/C159`, `feat/conformance-guards-2026-11-23-kill-allow`), branch not on origin: prerequisite P3 |
| CI wrapper `./dev ci step` and the job record | ADR 0118, uncommitted in the `feat/test-results-artifact` worktree (no commit beyond 64e01dd84 on 2026-11-23): prerequisite for WP-13 |
| `Scope`, `Held`, `Lifetime`, `start_detached`, handle text, `held.jsonl`, `./dev held`, `ask_group`, the spawn ban, `background-guard`, `./dev run --hold` | not built (this ADR) |

Recurring: "something ends but does not end what it started" (ADR 0112's class), outside CI jobs. Earlier instances: run 8099
(a detached `slot hold` left by a failed test, 82d5ee496), ADR 0103 (test processes outlived tests), run 8674 (a harness ended
strangers by stale pid, 682208725), and the four orphaned holders of 2026-11-22/23 (lessons, rule 89). Second redesign for the
class after ADR 0112, which covered only CI jobs.
