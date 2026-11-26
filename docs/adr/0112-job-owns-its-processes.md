# 0112. Start, hold, watch, end: every CI job is a fixture that owns its processes, its folder and its handles

- Status: Accepted 2026-11-21 (user, relayed by the coordinator). D1: B, the full fix. D2: no runner host hooks (the user dropped
  option D the same day: the runners serve other repositories, and host config belongs to the user); the end runs inside Paddock.
  D3: `BASH_ENV`. D4: an upstream issue, no fork. D5: bind slot holders to their starter now. The same day the user widened the
  decision: every Paddock job is a fixture (prepare, execute, clean up) from one shared job action, under the principle "start,
  hold, watch, end" (ER-51). Later the same day the user added one log bundle per CI run ("Decision: one log bundle per run"). Proposed earlier on
  2026-11-21 (df851a925).
- Date: 2026-11-21
- Related: **ER-51** (start, hold, watch, end; the fixture shape; one folder per unit; engineering-rules cdb2a9c), ER-14, ER-21 /
  rule 40, ER-22 / rule 42, ER-27, ER-32, ER-33, ER-37 / rule 76, ER-41, ER-43, ER-46, ER-48; rule 53 and ADR 0033; rule 55
  (`docs/known-copies.toml`); ADR 0025 (CI job isolation); ADR 0103 (the test `Reaper`); ADR 0111 (no writable folder shared
  between lanes); the new Paddock rule 97 (this ADR; numbered 97 by ADR 0119 D7, as 90 was taken).
- Number: 0112. On 2026-11-21 the highest ADR on `origin/main` was 0108 and on any `origin` branch 0111.
- Amends: ADR 0025 (a job owns its processes and its folder; the job action replaces the separate acquire and release steps),
  ADR 0103 (the Reaper's group rule moves into `paddock_proc::own`), the lesson "Job processes (ER-14)" on
  `feat/lessons-check-and-job-cleanup` (corrected in docs/lessons.md by this change), `tools/paddock-dev/src/slots.rs` (its note on
  the runner's job objects: the corrected comment is WP-7's first commit, text below, because a change there needs the
  paddock-dev Clippy rows, which this docs branch could not get a heavy slot for on 2026-11-21), `.gitea/actions/README.md` (runner versions, WP-12).
- Upstream: [docs/research/2026-11-21-gitea-runner-unix-job-group-issue.md](../research/2026-11-21-gitea-runner-unix-job-group-issue.md), the issue text for gitea/runner (D4), to be posted by the
  coordinator.

Evidence heads read (ADR 0091), re-read right before the commit: `origin/main` 3d776b605,
`origin/feat/lessons-check-and-job-cleanup` 9dfa4e6dc, `origin/train/2026-11-21-18` 75ce0d2c8 (the health gate and no-prompts,
aa64cb4c3 via d7b27387c, and the joint reservation 63cd1e7ff; neither is on main), `origin/fix/ci-no-shared-lane-folders` 18ecd84a4
(ADR 0111). Shipped code: none. This is CI tooling; no app tag ships it. `git tag --contains 82d5ee496` is empty.

Runner sources read at their tags on 2026-11-21 (`gitea.com/gitea/runner`, MIT): v5.4.1 and v6.0.1. Versions from line 1 of job
logs that day: job 43808 (`macos-latest`) v5.4.1, job 43703 (`windows-latest`) v6.0.1, job 43798 (`ubuntu-latest`) v6.0.1.

## Context: the class

The class is "something ends but does not end what it started". Instances:

| When | What | Evidence |
| --- | --- | --- |
| 2026-11-14 | VM tooling tests on macOS failed 15 tests. A detached `ci slot hold` and a `vm-win.sh` were left by a test that failed partway; the leak failed the tests after it. | run 8099, job 38394 (conclusion `failure`, checked through the API on 2026-11-21); fix 82d5ee496 |
| 2026-11-15 | A test's processes outlived the test until the `Reaper` was built. It ends a test's processes, not a job's. | ADR 0103, `crates/paddock-testkit-start/src/reap.rs` on main |
| 2026-11-20 | Leaked test folders filled the user temp folder on the Mac runner; Chrome took over 5 s to quit while it read them. A unit wrote outside a folder it owned. | ci.yml runs 8334, 8341, 8346 (Spotter); docs/lessons.md "Chrome took over 5 s to quit" |
| 2026-11-20 | `vm-lock.sh` (`vm_lock_acquire`, pid 58558) still ran on the Mac runner four days after its job ended. | commit c528dbfb1 on `feat/lessons-check-and-job-cleanup` (an observation on the runner, no CI run) |
| reported 2026-11-21 | `paddock-dev ci slot hold` processes left after agents ended. | the coordinator's report; no run or commit checked, not counted as an incident |

Each fix so far was per owner and found members after the fact, by what they carry. None made the owner hold what it started.

### What the runner already does (read from source, not measured on our runners)

- **Each step on Unix** leads a new process group (`internal/pkg/process/sysprocattr_unix.go`: `Setpgid: true`, the same file at
  v5.4.1 and v6.0.1). The runner signals that group only on cancel (`killer_unix.go`: SIGINT, 7.5 s, SIGTERM, 2.5 s, SIGKILL to
  `-pgid`). A step that ends normally leaves the rest of its group running (`treekill.go`, `Wait`: the leftover kill is Windows-only).
- **Each job on Windows** has one job object (`group_windows.go`, `Group`). Every tracked step process joins it while still
  suspended (`act/container/host_environment.go`, `exec`); `Remove` at the job's end calls `TerminateJobObject`. No breakaway is
  allowed. Since v5.4.x it no longer sets `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (v5.2.3 did), so a crashed runner leaves them.
  Added by gitea/runner #1080 (2026-07-22). The branch's lesson said the opposite; this change corrects it.
- **Each job on Linux and macOS** ends with `KillTrackedProcesses` (`tracked.go`): SIGKILL by bare pid for every process whose
  environment holds the job's `RUNNER_TRACKING_ID`.
- **macOS hides that marker** for programs Apple ships. Measured 2026-11-21 on macOS 27.2.1: `ps eww` of `/bin/sleep` started with
  a marker shows none; an ad-hoc signed copy of the same binary shows it. The branch measured the same for `/bin/bash` and `/bin/sh`.
  So a bash script a step leaves behind outlives the job.
- **`if: always()` and `post` on failure and cancel.** `newMainStepsExecutor` (`act/runner/job_executor.go`, same at v5.4.1 and
  v6.0.1) keeps going after an interrupt: `runMainStepsAfterInterrupt` runs the remaining steps under a fresh 5-minute context, so
  `always()` and `cancelled()` steps run ("mirroring GitHub Actions"; gitea/runner #1043, 2026-06-28). Post steps run after the
  main steps under `postStepsContext`, which detaches from a cancel or a job timeout with a fresh 5-minute deadline. A post step's
  default condition is `always()` (`step.go`: `DefaultStatusCheckAlways` for `stepStagePost`). It runs when the action's main step
  ran and was not skipped (`action.go`, `shouldRunPostStep`), and only node, docker, go and composite actions have one
  (`hasPostStep`). What skips both: a cancel during job setup (`setupError`: "no step runs after the job setup failed"), a runner
  crash or a hard kill, and cleanup that runs past the 5-minute bound.

So Windows is covered by the runner's job object, Linux by the container, and macOS has a real gap.

## Principle: start, hold, watch, end (ER-51)

Whoever starts something gets a handle back from the start call, keeps it, waits on it with a bound, reads its result from it,
and ends it through it in cleanup. Nothing is found again later by name or by search: no `pgrep` or `pkill`, no `ps | grep`, no
"the newest run on this branch".

### The kinds of handle

| Thing | Handle | Where it lives |
| --- | --- | --- |
| A process | `paddock_proc::ProcessId` (pid plus start time) | `paddock-proc` (rule 53) |
| A slot or lock | the holder token (`PADDOCK_CI_SLOT_TOKEN`) plus the holder's `ProcessId` | `paddock-proc` and `paddock-slots` |
| A job's processes | the job ledger (B below): the step groups, each by its leader's `ProcessId` | `paddock-proc::own` |
| A unit's files | its one folder, by `paddock_env::FileIdentity` where it must be re-checked | `paddock-env` and `paddock-watch` |
| A CI run | its run id | `paddock-dev` (`ci_runs`) |
| A container | the id the docker helper returns | `paddock-dev` |

### What exists, mapped to it

- Rule 53 and ADR 0033: act on a held identity, never on a pid read earlier. Enforced by Clippy `disallowed-methods`
  (`lint/clippy.toml`: `libc::kill`, `libc::killpg`, nix and sysinfo kills) and conformance C53.
- ER-32 and `./dev wait --pid | --ci-run | --plock | --file ... --line ...`, always with `--max`: wait on a real signal with a bound.
- ADR 0103's `Reaper`: a test holds what it spawned.
- C143, the spawn lint (cited by the coordinator; I did not read its code for this ADR).
- `paddock_proc::Supervised`, `Session`, `own::KillOnClose`: a child held by a value whose drop ends its tree.

### Gaps (a sample, not a full inventory)

- A background start with no saved pid: `apps/pitwall/scripts/linux-container-inner.sh:54` starts the service with `&` and never
  reads `$!` (inside a container, so the container ends it). Other scripts with `&` save `$!` (`macos-smoke.sh`, `macos-verify.sh`,
  `vm-win.sh`).
- `pgrep`, `pkill` and `ps | grep`: none found in tracked scripts and workflows on main on 2026-11-21. The check (WP-6) keeps it so.
- A dispatch that finds its run again afterwards: `tools/paddock-dev/src/ci_runs/dispatch.rs` waits for the runner and dispatches,
  and I found no run id in its result; a caller that needs the run looks it up (to verify in WP-6).
- Slot holders whose owner is an agent: the holder ends when its owner folder goes, and an agent's folder outlives the agent (D5,
  WP-7).
- A job's processes on macOS: nothing holds them after a step ends (the gap above; WP-3).

### Where the mechanism lives

No new crate. Each kind of handle stays with the crate that already owns it: processes and slots in `paddock-proc` (`own`,
`ProcessId`), files in `paddock-env` and `paddock-watch`, CI runs and containers in `paddock-dev`. They share one small trait in
`paddock_proc::own`, `Owned`: a handle, `wait(bound)` (ends at the bound with an outcome, never blocks for ever), `result()`, and
`end()` (by the held handle). The job ledger, the Reaper and the slot holder implement it in `paddock-proc`; a CI run and a
container implement it in `paddock-dev`.

The alternative was one "monitor everything" crate holding processes, files, runs and containers. I rejected it: it would have
four reasons to change (single responsibility), it would pull an HTTP client and the docker helper into every crate that only
wants a process (low coupling), and it would copy what `paddock-proc` and `paddock-dev` already do instead of sharing it (ER-22).

## Decision: every job is a fixture

A Paddock job has three parts, like a unit test: prepare, execute, clean up. Prepare and cleanup come from **one shared job
action**, `./.gitea/actions/job`, so they cannot be unpaired. Execute is the job's own steps, unchanged.

### The shape: a node action with `main` and `post`

`runs.using: node20`, `runs.main` runs prepare (`./dev ci job prepare`), `runs.post` runs cleanup (`./dev ci job cleanup`). This is
GitHub's documented pre/post shape for JavaScript actions, and our runner runs the post step on success, failure and cancel (above).
The action file is a thin launcher; the logic is in `paddock-dev`. Preconditions, measured in WP-4 before the migration: Node is on
the host runners' PATH before the job's own setup-node (the runner itself needs it for node actions), and the post step still finds
the action after stable-workspace moved the checkout (it leaves a link at the old path).

**Fallback**, if WP-4 shows `post` unreliable on a runner: a prepare action plus one shared cleanup step
`uses: ./.gitea/actions/job-cleanup` with `if: always()` as the job's last step. It runs on cancel too (`runMainStepsAfterInterrupt`),
but it can be left out of a job, so the check (WP-5) must then also require the last step. That is why `post` is preferred.

The local action needs the checkout, so the order is: checkout, then the job action, then the job's steps. Post steps run in reverse
order, so cleanup runs before the checkout's own post.

### Prepare collects what is scattered today

1. **The health gate and no-prompts** (`tools/paddock-dev/src/health.rs`, aa64cb4c3, on train 18): hold on a down Gitea or RustFS;
   tools never wait on stdin or auto-install.
2. **The reservation is checked, not waited for.** `runner-wait` stays in the run's first job on a Linux runner, where waiting holds
   nothing (ER-43). Prepare on a host job only reads that the run holds the reservation (the joint reservation, 63cd1e7ff, train 18)
   and says so; it never waits while it holds the host runner.
3. **Job isolation acquire** (the slot, `PADDOCK_JOB_TEMP`, the socket folder) and the sweep that reports processes of finished jobs
   (`processes sweep`, branch).
4. **The ledger start**: exports `BASH_ENV=<kit>/job-group.sh` so every later bash step records its process group (B below).
5. **The job's one folder** (ER-51): the lane folders of ADR 0111 and the queued scratch-root item (one shared scratch root, and a
   launcher that sets the working folder, `TMPDIR` and the environment) become one step. Prepare makes `<job temp>` (the runner's
   own per-job `tmp/`, removed with the job) the root for temp, logs and outputs, points `TMPDIR`, `TMP`, `TEMP` and `HOME`-relative
   caches that are not named shared caches at it, and exports it. Nothing writes elsewhere except declared outputs and the shared
   caches named in `kept-folders.txt` (ADR 0025). Today's jobs that write outside their folder, a sample: Spotter's real-browser
   tests left folders in the user temp folder (runs 8334, 8341, 8346); `linux-container-inner.sh` writes `/tmp/data` (inside its
   container); `ci boot-volume` hands the boot volume's temp to programs macOS treats as their own responsible process, a declared
   exception for TCC (`tools/paddock-dev/src/ci_host/boot_volume.rs`) that stays declared.

### Cleanup

1. End the ledger's processes by identity (B), then the marker net (`processes stop`, branch).
2. Job isolation release: the socket folder, the slot.
3. Remove the job's folder contents that are not declared outputs (the runner removes the folder itself afterwards).
4. Write the job summary to `$GITHUB_STEP_SUMMARY` (ER-27): what ran, what was skipped and why, what cleanup ended (pid, command,
   group), and what it could not see.
5. Always exit 0, except when the slot's holder died during the job (today's rule in `release`, ADR 0025).

### B: the job ledger

The runner already makes each step a group leader on Unix. Bash sources `BASH_ENV` in every non-interactive bash, so each
`shell: bash` step appends `pgid leader-pid leader-start` to `<job temp>/groups`. I measured that Apple's bash 3.2.57 sources it
under the runner's `--noprofile --norc -e -o pipefail`. No workflow or composite step changes.

Cleanup reads the process table once (`paddock_proc::own::look`, on main, gives each row's group). A row is a member when its group
is in the ledger, it runs as this user, and it started at or after that group's leader. Descendants of a member that left its group
are followed by age (rule 53). Each member is ended by identity through `paddock_proc::terminate`; nothing signals a group id
(`killpg` stays banned). The ledger is a hint file in rule 53's sense: the table proves membership before anything is acted on. POSIX
does not reuse a process group id while the group has members (XBD 4.17), and the start-time test covers a group that emptied and
whose id came back. The slot holder leaves the step's group by design and is spared, as today.

Per-step ending was rejected: it would end what a step starts on purpose for later steps (the job's sccache server from
setup-rust).

Markers stay as the second net: a process that called `setsid` and is no descendant of a member, a non-bash step (a node action),
and the next job's acquire sweep for a runner crash or a hard kill (branch, `processes sweep`).

On Windows the runner's job object is the mechanism; WP-8 measures it on the real runner. On Linux the container is.

### Checks (report-only first, ER-41)

- **Fixture check** (WP-5): every job in a Paddock workflow, on every self-hosted runner class, uses `./.gitea/actions/job` after its
  checkout and before any other step that starts a process or names a kept folder; a job that calls a reusable workflow is checked in
  the callee. It reads workflows through the one workflow reader in `paddock-dev` (ER-42), not by text. Positive control: a fixture
  workflow with one job without the action, and one with it after a kept-folder step, both red. It blocks after its first green run
  on the real tree, recorded by run id. When it blocks, the host-job part of `conformance/test/job-isolation.test.mjs` retires.
- **Handle check** (WP-6): in scripts, workflows and actions, flags `&` whose `$!` is not saved within the next lines, `pgrep`,
  `pkill`, and `ps` piped to `grep`. Its exceptions file only shrinks (ER-46). Positive control: a fixture script with each shape, red.

## Decision: one log bundle per run

Added by the user on 2026-11-21. The job's folder (ER-51) also holds its logs, so a failed run can be read from one bundle.

**The format is JSON lines**, `paddock-log`'s CLEF, one event per line, under one declared schema (below). The user asked on
2026-11-21 that the logs hold every detail a failure needs (ER-27).

### The log schema

The schema is one declared file, `conformance/ci/log-schema.toml`. The validator and the reference doc
(`docs/ci-log-schema.md`) are generated from it (ER-40), and a test fails when either differs from what the schema generates.

**Every line carries:** the time (UTC, milliseconds) and the level; the run id, the workflow, the job, the runner name, its OS and
architecture, the attempt, the step and the stream (`stdout`, `stderr` or `event`); the commit sha and the ref; the message.

**Event lines** (`stream = event`) carry a `kind` and its fields:

| Kind | Required fields |
| --- | --- |
| `job-start` | The environment: tool versions (rustc, cargo, node, nextest, sccache), the cache backend and the lane, the job folder, the load and free disk at the start |
| `phase-start`, `phase-end` | The phase name; at the end its duration and outcome |
| `spawn` | The command with secrets masked, the working folder, the pid plus start time (the ER-51 handle), the exit code (with the NTSTATUS on Windows), the duration, and the stderr tail when it fails |
| `test-result` | The name, `file:line`, the attempt (1 or 2), the outcome (`pass`, `fail` or `FLAKY`, ER-12), the duration |
| `error` | The cause chain, `file:line`, the next step |
| `wait` | What it waits on, the bound, the outcome, the time waited split into queue and run |
| `cache` | The key, hit or miss, the size |
| `job-end` | What ran, what was skipped and why, and the totals |

**Rules:**
- No user content. Every line goes through `paddock-log`'s `Redactor` (`crates/paddock-log/src/redact.rs`) before it is written, so
  secrets are masked.
- A field that is not known is written as `"unknown"` with a reason (`{"value": "unknown", "why": "..."}`), never left out (ER-33).

**The check:** `./dev ci log check` runs in cleanup over the job's lines and fails the job when a line or an event lacks a required
field. It starts report-only (ER-41): a `::warning::` per missing field and a count in the summary, blocking after its first green
run on the real tree, recorded by run id. Positive control: a fixture log with each kind missing one required field, and one line
missing a common field, each red; a complete fixture green.

- **Structured logs.** Prepare sets `PADDOCK_LOG_DIR=<job folder>/logs` and the ids `PADDOCK_LOG_RUN`, `PADDOCK_LOG_JOB` and, per
  step, `PADDOCK_LOG_STEP` (the step's id, set by the `BASH_ENV` recorder). Every Paddock tool in the job writes `paddock-log` events
  there: CLEF JSON lines (`crates/paddock-log/src/clef.rs`, Serilog's compact format), through the crate's redaction
  (`redact.rs`, `core.rs`), each tagged with the three ids.
- **Raw step output.** The `BASH_ENV` recorder also tees the step's stdout and stderr: the step's output still reaches the runner (the
  job log is unchanged), and a copy goes through `paddock-log`'s child reader (`child.rs`: bounded lines, a daily budget) into the
  same folder, one event per line with a timestamp, the step id and the stream. The redaction rules apply, plus the values of the
  secret variables the job declares; the runner masks `secrets.*` in its own log, but our copy only knows what it is told, so a
  value that is neither declared nor matched by a rule could reach the bundle. That is the main risk of this part; the bundle is a
  private artifact of a private repository.
- **The bundle.** Cleanup packs `logs/` into one artifact named for the job, and writes a merged, time-ordered view of the job
  (`run-view.clef`): one time-ordered file of the same JSON lines, so agents and tools can query it (`jq`, Debrief, a
  test) without the runner's web view. The upload uses the artifact client that `actions/upload-artifact` v3 uses, the version the workflows already
  run on our Gitea (pinned by digest, for example `app-release.yml:376`); the official v4 action refuses hosts it takes for GitHub
  Enterprise Server, and the Gitea-specific fork (`christopherHX/gitea-upload-artifact`) is a third-party action I do not adopt
  here. A post step cannot call another action, so the job action bundles that client (npm, MIT) in its own code. A last Linux job
  per workflow (`needs` every job, `if: always()`) downloads the job bundles and writes the run's merged view with
  `./dev ci logs merge`.
- **The sink is an interface.** `paddock-log` writes through a `Sink` trait with one implementation now, a file. A collector, or Loki
  behind the existing Grafana, can come later through a standard shipper that reads the files (for example Grafana Alloy; not
  evaluated for this ADR), not a server of ours.
- **No log server in CI now.** A server is one more long-running process to start, hold, reach and end in every job, from
  containers and from the Windows runner, which is exactly what this ADR removes (ER-51). Files in the job's folder need none of
  that, and the runner already removes the folder.
- **Later option, not built now: a per-run live server.** The user may pick it later: one log server per run, fed by the same JSON
  lines, so a run can be watched while it runs. Cost about 3 to 4 days, mostly the parts this ADR avoids: reaching it from the Linux
  containers and from the Windows runner, starting and ending it with the run (its handle, its lifetime across jobs), and what happens
  when it is down. The file format stays the contract, so nothing written now changes if it comes.
- **Follow-up, not in this ADR:** Debrief opens a run bundle (it already opens CLEF logs).

## Comparable practice (ER-37)

| Software | Practice | Source (read 2026-11-21) |
| --- | --- | --- |
| xUnit (JUnit, NUnit, pytest) | A fixture pairs setUp and tearDown; tearDown runs when the test fails. pytest's yield fixture keeps both halves in one function. | [JUnit 5 user guide, lifecycle methods](https://junit.org/junit5/docs/current/user-guide/#writing-tests-definitions); [pytest yield fixtures](https://docs.pytest.org/en/stable/how-to/fixtures.html#yield-fixtures-recommended) |
| GitHub Actions | JavaScript actions declare `runs.pre`, `runs.main`, `runs.post`; "The `post:` action always runs by default", and "If not set, then `post-if` defaults to `always()`." Runner: on Windows it walks the tree on cancel, on Unix kills the direct child (`ProcessInvoker.cs`); at job end `FinalizeJob` kills processes whose environment has the job's `RUNNER_TRACKING_ID`, switched off by the job variable `process.clean` (`JobExtension.cs`). | [metadata syntax, runs for JavaScript actions](https://docs.github.com/en/actions/reference/workflows-and-actions/metadata-syntax#runs-for-javascript-actions); [ProcessInvoker.cs](https://github.com/actions/runner/blob/main/src/Runner.Sdk/ProcessInvoker.cs); [JobExtension.cs](https://github.com/actions/runner/blob/main/src/Runner.Worker/JobExtension.cs) |
| Gitea runner (ours) | Per-step group (Unix), per-job job object (Windows), `RUNNER_TRACKING_ID` kill (Unix); `always()` steps and post steps run on cancel. | [host_environment.go](https://gitea.com/gitea/runner/src/tag/v6.0.1/act/container/host_environment.go), [internal/pkg/process](https://gitea.com/gitea/runner/src/tag/v6.0.1/internal/pkg/process), [job_executor.go](https://gitea.com/gitea/runner/src/tag/v6.0.1/act/runner/job_executor.go) |
| GitLab Runner | `after_script` runs after `script`, on a `script_failure` and, since GitLab 17.0, when the job is cancelled during `script`; not after a timeout by default. Unix kill: SIGTERM then SIGKILL to the process group; Windows job object behind `FF_USE_WINDOWS_JOB_OBJECT` (default off). | [after_script](https://docs.gitlab.com/ci/yaml/#after_script); [killer_unix.go](https://gitlab.com/gitlab-org/gitlab-runner/-/blob/main/helpers/process/killer_unix.go); [feature flags](https://docs.gitlab.com/runner/configuration/feature-flags/) |
| Buildkite | The `pre-exit` hook "Runs before the job finishes. Useful for performing cleanup tasks." The docs warn: "On canceled or timed-out jobs, it might not run or finish." cancel sends `cancel-signal`, then SIGKILL after `cancel-signal-timeout`. Its group or job-object handling is not in the docs; I did not read its code (unverified). | [agent hooks](https://buildkite.com/docs/agent/v3/hooks); [agent configuration](https://buildkite.com/docs/agent/v3/configuration) |
| systemd | A unit is a cgroup; `KillMode=control-group` (the default) kills "all remaining processes in the control group of this unit" on stop. | [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html) |
| Microsoft | `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` ends every process of the job when its last handle closes; children join their parent's jobs unless breakaway is allowed; terminating a nested job ends its child jobs. | [JOBOBJECT_BASIC_LIMIT_INFORMATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information), [Nested jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs) |
| POSIX, macOS | `setsid`, `killpg`; "A process group ID shall not be reused by the system until the process group lifetime ends." macOS has no cgroups; launchd kills the remaining members of a dead job's process group unless `AbandonProcessGroup`, which is the runner's group, not a step's. | [setsid](https://pubs.opengroup.org/onlinepubs/9799919799/functions/setsid.html), [XBD 4.17](https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/V1_chap04.html); `man 5 launchd.plist` on macOS 27.2.1 |

The fixture shape is common practice (xUnit, GitHub's pre/post, GitLab's `after_script`, Buildkite's `pre-exit`). A job-scoped
group on macOS is not done by any of these; it is what closes our measured gap.

## Options and costs

Agent-days. Baseline: the markers on the branch (built, about 1.5 days spent, not counted).

| Option | What | Days |
| --- | --- | --- |
| A | Markers only, plus the Windows control | 0.5 |
| B + D (accepted first, then dropped) | Ledger plus runner host hooks (`job_completed`, `post_task_script`) | 3.0 (3.5 with D5) |
| B + E (after the user dropped D) | Ledger plus a shared `if: always()` cleanup step and its check | 3.0 (3.5 with D5) |
| Fixture (accepted first) | One job action (prepare and post cleanup) holding B, isolation, health, folder and summary; owner trait; two checks; D5; migration of every job | 6.5 |
| **Fixture plus log bundle (accepted)** | The above, plus structured logs, the raw output tee, the bundle, the run view, and the declared log schema with its validator | **9.5** |
| C | Upstream runner change | issue only (D4); a fork rejected |

The accepted scope is 9.5 days (6.5 for the fixture, 3.0 for the log bundle and its schema) against 3.5 for B + E with D5: 6.0 more, 171% growth. Most of it is the migration of 96 jobs
(19 on host runners, 77 in Linux containers; `git grep runs-on` on main) and the handle check. ER-37 asks that growth over 20% goes
back to the user: the user asked for this scope on 2026-11-21, and the cheaper option is still open as D6.

## Work packages

Each WP owns its files, so implementers can work in parallel. `.gitea/` files are shared integration points: the implementer
prepares the change on its branch and the integrator merges it (AGENTS.md, Ownership). A new check id comes from
`./dev check new-id --reserve <branch>`.

| WP | What | Owns | Days | Needs |
| --- | --- | --- | --- | --- |
| WP-1 | `Owned` trait and `own::Unit` (groups by leader identity, marker, start time, spared identities) with the pure `members(table, unit)`; unit tests on rows; known-copies entry naming the Reaper, the job ledger and `host-cleanup`'s patterns. | `crates/paddock-proc/src/owner.rs` (new), one line in `crates/paddock-proc/src/lib.rs`, `crates/paddock-proc/tests/owner.rs`, `docs/known-copies.toml` (one entry) | 0.5 | none |
| WP-2 | The Reaper on `Unit`, its tests green. | `crates/paddock-testkit-start/src/reap.rs` | 0.25 | WP-1 |
| WP-3 | Job ledger: the `BASH_ENV` recorder, the reader, `processes stop` ends ledger members first. Stand-ins: a member that ignores SIGTERM, an Apple-binary member that moved folder (the gap), a `setsid` child of a member; a second job's stand-in as the positive control that must survive. | `conformance/ci/job-group.sh` (new), `tools/paddock-dev/src/ci_isolation/ledger.rs` (new), `tools/paddock-dev/src/ci_isolation/processes.rs`, `tools/paddock-dev/tests/it/job_processes.rs` | 1.0 | WP-1; `feat/lessons-check-and-job-cleanup` landed |
| WP-4 | `./dev ci job prepare` and `cleanup` and the job action; first measure the two preconditions (Node on the host runners, `post` after stable-workspace) and, if either fails, build the fallback. Prepare: health, reservation read, acquire and sweep, `BASH_ENV`, the job folder; cleanup: ledger end, release, folder, summary. | `tools/paddock-dev/src/ci_job/` (new), `tools/paddock-dev/tests/it/ci_job.rs`, `.gitea/actions/job/` (new, via the integrator) | 1.0 | WP-3; train 18 (health, reservation); ADR 0111 branch |
| WP-5 | Fixture check, report-only, with its positive controls and fixtures. | `tools/paddock-dev/src/check/c<n>.rs`, `tools/paddock-dev/tests/it/check_c<n>.rs`, its fixtures, one row in `docs/gates.toml` | 0.5 | WP-4 (the action's name) |
| WP-6 | Handle check, report-only, with its positive control and a shrink-only exceptions file; confirm whether any dispatch caller looks its run up again, and make `ci dispatch` print the run id if it does not. | `tools/paddock-dev/src/check/c<m>.rs`, its test and fixtures, its exceptions file, one row in `docs/gates.toml`, `tools/paddock-dev/src/ci_runs/dispatch.rs` (only the run id) | 0.5 | none |
| WP-7 | Slot holder bound to its starter: the holder also ends when its starter's `ProcessId` has ended (failing test first: an agent-like starter ends, its holder must follow). First commit: the `slots.rs` holder note names both runner job objects (the step's, which does not kill on close, and the job's, which `Remove` terminates; `group_windows.go`) and says macOS has no such unit (ADR 0112). | `tools/paddock-dev/src/slots.rs`, `crates/paddock-slots/src/`, `tools/paddock-dev/tests/it/slots.rs` | 0.5 | WP-1 |
| WP-8 | Positive control on the real Windows and macOS runners: a step leaves a stand-in that writes a heartbeat outside the job folder; a second job checks the heartbeat stopped at the first job's end (Windows: by the runner; macOS: by cleanup). | `conformance/ci/probes/job-unit-probe.sh` (new), `.gitea/workflows/job-unit-probe.yml` (new, dispatch only, via the integrator) | 0.25 | WP-4 |
| WP-9 | Migration batch 1: every job of `ci.yml` uses the job action; acquire and release steps go. | `.gitea/workflows/ci.yml` | 0.5 | WP-4 |
| WP-10 | Migration batch 2: `debrief.yml`, `debrief-*.yml`, `dyno-screenshots.yml`, `service-smoke.yml` (its `macos-vm` job has no isolation today), `debrief-runner-check.yml` (its `host` job has none either). | those files | 0.5 | WP-4 |
| WP-11 | Migration batch 3: `app-release.yml`, `heavy-release.yml`, `app-fuzz.yml`, `nightly.yml`, `dev-tool.yml`, `testcommand-ab.yml`, the demo releases and the remaining Linux-only workflows. | those files | 0.75 | WP-4 |
| WP-12 | Docs: ADR 0025 and ADR 0103 amendments, `.gitea/actions/README.md` (runner versions, the job action). | `docs/adr/amendments/0025-*.md`, `docs/adr/amendments/0103-*.md` (new), `.gitea/actions/README.md` | 0.25 | WP-4 |
| WP-13 | Log setup: the `Sink` trait with the file sink; the common fields from the schema filled by `paddock-log` (runner, OS, attempt, sha, ref from the runner's variables, `"unknown"` with a reason when absent); the CI ids (`PADDOCK_LOG_RUN`, `_JOB`, `_STEP`) and `PADDOCK_LOG_DIR` read by `paddock-log`'s config; prepare exports them. | `crates/paddock-log/src/sink.rs` (new), `crates/paddock-log/src/ci.rs` (new), one line in `crates/paddock-log/src/lib.rs`, `crates/paddock-log/tests/ci.rs` | 0.5 | WP-4 (prepare) |
| WP-14 | Raw output tee in the `BASH_ENV` recorder through `paddock-log`'s child reader, with redaction and the declared secret names; a test that a declared secret's value never reaches the folder (positive control: the same test without the declaration, red). | `tools/paddock-dev/src/ci_job/tee.rs` (new), `conformance/ci/job-group.sh` (after WP-3), `tools/paddock-dev/tests/it/ci_job_tee.rs` | 0.5 | WP-3, WP-13 |
| WP-15 | Bundle and views: cleanup packs `logs/` and writes the job's merged view; the job action uploads it with the bundled v3 artifact client (npm dependency in the root lockfile); `./dev ci logs merge` for the run view; the last `logs` job is added to each workflow by WP-9 to WP-11. | `tools/paddock-dev/src/ci_logs/` (new), its tests, `.gitea/actions/job/` upload part (via the integrator), `package.json` and `package-lock.json` (one dependency, via the integrator) | 0.75 | WP-4, WP-13 |
| WP-16 | The log schema: `conformance/ci/log-schema.toml`, the generator of the validator and of `docs/ci-log-schema.md`, the generated-matches test (ER-40), `./dev ci log check` (report-only, ER-41) with its positive controls; the event kinds emitted by `ci job`, `ci run`, `./dev wait`, the test runners' result readers and `setup-rust` (cache). | `conformance/ci/log-schema.toml` (new), `docs/ci-log-schema.md` (generated), `tools/paddock-dev/src/ci_logs/schema.rs` and `check.rs` (new), the emitting call sites in `tools/paddock-dev/src/ci_job/`, `ci_run/`, `wait.rs`, `ci_host/setup_rust.rs`, their tests | 1.25 | WP-13, WP-15 |

Total: 9.5 days (WP-1 to WP-12: 6.5; WP-13 to WP-16: 3.0). WP-1, WP-6, WP-7's test and WP-13's sink can start at once; WP-9 to
WP-11 run in parallel once WP-4 lands, one implementer each.

### Migration inventory (main, 2026-11-21)

Host jobs and what each does by hand today (acquire and release are the job-isolation steps; hc is host-cleanup; sc is
sccache-stats). Jobs that call a reusable workflow (`dyno-release`, `pitcrew-release`, `pitwall-release`, `spotter-release`,
`nightly`'s installers and fuzz jobs) are covered through `ci.yml`, `app-release.yml` and `app-fuzz.yml`.

| Workflow | Job | Runner | acquire | release | hc | sc |
| --- | --- | --- | --- | --- | --- | --- |
| app-release.yml | bundle | matrix | yes | yes | yes | yes |
| ci.yml | e2e-webkit, tools-macos | macOS | yes | yes | no | no |
| ci.yml | crate, paddock-dev | matrix | yes | yes | no | yes |
| ci.yml | app | matrix | yes | yes | yes | yes |
| debrief.yml | rust-test-macos, tauri-smoke, bundle | macOS, matrix | yes | yes | no | yes |
| debrief-runner-check.yml | host | matrix | **no** | **no** | no | no |
| dev-tool.yml | publish, publish-macos | matrix, macOS | yes | yes | no | no |
| dyno-screenshots.yml | screenshots | macOS | yes | yes | no | no |
| heavy-release.yml | build-host | matrix | yes | yes | no | no |
| nightly.yml | dialog-window-probe | matrix | yes | yes | no | yes |
| service-smoke.yml | macos-vm | macOS | **no** | **no** | no | no |
| shell-demo-release.yml, workbench-demo-release.yml | bundle | matrix | yes | yes | no | no |
| testcommand-ab.yml | ab | Windows | yes | yes | no | yes |

None does the health gate, the ledger, the folder or the summary in one place today; those come with the action.

## Decisions for the user

- **D1-D5:** taken (see Status). D2 is not needed: the end runs in the job action's post step, inside Paddock.
- **D6.** The accepted scope is 9.5 days against 3.5 for B + E with D5 (171% growth, ER-37). Keep the full scope, or build
  B + E now and the fixture after the releases? Recommended: the full scope, because the migration is mechanical and parallel.
- **D7.** Linux container jobs: migrate them too (in WP-9 to WP-11, mostly for the summary and the health gate), or only host jobs?
  Recommended: all jobs, as the user asked; the container already ends their processes.

## Consequences

- A job's leftovers end with the job on every runner class. A workflow that meant one to outlive the job (none known) breaks; that
  was already against ER-14.
- A job's temp files live in one folder that the runner removes; the user temp folder stops filling up.
- The branch's "not measured" on Windows becomes a measured answer (WP-8), not a pass by assumption (ADR 0089).
- Escapes stay possible on macOS: a process that calls `setsid` from a non-recorded step and clears its environment. The summary
  reports what cleanup could not see; the next acquire's sweep reports leftovers.

## Status of the mechanisms (ADR 0096)

| Mechanism | Where |
| --- | --- |
| `paddock_proc::own::{look, group_of, own_group, KillOnClose}`, `terminate`, `kill_tree`, `ProcessId` | `origin/main` |
| Clippy ban on `libc::killpg`, C53 | `origin/main` |
| `Reaper` | `origin/main`, `crates/paddock-testkit-start/src/reap.rs` |
| `./dev wait` with `--pid`, `--ci-run`, `--plock`, `--max` | `origin/main` |
| `own::survey`, `processes stop` and `sweep`, `PADDOCK_JOB_TEMP` export | `feat/lessons-check-and-job-cleanup` 9dfa4e6dc, a prerequisite |
| Health gate and no-prompts, joint reservation | `origin/train/2026-11-21-18` 75ce0d2c8, a prerequisite |
| Lane folders (ADR 0111) | `origin/fix/ci-no-shared-lane-folders` 18ecd84a4, a prerequisite |
| Scratch-root item | queued, not on a branch; folded into WP-4 |
| C143 spawn lint | cited by the coordinator; not read for this ADR |
| Runner per-step group, Windows job object, `RUNNER_TRACKING_ID` kill, post steps on cancel | gitea/runner v5.4.1 (Mac) and v6.0.1 (Windows, Linux), external |
| `paddock-log` CLEF writer, redaction, child-output reader | `origin/main`, `crates/paddock-log` |
| `actions/upload-artifact` v3 on our Gitea | `origin/main`, used in the workflows (pinned by digest) |
| `Owned`, `own::Unit`, ledger, `job-group.sh`, the job action, `ci job`, both checks, the probe, the log `Sink`, the tee, `ci logs`, the log schema and `ci log check` | not built (this ADR) |
