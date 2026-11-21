# Amendment to ADR 0087 (2026-11-16, draft): the other-OS member gate runs on the real CI runners

- Status: Proposed 2026-11-16. **Decided by the user on 2026-11-16: real runners**, as merge queues and merge trains do. The step
  is built and report-only (ER-41); enforced 2026-11-21 (see below).
- Trigger: train 12 spent about 17 hours in three red CI rounds, and every failure was Linux- or Windows-only: run 8138 (23
  failures: a hidden PATH, `/srv` paths that are not absolute on Windows, a lost Git-bash PATH, slow process starts, ext4 folder
  blocks), run 8147 (a `#!/bin/sh` fixture Windows cannot run), run 8152 (TestCommand 6x slower on Windows). All of them passed
  the Mac member gates. Goal: a member's affected tests run on Linux and Windows BEFORE it joins a train.
- Related: ER-9, ER-24 and the user's D-1 (2026-11-15: one train's CI per single-machine runner class), ER-33 (a row says why it
  did not look), ER-36, ER-41 (report-only first), ER-42, ER-37 (common practice and cost), rule 55 (one copy of "what does a
  change reach"), ADR 0087 and its amendment of 2026-11-13 (`run --ci-like`), ADR 0098 (member gates, trial refs).

```evidence-heads
read: 2026-11-16
origin/train/2026-11-15-13 13881087a
```

## Why not an emulation of the runners (the first version, measured)

The first version ran the member's tests in a container on the Arm Mac (`linux/amd64` under emulation, ext4 loop mount, fresh
clone) and on the Windows test VM (Arm, x64 programs under Prism, a runner-style step). It worked as a mechanism (a planted
Linux-only and a planted Windows-only failure were each red on their OS) but it gave **false findings**:

- On a base where the real Windows runner is green, 22 `paddock-dev` tests failed on the VM and one timed out (1421 of 1444
  passed): `/srv` paths that are not absolute (`places`, `wrapper`, `queue`), `grep: command not found` under the VM's runner-style
  PATH, `TotalPhysicalMemory` unreadable for a standard user. They are differences between the VM and the runner (account, PATH,
  CPU, architecture), not findings in the member.
- The native `linux/arm64` image had no pinned Chromium (a false conformance failure); `linux/amd64` ran under emulation, where a
  failed exec may not be reported.
- The VM needed the Build Tools in its image, a per-run toolchain copy, Node, a pinned Arm nextest (the x64 one does not start),
  and a 13 to 15 minute cold build for `paddock-dev`, and it blocked the single VM lock for every other lane.
- A review of that version also found that selection by file heuristics missed OS-specific crates and their dependents, and that
  failures already red on the base were reported as the member's own.

A run on the real runners removes the false findings by construction (same machines, same account, same image as CI), and CI's
own planner already selects the jobs.

## The gate: `other-os`, a row of `./dev merge check --gates`

`tools/paddock-dev/src/merge_check/other_os.rs`, after the fast gates, for a merge commit (`ENFORCED = true` since 2026-11-21):

1. **Selection is the plan's.** `./dev plan <train before> <merge>`; a member that reaches no project is `not run (<why>)`.
   There is no second copy of "what does a change reach" (rule 55); the earlier path heuristic and `select()` are gone.
2. **Push and dispatch.** The merge commit is pushed to the short-lived ref `trial/member-<sha12>` (the `trial/*` namespace of ADR
   0098 WP-4, cleaned by this step and by `./dev ci trial-prune`) and `ci.yml` is dispatched on it with `base` = the train before,
   so CI's planner runs only the member's affected jobs. The dispatch is `ci dispatch`'s own: it waits, bounded (45 minutes), while
   another run holds the macOS or Windows runner, then reports `not measured` (ER-24, D-1).
3. **One bounded wait** (`./dev wait --ci-run`, at most 2 hours), then the jobs and `ci triage` are read.
4. **Only new failures fail a row.** `ci triage` compares each failed test with the last main run and the flake history; a failure
   also on main is listed (`alsoOnMain`), not blamed; an unknown one is `not measured`, never a pass.
5. **Rows are the shared types (C124):** per OS, `measure` makes `Clean` (evidence for this run) or `Findings` (the new
   failures); `NotMeasured` carries the reason; `NotApplicable` says the plan ran no job on that OS. The Linux row is the run's
   `ubuntu-latest` jobs and the Windows row its `windows-latest` jobs, named by their matrix label.

`ci verdict` is not the reader here: it is the one reader of FULL runs and refuses a base dispatch by design (a partial run
"never counts"). The pass or fail of a partial member run is this step's, built on the same `Client` and `triage` readers the train
queue uses.

Needs `GITEA_TOKEN` (write access: it pushes the scratch ref and dispatches `ci.yml`). The trial's scratch folder is
`<trial root>/<branch>-<sha12>`: the trial root is `PADDOCK_TRIAL_DIR`, else git config `paddock.trialRoot`, else `trials` beside
`paddock.worktreeRoot` (never inside HOME, ER-14); the step makes the folder and removes it at the end (ER-51).

### Enforced since 2026-11-21 (user: "member checks on real runners, build it next")

The gate was report-only (ER-41) and, with `PADDOCK_TRIAL_DIR` unset, printed "NOT MEASURED" and let the merge go on, so no
train 18 member was ever checked on Windows or Linux; a Windows-only failure (the `sccache_eviction` tests, runs 8495 and 8509)
was found only after the merge. Now `ENFORCED = true`:

- `merge check --gates` on a merge commit dispatches the trial itself whenever `GITEA_TOKEN` is set, with the same code path as
  `ci dispatch` (reservation, `Sent::Busy` bound), and waits on the run id with a bound (ER-32).
- What it could not measure (no token, no trial folder, the runner reservation not had within 45 minutes, a run that did not end
  in 2 hours, a failure triage cannot blame) is `not measured` and exits 6; a NEW failure on one OS exits 5. The output names
  the cause and the next step. A member the plan says reaches no project is `not run`, with no token needed.
- Judged per test by the train's rules (ER-12): a test that failed both tries blocks. A FLAKY test (failed, then passed on its
  one retry) blocks only when the member's change touches its feature (`feature_touched`: a changed file or folder named like
  one of the test's modules; a test whose module cannot be told counts as touched, so the rule errs toward blocking); in an
  untouched feature the row passes and the flake is listed as `FLAKY (recorded, not blocking ...)` and in `flakyRecorded`, a fix
  item for the next train.
- Logged (ER-27): the run id, the verdict per OS and the run's wall time split into queue wait (run created to job start), setup,
  compile and tests (the jobs' step timings, by step name; job-seconds summed per OS, the queue wait the longest) in the step's
  text and `data.rows[].phases`.

## What stays of the VM work

The Windows test VM remains for GUI and installer tests. Kept: the snapshot `buildtools-baseline` (clean-baseline plus the Visual
Studio Build Tools 2022 17.12.37502.7; rollback snapshot `pre-buildtools-2026-11-16`; install 540 s, about 8.3 GB), `vm-win.sh
toolchain-setup`, `vm-win.sh run --ci-like --tools-provided`, and `scripts/member-gate-windows.sh` and its lock file, now **marked
for manual use** (one package natively on the VM for a Windows-only investigation, with the VM's own differences listed above).
Removed: the Linux container scripts and `--loop-mount` of `tools/linux-container.sh`, and the path selection.

## Cost (ER-37)

Common practice is what the user chose: GitHub merge queues and GitLab merge trains run the member's merge result on the real
runners before it lands. Build cost of this rework: about 1 agent-day (the first version was about 4). Run cost per member: the
affected jobs of the plan on the real runners, for a `crates` change the whole workspace on Linux, macOS and Windows (about 35 to 45
minutes on the Windows runner), queued behind a busy runner by D-1. That is minutes of CI, not of an agent. A docs-only member
costs nothing (`not run`).

## First runs (2026-11-16, report-only)

**Run 8166** (a real member merge touching only `crates/paddock-view`, dispatched by this step on `trial/member-0e5fd6446864` after
waiting 31 minutes 53 seconds behind a release run on the macOS runner) was dispatched correctly and then **cancelled by me on the
coordinator's order**: it had slipped into the gap between two workflows of train 12's release sequence and train 12's Pitwall gate
was waiting behind it. So there is **no green or red real-runner result yet**; the planted red run and the green run are to be
made after train 12 has landed, on the integrator's say (ER-1: the release goes first). What was proven: the plan selection, the
push to the scratch ref, the dispatch with `base`, the wait behind a busy macOS runner (D-1), the run's creation, and the scratch
ref's cleanup. The rows, the NEW-only blame and every `not measured` path are covered by unit tests with a fake CI
(`other_os.rs`: a planted new failure on Windows is red there and green on Linux; a busy runner, a still-running run, an
unreadable plan are `not measured`).

## The train reservation (the gap that 8166 showed)

`ci dispatch`'s runner guard sees only a busy runner, so a member run can take the gap between two workflows of a train's CI
sequence. Fixed as a design rule, not a timing slip: **the integrator's sequence holds a reservation for its whole duration**, and
every dispatch that is not a train's waits on it.

- `scripts/train-ci-hold.sh <label> -- <the sequence>` holds one kernel file lock under `<lock root>/train-ci` (the machine's lock
  mechanism of `scripts/vm-lock.sh`: freed when the holder dies; wrap the sequence in `timeout 8h`, so the hold is bounded by its
  command). `--status` shows the holder.
- `ci dispatch` of a non-train ref (and `--trial`, and the member gate) calls `ci_runs/reservation.rs` first: it waits while the
  lock is held, polling every 15 s, at most `--max` (the member gate: 45 minutes), and then does not dispatch: `ci dispatch` fails
  with "a train's CI sequence holds the runners", the member gate is `not measured`. A `train/*` dispatch never waits on it; an
  unreadable lock is an error, never "free". A test (`reservation.rs`) shows a member dispatch waiting while the reservation is
  held and going on when it is released, and being reported still held at the bound.
- Checked by hand: with the hold taken, `./dev ci dispatch ci.yml --ref <anything> --max 10s` printed the wait line and refused,
  and dispatched nothing.
- **The integrator must wrap each train's sequence in `train-ci-hold.sh`** (brief template change; the rule rests on the
  integrator until a hook exists). A member run that is already running when a sequence starts is not stopped: the train's own
  runner-wait waits for it, as before.
