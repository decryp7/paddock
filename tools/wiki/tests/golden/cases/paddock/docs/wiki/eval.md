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
Expected: build the batch branch (`steward/sNN-<subject>`), POST to Gitea's `actions/workflows/ci.yml/dispatches` with the batch `ref` and `inputs.base` set to the merge base with
origin/main, also dispatch the app's own workflow when the plan marks the app, one run per head (cancel stale ones), land by fast-forward when green.
Sources: [ci-and-runners](ci-and-runners.md); src: `merge-steward.md:3, 118, 644`; [CI economy](../rules.md#ci-economy-user-2026-10-29).

### 2. Why did UI specs time out at 5000 ms in the s23b runs?
Expected: a spec spied on `setTimeout` under fake timers and left a dead fake clock; the specs of a bundle share a page (`isolate: false`), so later files hung.
Fix: the shared test-kit timer guard (head `d459854a6`, `onTestFinished`, in review) and a spec that uses `getTimerCount`; not on main.
Sources: [test-isolation](test-isolation.md); src: `merge-steward.md:673, 683, 685`.

### 3. Which conformance check number is free?
Expected: C66. On main: C50, C51, C53, C54, C56, C57. Claimed: C49 provider-grants (Spotter line), C52 Pacenotes cargo-metadata, C55 windowed test browser, C58 copies, C59 updater guard, C60 busy-wait-read, C61 the Material bridge (`fix/ui-settings-density`, Pitcrew p8, s28), C62 pipefail-grep, C63 to C65 logging. Check the branches first.
Sources: [process-starts](process-starts.md); [conformance README](../../conformance/README.md); src: `merge-steward.md:150, 155, 566, 600, 641, 712`.

### 4. What blocks the Dyno 1.0 tag?
Expected: main merged into `steward/s28-dyno-1.0` together with the replacement of the macOS socket sweep the user required (`fix/ipc-poll-waits` `c028d3f39`, `dyno/macos-socket-list` `5defc8fcc`, being merged), then ci.yml and a gated `dyno-release` run with installers; the open screenshot findings (disk activity series, tray default); VM checks on those installers (retarget checklist, `--gui`, WebContent); the user's GUI look; then the tag on the same commit.
Sources: [releasing](releasing.md); [Dyno releasing, Before tagging](../../apps/dyno/docs/releasing.md#before-tagging); src: `merge-steward.md:294, 429, 473, 478, 689`.

### 5. How does the heavy-step lock work?
Expected: `plock run [--high] [--no-gate] <agent> -- <cmd>` takes an atomic lock directory with an owner record, waits for load below 30, runs, releases; broken only by the tool when the owner pid is gone or its start time differs, logged; never remove it by hand.
Sources: [ci-and-runners](ci-and-runners.md); src: `/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock`.

### 6. Why must a child's start not be judged on a fixed wait?
Expected: a loaded or cold machine, or an OS that stops the app, makes a fixed window expire while nothing is wrong; the start judge looks at run state, CPU time, progress and a ready handshake (ADR 0031); Dyno doubled its agent after a fixed 5 s.
Sources: [process-starts](process-starts.md); [ADR 0031](../adr/0031-patience-not-wall-clocks.md); [lesson](../lessons.md#a-start-window-on-a-wall-clock-doubled-dynos-agent-on-a-cold-disk-2026-10-30-dyno).

### 7. Are rules 56 to 58 and ADR 0036 on main, and what is their status?
Expected: none is on main at `233c1339b`. Rule 56 is on `docs/rule-logs-explain-failures` (`ef8f52349`, approved for the next shared batch); rules 57 and 58 are on `docs/rca-rules` (`9fba4291f`, in review); ADR 0036 is on `docs/adr-logging-root-cause`, approved at `021dc3c8d` and queued in the docs batch.
Sources: [logging-and-diagnostics](logging-and-diagnostics.md); [test-isolation](test-isolation.md); src: `git show origin/docs/adr-logging-root-cause:docs/adr/0036-logging-for-root-cause.md`.

### 8. Does a release-path change need installers before it lands, and how is it proved?
Expected: yes (rule 54); a manual `<app>-release.yml` run builds the installers on three systems and the verdict job says `installers: built`; the release-path proof is on main as `5262ceb03`.
Sources: [releasing](releasing.md); [rule 54](../rules.md#release-path-user-2026-10-30); [Dyno releasing](../../apps/dyno/docs/releasing.md#before-tagging).

### 9. Does the Windows `ERR_ABORTED` flake mean pid reuse killed a browser, and is it still open?
Expected: no, and no. Pid reuse was disproved by run 7158. The cause was Chrome for Testing's sandboxed network service failing to start in a user folder, restarting and losing in-flight loads; run 7170 confirmed it (0 errors over 5 rounds) and the fix is on main (`7f09da074`, `440eb48c9`, via s15 `863858bce`). A new `ERR_ABORTED` is a regression. Only the grant for the runtime store is an open follow-up.
Sources: [ci-and-runners](ci-and-runners.md); [process-starts](process-starts.md); [lesson](../lessons.md#read-the-failing-environments-own-logs-before-building-a-reproduction-2026-10-30); src: `merge-steward.md:188, 210`.

### 10. Do CI runs include a VM check, and where does it run?
Expected: no. `scripts/vm-smoke/vm-smoke.sh` runs locally in a fresh Tart VM clone on the installers of a CI run; CI never starts a VM; the result file is attached before the tag.
Sources: [releasing](releasing.md); [vm-smoke README](../../scripts/vm-smoke/README.md).
