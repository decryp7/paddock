---
title: CI and runners
verified: 2026-10-31
sources: [.gitea/workflows/ci.yml, rule 50, rule 41, ADR 0025, handoff/merge-steward.md, handoff/runners-2026-10-28.md, handoff/linux-ci-share-ab.md, handoff/windows-runner-update-policy.md]
---
How a change reaches CI today, which machines run it, why the single Mac is the queue, and which failures are known flakes.
Origin/main at `233c1339b` (2026-10-31). The CI economy is rule 50 and the load rule is rule 41; this page links them and
does not restate them. Steward-log citations are `merge-steward.md:<line>` in `/Volumes/d7sst7/dev-targets/handoff/`.

## Current state
- ci.yml starts with a plan job that runs `scripts/affected.mjs` and runs only the projects a change reaches; the base of the
  diff is a pull request's merge base, the last green push run of main, or the `base` input of a manual run; a manual run
  without a base runs everything. Source: [ci.yml header](../../.gitea/workflows/ci.yml).
- `nightly.yml` runs everything with WebKit every night as the safety net for a wrong filter. Source: [ci.yml header](../../.gitea/workflows/ci.yml).
- ci.yml has a concurrency group of workflow, ref and event with cancel-in-progress, so a second dispatch on the same ref
  cancels the first. Source: [ci.yml](../../.gitea/workflows/ci.yml); src: `linux-ci-share-ab.md` (the same remark).
- What CI may run, and when, is rule 50: [CI economy](../rules.md#ci-economy-user-2026-10-29). Why it exists: [lesson of 2026-10-29](../lessons.md#stale-duplicate-and-global-trigger-ci-runs-wasted-the-runners-2026-10-29).
- Three runners exist in Gitea's admin list: the Linux host `d-gtr9`, the Mac mini's `d7-mac-mini` and the Windows host
  `decryp7ology-dev`. Linux jobs run in the `paddock-ci:v2` container on `d-gtr9` (16 cores and about 128 GB at the 2026-10-30
  measurement; it runs up to 4 jobs at once, as observed over 100 runs). Source: src: reviewer's check of the admin runner list, 2026-10-31; src: `linux-ci-share-ab.md` (Question); [ci.yml header](../../.gitea/workflows/ci.yml).
- Two more Mac runners (`d7-mac-mini-2`, `-3`) were registered on 2026-10-28 and parked; they were never enabled and are not in the
  admin list. Source: src: `runners-2026-10-28.md` (Mac mini section); src: reviewer's check of the admin runner list.
- Job isolation is in main as [the job-isolation action](../../.gitea/actions/job-isolation/action.yml); [ADR 0025](../adr/0025-ci-job-isolation.md) reads Proposed on
  main but is Accepted on `origin/pitcrew/1.1-i1` at `707300972`. Source: src: `git show origin/pitcrew/1.1-i1:docs/adr/0025-ci-job-isolation.md`; src: `merge-steward.md:93, 413` (Proposed on main).
- The Mac is the bottleneck: macOS jobs of a batch queue because the single Mac runner is busy (`merge-steward.md:524`) and the
  installer dispatches wait until the Mac queue clears (`merge-steward.md:658`). Source: src: `merge-steward.md:524, 658`.
- Load on the Mac came from agent and host work, not from CI: a test binary spinning for 70 minutes (`merge-steward.md:547`) and a
  process storm with low CPU (`merge-steward.md:533`); the log notes an hour at load 31 to 50 (`merge-steward.md:546`). Source: src: `merge-steward.md:533, 546, 547`.
- Dispatching a batch: the steward merges the approved branches into a batch branch named `steward/sNN-<subject>`
  (for example `steward/s17-batch`, `steward/s28-dyno-1.0`), then POSTs to Gitea's `actions/workflows/ci.yml/dispatches` with the
  batch ref and `inputs.base` set to the merge base with origin/main, and dispatches the app's own workflow too when the plan marks
  the app (for example `debrief.yml`). A stale run on an older head is cancelled first. It lands by fast-forward when the runs are
  green. Source: src: `merge-steward.md:3` (the POST), `merge-steward.md:118` (two workflows), `merge-steward.md:644` (cancel the stale run, dispatch on the new head), `merge-steward.md:279` (fast-forward landing).
- A change that touches an app's release path needs a manual `<app>-release.yml` run that builds the installers before it lands:
  [Release path](../rules.md#release-path-user-2026-10-30). See [releasing](releasing.md).
- Caches: host runners check out into a per-job hashed folder and sccache keys a compile on its folder, so the
  `stable-workspace` action builds every time from the same physical path. Source: [ci.yml](../../.gitea/workflows/ci.yml) (comment above the `stable-workspace` step).
  The family doc says sccache gives almost nothing across worktrees. Source: [AGENTS.md](../../AGENTS.md) (Build folders).
- Linux jobs leave `CARGO_BUILD_JOBS` unset, so each job starts one rustc per core; an A/B of 16, 8 and 4 jobs per job is
  planned and not run. Source: src: `linux-ci-share-ab.md` (branch `ci/build-parallelism`, head `eb2acadcc`).
- The OS update channel of CI machines is ER-26 (engineering-rules). The incident behind it: [lesson, dwState 0](../lessons.md#2026-10-30-newer-windows-11-builds-list-a-listener-with-dwstate-0-the-table-says-it-listens-dyno). The policy that keeps optional updates off the CI PC was prepared; its application is not recorded. Source: src: `windows-runner-update-policy.md` (header).

## Machine load and the heavy-step lock (agents on the Mac)
- Agents gate heavy steps on a 1-minute load below 30; the load rule is [Throughput, rule 41](../rules.md#throughput-user-2026-10-24). Source: src: `merge-steward.md:533` (gate stated in the entry).
- Heavy steps (cargo, npm, ng, playwright, llvm-cov, xwin) take turns through the lock tool `plock`, one at a time:
  `plock run [--high] [--no-gate] <agent-name> -- <command>` takes the lock, waits for the load gate, runs, releases; `plock status` shows the holder. Source: src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock` (header; a scratch tool outside the repository).
- The lock is a directory that appears atomically with its owner record (pid, start time, name) inside; it is broken only by the tool, only
  when the recorded pid is gone or its start time differs, and each break is logged. Do not remove it by hand. Source: src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock` (header and `break_if_stale`).
- Why the tool exists: an agent removed a stale lock by hand while its owner record was empty (`merge-steward.md:646`); the tool was announced in `merge-steward.md:649`. Source: src: `merge-steward.md:646, 649`.

## Known flakes (as the steward judged them in the log; re-check before waiving)
- Windows `ERR_ABORTED` in the `paddock-chromium` real-browser tests is fixed, not a flake: Chrome for Testing's sandboxed network
  service could not start in a user folder, restarted, and lost in-flight loads. Run 7170 showed 0 errors over 5 rounds; the fix is on main
  (`7f09da074`, `440eb48c9`, merged through s15 `863858bce`). A new `ERR_ABORTED` is a regression. Open follow-up: the grant for the runtime store. Source: [lesson of 2026-10-30](../lessons.md#read-the-failing-environments-own-logs-before-building-a-reproduction-2026-10-30); src: `merge-steward.md:188, 210`.
- macOS `real_browser` start timeout on the loaded Mac (runs 7213 and 7250 among others): the fixed 20 s start wait in the
  Debrief chromium driver; cured by the start judge (`merge-steward.md:682`), merged into s26b (`merge-steward.md:684`) and landed
  at `233c1339b` (`merge-steward.md:707`). Source: [lesson of 2026-10-31](../lessons.md#the-fixed-start-wait-hid-in-a-helper-named-connect-2026-10-31-debrief-chromium-driver); see [process-starts](process-starts.md).
- UI jobs with 5000 ms timeouts in later spec files: a leaked fake timer under `isolate: false`; see [test-isolation](test-isolation.md).
- Dyno `a_job_runs_at_the_gentle_level` EBADF: a retry (`c5c240c61`) is on main (`merge-steward.md:679`); the root cause is the macOS socket
  sweep (`merge-steward.md:429`), fixed by `fix/ipc-poll-waits` at `c028d3f39` (`merge-steward.md:473`), being merged into the Dyno batch. Source: src: `merge-steward.md:429, 473, 679`.
- Runs that died in seconds with an empty log or missing step scripts were re-dispatched and treated as infrastructure: 6664 and 6665
  (2026-10-28, `merge-steward.md:10`), 6988 (`merge-steward.md:78`), 7010 (`merge-steward.md:88`); of those two only the plan job failed, and the runs lasted 11 and 13 minutes. Source: src: `merge-steward.md:10, 78, 88`; src: reviewer's check of the run durations, 2026-10-31.

## See also
[releasing](releasing.md), [test-isolation](test-isolation.md), [open-decisions](open-decisions.md).
