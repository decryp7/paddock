# 0112. A CI job owns its processes as a unit, so ending the job ends everything it started

- Status: Proposed 2026-11-21. The user approved drafting this redesign on 2026-11-21 (ER-21, relayed by the coordinator). No
  decision below is taken yet.
- Date: 2026-11-21
- Related: ER-14 (no process left behind), ER-21 / rule 40 (a recurring class gets a redesign), ER-22 / rule 42 (share first),
  ER-27, ER-33 (positive controls), ER-37 / rule 76 (common practice and cost), ER-41 (new checks start report-only), ER-48 (the
  layout lives in the repository); rule 53 (act on identity) and ADR 0033; rule 55 (`docs/known-copies.toml`); ADR 0025 (CI job
  isolation); ADR 0103 (the test `Reaper`); the marker cleanup on `feat/lessons-check-and-job-cleanup`.
- Number: 0112. On 2026-11-21 the highest ADR on `origin/main` was 0108 and on any `origin` branch 0111
  (`git ls-tree docs/adr/` over every remote branch).
- Amends, if accepted: ADR 0025 (a job owns its processes), ADR 0103 (the Reaper's group rule moves into `paddock_proc::own`),
  the lesson "Job processes (ER-14)" on `feat/lessons-check-and-job-cleanup` (its Windows premise is wrong, see below),
  `tools/paddock-dev/src/slots.rs` (its note on the runner's job object), `.gitea/actions/README.md` (runner versions).

Evidence heads read (ADR 0091), re-read right before the commit: `origin/main` 3d776b605,
`origin/feat/lessons-check-and-job-cleanup` 9dfa4e6dc. Shipped code: none. This is CI tooling; no app tag ships it.
`git tag --contains 82d5ee496` is empty. The branch's commits (c528dbfb1, b62fbe906, 9dfa4e6dc) are not on `origin/main`.

Runner sources read at their tags on 2026-11-21 (`gitea.com/gitea/runner`, MIT): v5.4.1 (the Mac runner) and v6.0.1 (the
Windows runner). Versions taken from line 1 of job logs that day: job 43808 (`macos-latest`) says `version:v5.4.1`, job 43703
(`windows-latest`) says `version:v6.0.1`.

## Context: the class

The class is "something ends but does not end what it started". Instances:

| When | What | Evidence |
| --- | --- | --- |
| 2026-11-14 | VM tooling tests on macOS failed 15 tests. A detached `ci slot hold` and a `vm-win.sh` were left by a test that failed partway. The leak failed the tests after it. | run 8099, job 38394 (conclusion `failure`, checked through the API on 2026-11-21); fix 82d5ee496 |
| 2026-11-15 | A test's processes outlived the test until the `Reaper` was built. It ends a test's processes, not a job's. | ADR 0103, `crates/paddock-testkit-start/src/reap.rs` on main |
| 2026-11-20 | `vm-lock.sh` (`vm_lock_acquire`, pid 58558) still ran on the Mac runner four days after its job ended. | commit c528dbfb1 on `feat/lessons-check-and-job-cleanup` (an observation on the runner, no CI run) |
| reported 2026-11-21 | `paddock-dev ci slot hold` processes left behind after agents ended. | coordinator's report in the brief; no run or commit checked. Not counted as an incident row. |

Each fix so far was a per-owner fix: a test file ends its holders (82d5ee496), a test ends its children (the Reaper), a job ends
what carries its marker (the branch). Each finds members after the fact, by what they carry. None makes the owner hold its
members as one unit.

### What the branch builds

`feat/lessons-check-and-job-cleanup` (head 9dfa4e6dc) adds `dev ci isolation processes stop|sweep`
(`tools/paddock-dev/src/ci_isolation/processes.rs`) on `paddock_proc::own::survey` (`crates/paddock-proc/src/own.rs` on the
branch). `release` ends every process that carries the job's marker: `PADDOCK_JOB_TEMP` in its environment, the job folder as
its current folder, or the job folder in its command line. It ends each by identity through `paddock_proc::terminate`
(rule 53). It spares the release step, its ancestors and the slot holder. On Windows it answers "not measured", because another
process's environment and folder are not read there.

### What the runner already does (read from source, not measured on our runners)

The branch's lesson says the runner "gives the job no process group or job object that its end closes". For Windows that is
not true of the versions we run.

- **Each step on Unix** starts as the leader of a new process group: `SysProcAttr` returns `Setpgid: true`
  (`internal/pkg/process/sysprocattr_unix.go`, the same file at v5.4.1 and v6.0.1). The runner signals that group only when the
  step is cancelled (`killer_unix.go`: SIGINT, 7.5 s, SIGTERM, 2.5 s, then SIGKILL to `-pgid`). A step that ends normally leaves
  its group's other members running (`treekill.go`, `Wait`: the leftover kill runs only on Windows).
- **Each job on Windows** gets one job object (`group_windows.go`, `Group`). Every tracked step process is assigned to it while
  it is still suspended (`act/container/host_environment.go`, `exec`: `processGroup(ctx).Assign`). `Remove`, at the end of the
  job, calls `Group.Close`, which calls `TerminateJobObject`. So every process a step started on Windows ends with the job,
  orphans included. The job sets no breakaway limit, so a child cannot leave it. Since v5.4.x it no longer sets
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (v5.2.3 did), so a runner that crashes leaves them. Added by gitea/runner #1080
  (2026-07-22, "stop host-mode jobs from leaking processes on Windows").
- **Each job on Linux and macOS** ends with `KillTrackedProcesses` (`tracked.go`, same file at v5.4.1 and v6.0.1). It SIGKILLs
  every process whose environment holds `RUNNER_TRACKING_ID=<the job's id>`, "like GitHub's orphan cleanup". It acts on a bare
  pid read from the table, not on an identity.
- **On macOS that marker misses most of what leaked.** macOS does not show the environment of a program Apple ships. I measured
  it on 2026-11-21 (macOS 27.2.1): `ps eww` of `/bin/sleep` started with `MARKX=adrprobe` shows no `MARKX`; an ad-hoc signed copy
  of the same binary shows it. The branch measured the same for `/bin/bash`, `/bin/sh` and `/bin/sleep`. `vm-lock.sh` runs
  under `#!/usr/bin/env bash`, which on the runner may be Apple's bash. So the runner's own job-end cleanup cannot see it.
- **Runner hooks.** Both versions run `runner.hooks.job_completed` "after the job's last post step, while the job environment
  is still up", inside the job environment, and the docs say to "expect `job_completed` to run after success, failure, and
  cancellation alike" (`docs/job-hooks.md`). A non-zero exit fails the job. `runner.post_task_script` runs on the host after
  the runner's own cleanup, bounded by `post_task_script_timeout` (default 5 m), while the runner takes no new work
  (`docs/post-task-script.md`).

So the gap is narrow. Windows is covered by the runner's job object. Linux jobs run in containers (ADR 0025), which Docker
removes with all their processes. The gap is macOS: a process a step left in its group, that runs an Apple binary, is not
seen by the runner's marker, and is seen by the branch's markers only while its folder or command line names the job.

## Comparable practice (ER-37)

| Software | How a job's processes end | Source (read 2026-11-21) |
| --- | --- | --- |
| GitHub Actions runner | Cancel: SIGINT, then SIGTERM, then kill. On Windows it walks the tree by parent pid; on Unix it kills the direct child only (`ProcessInvoker.cs`, `KillProcessTree`, `NixKillProcessTree`). Job end: `FinalizeJob` kills every process not in the start snapshot whose environment has the job's `RUNNER_TRACKING_ID`; the job variable `process.clean` turns it off (`JobExtension.cs`). There is no `ACTIONS_RUNNER_PROCESS_CLEANUP` variable. Job hooks: `ACTIONS_RUNNER_HOOK_JOB_STARTED` / `_COMPLETED`. | [ProcessInvoker.cs](https://github.com/actions/runner/blob/main/src/Runner.Sdk/ProcessInvoker.cs), [JobExtension.cs](https://github.com/actions/runner/blob/main/src/Runner.Worker/JobExtension.cs), [run scripts](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/run-scripts) |
| Gitea runner (ours) | Per step: a process group (Unix), killed on cancel; per job: a job object (Windows) terminated at job end; `RUNNER_TRACKING_ID` kill at job end (Linux, macOS). Hooks `job_completed` and `post_task_script`. | [host_environment.go](https://gitea.com/gitea/runner/src/tag/v6.0.1/act/container/host_environment.go), [internal/pkg/process](https://gitea.com/gitea/runner/src/tag/v6.0.1/internal/pkg/process), [job-hooks.md](https://gitea.com/gitea/runner/src/tag/v6.0.1/docs/job-hooks.md) |
| GitLab Runner | Unix: SIGTERM, then SIGKILL, to the negative pid, "the process group" (`helpers/process/killer_unix.go`). Windows: a job object per process when `FF_USE_WINDOWS_JOB_OBJECT` is on (default off); "To force-kill the processes, the runner closes the job object." | [killer_unix.go](https://gitlab.com/gitlab-org/gitlab-runner/-/blob/main/helpers/process/killer_unix.go), [feature flags](https://docs.gitlab.com/runner/configuration/feature-flags/) |
| Buildkite agent | `cancel-signal`, then SIGKILL after `cancel-signal-timeout`. Whether it signals a group or a job object is not in its docs; I did not read its code (unverified). | [agent configuration](https://buildkite.com/docs/agent/v3/configuration) |
| systemd | A unit is a cgroup. `KillMode=control-group` (the default): "all remaining processes in the control group of this unit will be killed on unit stop". `process` and `none` are "not recommended". `systemd-run --scope` puts a command in its own unit. | [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html) |
| Microsoft | `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: "Causes all processes associated with the job to terminate when the last handle to the job is closed." A child joins every job of its parent unless the job allows breakaway. Since Windows 8 jobs nest; terminating a job terminates its child jobs. | [JOBOBJECT_BASIC_LIMIT_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information), [Nested jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs) |
| POSIX | `setsid` makes the caller "the process group leader of a new process group"; `killpg` signals a group. "A process group ID shall not be reused by the system until the process group lifetime ends." A member can leave by calling `setsid` or `setpgid` itself. | [setsid](https://pubs.opengroup.org/onlinepubs/9799919799/functions/setsid.html), [killpg](https://pubs.opengroup.org/onlinepubs/9799919799/functions/killpg.html), [XBD 4.17 Process ID Reuse](https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html) |
| macOS | No cgroups and no subreaper. launchd: "When a job dies, launchd kills any remaining processes with the same process group ID as the job" unless `AbandonProcessGroup` (`man launchd.plist`, read locally). That is the runner's group, not a step's (the runner moves each step into a new group), so it does not reach step leftovers. | `man 5 launchd.plist` on macOS 27.2.1 |
| nextest, Bazel | nextest ends a test's process group (Unix) or job object (Windows) on timeout; Bazel: "Tests may create subprocesses, but not process groups or sessions". | ADR 0103's table |

The common practice is: a kernel unit where the OS has one (cgroup, job object), a process group where it does not, and an
environment marker at job end as the net. Our runner already gives us the job object on Windows and the marker on Unix. What no
comparable tool does on macOS is end a step's group at job end; the runner's Unix `Group` is a no-op with the comment "process
groups already reclaim a step's tree", which holds only on cancel.

## Options

Costs are agent-days, against the common-practice baseline: rely on the runner (job object on Windows, marker on Unix), plus
the branch's markers, which are already built (about 1.5 days spent, not counted below).

**(A) Markers only (the branch as it is), plus one Windows measurement.** Keep `processes stop|sweep`. Replace "not measured" on
Windows with what the runner does: a positive control on the real Windows runner, where a step leaves a stand-in that writes a
heartbeat file outside the job folder, and the next job shows the heartbeat stopped at the first job's end. Fix the wrong lesson
and the `slots.rs` note. Cost: 0.5. Gap: on macOS an Apple-binary process that left the job folder and does not name it lives on.

**(B) A job-scoped group ledger: every step's process group is recorded, and the job's end ends every member by identity.**
This is the brief's wrapper, changed in one point: it ends the groups at the end of the job, not at the end of each step. A
per-step kill would end what a step starts on purpose for later steps (the job's sccache server from setup-rust, background
services a later step talks to), and the slot holder already leaves the step's group by design (`slots.rs`).
- Recording: the runner already makes each step a group leader on Unix. The acquire step exports `BASH_ENV=<kit>/job-group.sh`
  through `GITHUB_ENV`. Bash sources it in every later non-interactive bash, so each `shell: bash` step appends
  `pgid leader-pid leader-start` to `<job temp>/groups` (I measured that Apple's bash 3.2.57 sources `BASH_ENV` under the
  runner's `--noprofile --norc -e -o pipefail`). No workflow or composite step changes. The wrapper form
  (`./dev ci job run -- <step>` as `defaults.run.shell`) is the alternative; it does not reach composite actions, whose steps
  name their own shell, so it needs an edit per composite step.
- Ending: one read of the process table (`paddock_proc::own::look` gives each row's group, on main). A row is a member when its
  group is in the ledger, it runs as this user, and it started at or after that group's leader. Descendants of a member that
  left its group (a `setsid` child) are followed by age, as rule 53 allows. Each is ended by identity through
  `paddock_proc::terminate`, never with `killpg` (Clippy already bans `libc::killpg`, `lint/clippy.toml`). The ledger is a hint
  file in rule 53's sense: a row is acted on only after the table proves the membership. POSIX keeps a group id from reuse
  while the group has members, and the start-time test covers a group that emptied and whose id came back.
- Markers stay as the second net for what has no group in the ledger: a process that called `setsid` and is no descendant of a
  member, a non-bash step (a Node action), and the sweep of jobs that died before `release`.
- Windows: no code. The runner's job object is the mechanism; the ADR records it, and the positive control of (A) checks it.
- Cost: 1.75 (ledger hook and reader 0.5, the shared owner type below 0.5, stand-in tests with positive controls 0.5, docs and
  lesson corrections 0.25). Beyond baseline: 1.75, so it needs the user's yes (ER-37).

**(C) Patch or configure the runner.** Configure: the runner has no setting for a Unix job group. Patch upstream: make the Unix
`Group` record each tracked step's pgid and signal those groups in `Remove`, the same shape as its Windows `Group`. Cost: 1.0 for
the pull request with tests, then an unknown wait for review and a release, then a Mac runner update. A local fork means building,
signing and updating our own runner binary on the Mac, about 3 plus ongoing upkeep; I reject the fork.

**(D) Runner-level trigger: `runner.hooks.job_completed` runs `processes stop` (and the ledger end of B), and
`runner.post_task_script` runs `processes sweep --end`.** This makes the end run even when a workflow has no `release` step,
when `release` is skipped, or when the job is cancelled. It changes nothing about who is a member; it is when the end runs. The
hook must always exit 0 (a failing hook fails the job). The runner config lives on each host, so the files go in the repo
(`conformance/ci/runner-hooks/`) with a check that reads job logs for the hook's group line (ER-48: the layout lives in the
repository). Cost: 0.75.

## Recommendation

1. **B + D, with the Windows control of A.** B gives macOS a real unit (the step groups the runner already makes), D makes sure
   the end runs however the job ends, A's control proves the Windows job object on our runner. Total: 0.5 + 1.75 + 0.75 = 3.0,
   which is 2.5 beyond the cheapest option (A alone).
2. **Fallback: A + D (1.25).** This is the common practice (marker cleanup at job end, run from a runner hook), plus our markers by
   folder and command line, which already go beyond GitHub's. It leaves the macOS Apple-binary gap open but reported by `sweep`.
3. **File C upstream as an issue** (0.25, no code). If the runner gains a Unix job group, B's ledger retires and the markers stay.

What stays as the fallback in every case: the markers (`processes stop|sweep`), the slot holder's own end (its owner folder goes),
`host-cleanup`'s patterns, and the Reaper for tests.

## Design (if B is accepted)

### One owner type in `paddock_proc::own`

Today three places decide "what did this owner start": the Reaper's `choose` (`crates/paddock-testkit-start/src/reap.rs`, on
main), the job's `Job::owns` and `stop_job` (branch), and `host-cleanup`'s patterns (`tools/paddock-dev/src/ci_host/cleanup.rs`,
on main). The pure part becomes one type, `own::Unit`: the groups it holds (each kept by its leader's identity), its marker
(token, folder, command-line spellings), its start time, and the identities it must spare. One function `members(table, unit)`
answers the candidates; ending stays `terminate` by identity. The Reaper and the job end both call it. It is registered in
`docs/known-copies.toml` as one mechanism with its two callers (rule 55), and `host-cleanup`'s pattern list is named as the copy
that shrinks once the unit covers its cases.

### Fit with rule 53 and ADR 0103

- Rule 53 says a tree is followed by age or by job membership, never by parent pids alone, and that one acts on an identity read
  just before acting. The ledger's groups are membership; age decides descendants; `terminate` acts on identity. Nothing signals
  a group id.
- ADR 0103 already keeps groups "by their leader's identity" and lets a test helper make no new group while the test leads one.
  The same rule holds for a step: a helper never makes a new group inside a step, except the slot holder, which is spared by
  name and ends with its owner folder.
- On Windows the Reaper already uses `own::KillOnClose`. The job end there is the runner's job object; nothing new.

### Agents' leftover holders

The reported `ci slot hold` leftovers after agents end are a holder whose owner folder outlives its owner. B does not cover
agents (they are not CI jobs). The same `Unit` can: the holder takes its starter's identity as its owner too, and ends when that
identity has ended. That is a separate work package (WP-5) and needs its own failing test first; it is listed so the class is
closed in one place, not sized into the recommendation.

## Work packages

| WP | What | Days | Needs |
| --- | --- | --- | --- |
| WP-0 | Windows positive control on the real runner (a stand-in outlives its step, ends with the job). Correct the branch's lesson, the `slots.rs` note and the runner versions in `.gitea/actions/README.md`. | 0.5 | the branch landed |
| WP-1 | `own::Unit` and `members` in `paddock_proc::own`, unit tests on rows; the Reaper moves onto it with its existing tests green; known-copies entry. | 0.5 | none |
| WP-2 | Ledger: `conformance/ci/job-group.sh` (via `BASH_ENV` from acquire), `processes stop` reads it; stand-ins: a group member that ignores SIGTERM, an Apple-binary member that moved folder (the gap case), a `setsid` child of a member; a second job's stand-in as the positive control that must survive (ER-33). | 1.0 | WP-1, the branch |
| WP-3 | Docs: ADR 0025 amendment, rule line proposal, lesson. | 0.25 | WP-2 |
| WP-4 | Runner hooks in the repo, installed on the Mac and Windows runners by the user, and a report-only check on job logs that the hook ran (ER-41: blocking after its first green run, recorded by run id). | 0.75 | none |
| WP-5 | Holder bound to its starter's identity (agents). | 0.5 (not in the total) | WP-1 |
| WP-6 | Upstream issue for a Unix job group in gitea/runner. | 0.25 | none |

Total for the recommendation: WP-0 to WP-4 = 3.0 days. Fallback: WP-0 + WP-4 = 1.25 days.

## Decisions for the user

- **D1.** B + D (3.0 days) or the common-practice fallback A + D (1.25 days)? Recommended: B + D, because the 2026-11-20 leak
  was exactly the macOS case that only B closes.
- **D2.** May the end run from runner hooks (`job_completed`, `post_task_script`) set on each runner host? This is config on the
  machines, outside the CI workflows. Recommended: yes, with the files and the check in the repo.
- **D3.** Record steps by `BASH_ENV` (no workflow edits) or by the `./dev ci job run` wrapper as the shell (visible in every
  workflow, but each composite step needs an edit)? Recommended: `BASH_ENV`.
- **D4.** File the upstream issue for a Unix job group in gitea/runner (WP-6)? Recommended: yes; no fork.
- **D5.** Add WP-5 (agents' holders) now or after B lands? Recommended: after, with its failing test first.

## Consequences

- A step's background processes now end at the job's end on macOS, as they already do on Windows. A workflow that meant one to
  outlive the job (none known) breaks; that was already against ER-14.
- The branch's "not measured" on Windows becomes a measured answer from WP-0, not a pass by assumption (ADR 0089).
- Escapes stay possible on macOS: a process that calls `setsid` from a non-recorded step and clears its environment. The sweep
  reports what it can see; the rest is the known limit of a system with no cgroups.

## Status of the mechanisms (ADR 0096)

| Mechanism | Where |
| --- | --- |
| `paddock_proc::own::{look, group_of, own_group, KillOnClose}`, `terminate`, `kill_tree` | `origin/main` |
| Clippy ban on `libc::killpg` | `origin/main`, `lint/clippy.toml` |
| `Reaper` | `origin/main`, `crates/paddock-testkit-start/src/reap.rs` |
| `own::survey`, `processes stop` and `sweep`, `PADDOCK_JOB_TEMP` export | `feat/lessons-check-and-job-cleanup` (9dfa4e6dc), a prerequisite |
| Runner per-step group, Windows job object, `RUNNER_TRACKING_ID` kill, hooks | gitea/runner v5.4.1 (Mac) and v6.0.1 (Windows), external; hooks not configured yet |
| `own::Unit`, ledger, `job-group.sh`, hook files, hook check | not built (this ADR) |
