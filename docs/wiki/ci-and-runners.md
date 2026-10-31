---
title: CI and runners
verified: 2026-10-31
sources: [.gitea/workflows/ci.yml, rule 50, ADR 0025, handoff/merge-steward.md, handoff/runners-2026-10-28.md, handoff/linux-ci-share-ab.md, handoff/windows-runner-update-policy.md]
---
How a change reaches CI today, which machines run it, why the single Mac is the queue, and which failures are known flakes.
Origin/main at `233c1339b` (2026-10-31). The rules for CI economy are rule 50; this page links them and does not restate them.

## Current state
- ci.yml starts with a plan job that runs `scripts/affected.mjs` and runs only the projects a change reaches; the base of the
  diff is a pull request's merge base, the last green push run of main, or the `base` input of a manual run; a manual run
  without a base runs everything. Source: [ci.yml header](../../.gitea/workflows/ci.yml).
- `nightly.yml` runs everything with WebKit every night as the safety net for a wrong filter. Source: [ci.yml header](../../.gitea/workflows/ci.yml).
- ci.yml has a concurrency group of workflow, ref and event with cancel-in-progress, so a second dispatch on the same ref
  cancels the first. Source: [ci.yml](../../.gitea/workflows/ci.yml); the same remark in src: `handoff/linux-ci-share-ab.md`.
- The economy rules (one run per branch head, no refresh dispatches, docs-only changes skip CI) are rule 50: [CI economy](../rules.md#ci-economy-user-2026-10-29). Why it exists: [lesson of 2026-10-29](../lessons.md#stale-duplicate-and-global-trigger-ci-runs-wasted-the-runners-2026-10-29).
- Three kinds of runners: Linux jobs run in the `paddock-ci:v2` container on the Linux host `d-gtr9` (capacity 4, 16 cores,
  about 128 GB at the 2026-10-30 measurement); Windows jobs run on the host runner `decryp7ology-dev`; macOS jobs run on the
  Mac mini's local gitea-runner. Source: [ci.yml header](../../.gitea/workflows/ci.yml); src: `handoff/linux-ci-share-ab.md`; src: `handoff/windows-runner-update-policy.md`; src: `handoff/runners-2026-10-28.md`.
- The Mac is the bottleneck: macOS jobs of a batch queue because the single Mac runner is busy (steward triage 2026-10-31
  11:56), installer dispatches wait until the Mac queue clears (steward 2026-10-31 23:35), and its load ran at 31 to 50 for an
  hour and spiked once to about 700 (a process storm with low CPU; steward 2026-10-31 12:24 and 13:39). Source: src: `handoff/merge-steward.md` (those entries).
- Two more Mac runners (`d7-mac-mini-2`, `-3`) were registered on 2026-10-28 and parked offline until job isolation landed;
  whether they were enabled afterwards is not recorded. Source: src: `handoff/runners-2026-10-28.md`. Job isolation itself is in main as
  [the job-isolation action](../../.gitea/actions/job-isolation/action.yml) and [ADR 0025](../adr/0025-ci-job-isolation.md) (its status line on main still reads Proposed).
- Dispatching a batch: the steward merges the approved branches into `steward/sNN-batch`, then POSTs to Gitea's
  `actions/workflows/ci.yml/dispatches` with the batch ref and `inputs.base` set to the merge base with origin/main, and
  dispatches the app's own workflow too when the plan marks the app (for example `debrief.yml`). It lands on main by
  fast-forward only when the runs are green. Source: src: `handoff/merge-steward.md` (header line, steward 17 entry of 2026-10-31, entry of 2026-10-31 21:30).
- A change that touches an app's release path also needs a manual `<app>-release.yml` run that builds the installers, before it lands:
  [Release path](../rules.md#release-path-user-2026-10-30). See [releasing](releasing.md).
- Caches: host runners check out into a per-job hashed folder and sccache keys a compile on its folder, so the
  `stable-workspace` action builds every time from the same physical path. Source: [ci.yml](../../.gitea/workflows/ci.yml) (comment above the `stable-workspace` step).
  The family doc says sccache gives almost nothing across worktrees. Source: [AGENTS.md](../../AGENTS.md) (Build folders).
- Linux jobs leave `CARGO_BUILD_JOBS` unset, so each job starts one rustc per core; an A/B of 16, 8 and 4 jobs per job is
  planned and not run. Source: src: `handoff/linux-ci-share-ab.md` (branch `ci/build-parallelism`, head `eb2acadcc`).
- Keep CI machines on the stable OS channel (ER-26). A preview Windows update (KB5127653) changed how TCP listeners are
  reported and broke CI; the lesson is [newer Windows 11 builds list a listener with dwState 0](../lessons.md#2026-10-30-newer-windows-11-builds-list-a-listener-with-dwstate-0-the-table-says-it-listens-dyno).

## Machine load and the heavy-step lock (agents on the Mac)
- Agents gate heavy steps on a 1-minute load below 30; the scale-to-the-machine rule is rule 41 and ER-24. Source: [Throughput, rule 41](../rules.md#throughput-user-2026-10-24); src: `handoff/merge-steward.md` (entry of 2026-10-31 12:24).
- Heavy steps (cargo, npm, ng, playwright, llvm-cov, xwin) take turns through the lock tool `plock`, one at a time:
  `plock run [--high] [--no-gate] <agent-name> -- <command>` takes the lock, waits for the load gate, runs, releases; `plock status` shows the holder. Source: src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock` (header; a scratch tool outside the repository).
- The lock is a directory that appears atomically with its owner record (pid, start time, name) inside; it is broken only by the tool, only
  when the recorded pid is gone or its start time differs, and each break is written to a log. Never remove it by hand. Source: src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock` (header and `break_if_stale`).
- Why the tool exists: on 2026-10-31 an agent removed a stale lock by hand while its owner record was empty, which showed the older lock
  could be left without an owner. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 21:45 and 22:10).

## Known flakes (as the steward judged them in the log; re-check before waiving)
- Windows crate job, `paddock-chromium` `real_browser` tests (`a_cancelled_load`, `a_new_tab_loads_a_page_at_once`):
  `ERR_ABORTED`; waived in runs 7110 and 7123 (job 21598). Pid reuse was disproved as the cause by run 7158. Source: [ADR 0033 header](../adr/0033-processes-by-identity-never-by-a-bare-pid.md); src: `handoff/merge-steward.md`.
- macOS `real_browser` start timeout on the loaded Mac (runs 7213 and 7250 among others): the fixed 20 s start wait in the
  Debrief chromium driver; cured by the start judge, merged into batch s26b before it landed at `233c1339b`. Source: [lesson of 2026-10-31](../lessons.md#the-fixed-start-wait-hid-in-a-helper-named-connect-2026-10-31-debrief-chromium-driver); see [process-starts](process-starts.md).
- UI jobs with 5000 ms timeouts in later spec files: a leaked fake timer under `isolate: false`; see [test-isolation](test-isolation.md).
- Dyno `a_job_runs_at_the_gentle_level` EBADF: a retry (`c5c240c61`) is on main; the root cause (macOS socket sweep) has the
  approved fix `fix/ipc-poll-waits` at `c028d3f39`, not on main at the time of writing. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 08:21 and 09:52).
- Runs that died in seconds with an empty log or missing step scripts (6664, 6665, 6988, 7010) were re-dispatched; the log treats them as infrastructure, not code. Source: src: `handoff/merge-steward.md` (headers and entries of 2026-10-29).

## See also
[releasing](releasing.md), [test-isolation](test-isolation.md), [open-decisions](open-decisions.md).
