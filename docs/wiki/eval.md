---
title: Wiki evaluation
verified: 2026-11-09
sources: [handoff/research-llm-wiki-2026-10-31.md, handoff/merge-steward.md, docs/lessons.md]
---
Ten questions that agents re-derived recently, with the expected answers and where each answer comes from. The user made the wiki
official on 2026-10-31 (`merge-steward.md:719`), so this is an optional measure, not a gate: a fresh agent can answer all ten twice, once with
only the repository and once with the wiki, and a reviewer scores each answer for correctness, sources, tokens and time. The original pass and
kill criteria are in the research note (section 4). The expected answers were
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
Fix: the shared test-kit timer guard (`onTestFinished` head `d459854a6`, only on `fix/panel-snapshot-spec-cost`, in review) and a spec that uses `getTimerCount`; not on main.
Sources: [test-isolation](test-isolation.md); src: `merge-steward.md:673, 683, 685`.

### 3. Which conformance check number is free?
Expected: 103, written here as a number because the id with its letter would trip the C100 docs gate (it rejects a check id that does not exist). It is the first number above every check file (the highest on the tree is C99: `git ls-tree 0cf98adb2 conformance/src/checks`; origin branches hold files up to number 97, `git ls-tree origin/<branch> conformance/src/checks` for each) and above every number a doc names (the highest is number 102; `git grep -h -o -w -E 'C[0-9]{2,3}' <rev> -- docs 'apps/*/docs'` over the tree and the origin branches). Below it nothing is free to assume: ADRs name numbers that have no file yet, for example C58 (ADR 0034 WP-1, registered under `[[planned]]` in `docs/gates.toml`, src: `git show 0cf98adb2:docs/gates.toml`) and C52 (the Pacenotes `cargo-metadata`-off check: Pacenotes ADR 0002 line 105, src: `git show origin/pacenotes/a2-parse:apps/pacenotes/docs/adr/0002-core-types-and-ports.md`; its first-release plan line 361 says it waits for shared code, src: `git show origin/docs/pacenotes-fast-ingest:apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md`; C100 accepts the id because `docs/gates.toml` registers it under `[[planned]]`). On the tree the checks C59, C61 and C62 exist (C59 updater guard, C61 the Material bridge, C62 limit-completeness: the file names in `git ls-tree 0cf98adb2 conformance/src/checks`).
Sources: [process-starts](process-starts.md); [conformance README](../../conformance/README.md); the `git` commands named above.

### 4. What blocks the Dyno 1.0 tag?
Expected: main merged into `steward/s28-dyno-1.0` together with the replacement of the macOS socket sweep the user required (`fix/ipc-poll-waits` `c028d3f39`, `dyno/macos-socket-list` `5defc8fcc`, being merged; s28 not yet pushed with them), then ci.yml and a gated `dyno-release` run with installers; the open screenshot findings (disk activity series, tray default); VM checks on those installers (retarget checklist, `--gui`, WebContent); the user's GUI look; then the tag on the same commit.
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
