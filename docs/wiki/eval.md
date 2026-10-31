---
title: Pilot evaluation
verified: 2026-10-31
sources: [handoff/research-llm-wiki-2026-10-31.md, handoff/merge-steward.md, docs/lessons.md]
---
Ten questions that agents re-derived recently, with the expected answers and where each answer comes from. At the end of the pilot
a fresh agent answers all ten twice, once with only the repository and once with the wiki; a reviewer scores each answer for
correctness, sources, tokens and time. Pass and kill criteria are in the research note (section 4). The expected answers were
written from the sources on 2026-10-31 and must be re-verified before the test starts, because several state facts change daily.

## How to score
Per question: correct (0 or 1), cites a real source (0 or 1), tokens and wall time of the answer. Also count wiki facts that were wrong
or that misled an agent, and the lint findings of the fortnight. Source: src: `handoff/research-llm-wiki-2026-10-31.md` (section 4, WP-4).

## Questions

### 1. How do I dispatch CI on a batch of merged branches?
Expected: build the batch branch, POST to Gitea's `actions/workflows/ci.yml/dispatches` with the batch `ref` and `inputs.base` set to the merge base with
origin/main, also dispatch the app's own workflow when the plan marks the app, one run per head (cancel stale ones), land by fast-forward when green.
Sources: [ci-and-runners](ci-and-runners.md); src: `handoff/merge-steward.md` (header line 3); [CI economy](../rules.md#ci-economy-user-2026-10-29).

### 2. Why did UI specs time out at 5000 ms in the s23b runs?
Expected: a spec spied on `setTimeout` under fake timers and left a dead fake clock; the specs of a bundle share a page (`isolate: false`), so later files hung.
Fix: the shared test-kit timer guard `bb10e52a3` and a spec that uses `getTimerCount`; not on main.
Sources: [test-isolation](test-isolation.md); src: `handoff/merge-steward.md` (entries of 2026-10-31, stamped 2026-11-01 03:45 and 04:00).

### 3. Which conformance check number is free?
Expected: C66 is the first number past every claim in the log (C49 and C61 have no claim either); C52, C55, C58 to C60, C62 to C65 are taken or claimed; check the branches first.
Sources: [process-starts](process-starts.md); [conformance README](../../conformance/README.md); src: `handoff/merge-steward.md`.

### 4. What blocks the Dyno 1.0 tag?
Expected: the batch `steward/s28-dyno-1.0` needs main merged in and a gated `dyno-release` run with installers; the open screenshot P1s (disk activity series, tray default);
VM checks on those installers (retarget checklist, `--gui`, WebContent); the user's GUI look; then the tag on the same commit.
Sources: [releasing](releasing.md); [Dyno releasing, Before tagging](../../apps/dyno/docs/releasing.md#before-tagging); src: `handoff/merge-steward.md`.

### 5. How does the heavy-step lock work?
Expected: `plock run [--high] [--no-gate] <agent> -- <cmd>` takes an atomic lock directory with an owner record, waits for load below 30, runs, releases; broken only by the tool when the owner pid is gone or its start time differs, logged; never remove it by hand.
Sources: [ci-and-runners](ci-and-runners.md); src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock`.

### 6. Why must a child's start not be judged on a fixed wait?
Expected: a loaded or cold machine, or an OS that stops the app, makes a fixed window expire while nothing is wrong; the start judge looks at run state, CPU time, progress and a ready handshake (ADR 0031); Dyno doubled its agent after a fixed 5 s.
Sources: [process-starts](process-starts.md); [ADR 0031](../adr/0031-patience-not-wall-clocks.md); [lesson](../lessons.md#a-start-window-on-a-wall-clock-doubled-dynos-agent-on-a-cold-disk-2026-10-30-dyno).

### 7. Is rule 56 on main, and what is ADR 0036's status?
Expected: neither is on main at `233c1339b`; rule 56 is on `docs/rule-logs-explain-failures` (`ef8f52349`), ADR 0036 on `docs/adr-logging-root-cause` (`021dc3c8d`, Accepted 2026-10-31).
Sources: [logging-and-diagnostics](logging-and-diagnostics.md); src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md`.

### 8. Does a release-path change need installers before it lands, and how is it proved?
Expected: yes (rule 54); a manual `<app>-release.yml` run builds the installers on three systems and the verdict job says `installers: built`; the release-path proof is on main as `5262ceb03`.
Sources: [releasing](releasing.md); [rule 54](../rules.md#release-path-user-2026-10-30); [Dyno releasing](../../apps/dyno/docs/releasing.md#before-tagging).

### 9. Does the Windows `ERR_ABORTED` flake mean pid reuse killed a browser?
Expected: no. Pid reuse is a real hazard (rule 53) but run 7158, with every identity fix and none of the diagnostics, still failed, so the cause lies elsewhere and is open (ADR 0033 WP-4).
Sources: [process-starts](process-starts.md); [ADR 0033](../adr/0033-processes-by-identity-never-by-a-bare-pid.md); [ci-and-runners](ci-and-runners.md).

### 10. Do CI runs include a VM check, and where does it run?
Expected: no. `scripts/vm-smoke/vm-smoke.sh` runs locally in a fresh Tart VM clone on the installers of a CI run; CI never starts a VM; the result file is attached before the tag.
Sources: [releasing](releasing.md); [vm-smoke README](../../scripts/vm-smoke/README.md).
