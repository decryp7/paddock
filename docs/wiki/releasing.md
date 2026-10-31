---
title: Releasing
verified: 2026-10-31
sources: [apps/dyno/docs/releasing.md, .gitea/workflows/app-release.yml, scripts/vm-smoke/README.md, rule 54, rule 9, rule 25, rule 55, handoff/merge-steward.md]
---
How an app release goes from a green batch to a tag today, the VM checks that sit before the tag, the order of the apps, and
what the Dyno 1.0 tag still waits for. Rules 9, 25, 54 and 55 are authoritative and linked, not restated.

## Current state
- An app is released by an `<app>-vX.Y.Z` tag in CI: the app's `<app>-release.yml` runs the app's legs of ci.yml as gates, then
  the shared `app-release.yml` builds installers on three systems, strips the updater keys, and publishes the Gitea release.
  Shared packages release from a `v*` tag through `publish.yml`. Source: [app-release.yml header](../../.gitea/workflows/app-release.yml); [AGENTS.md](../../AGENTS.md) (Workflows are named after their app).
- A local build is for trying and is never published: [Way of working](../rules.md#way-of-working) (rule 9).
- Before tagging, on one commit: merge every 1.0 branch, bump the version, run one manual `dyno-release` with gates on, read the
  verdict job's `installers: built`, run the scripted VM check on those installers, run the install and update plan, then tag
  that same commit. Source: [Dyno releasing, Before tagging](../../apps/dyno/docs/releasing.md#before-tagging).
- A release-path change builds installers before it lands on main: [rule 54](../rules.md#release-path-user-2026-10-30); the release-path proof (nightly installers, a verdict, a scripted VM check) is on main as `5262ceb03`. Source: src: `git log origin/main` (commit `5262ceb03`, "prove the release path").
- The VM check `scripts/vm-smoke/vm-smoke.sh --app <app> --run <run id>` installs the CI macOS installer in a fresh Tart VM clone,
  cold-starts it and judges the process tree, the discovery files and Quit; it runs locally only, CI never starts a VM.
  Source: [vm-smoke README](../../scripts/vm-smoke/README.md). The GUI automation (`--gui`: Gatekeeper, Login Items) is on branch `feat/vm-smoke-gui` at `438115f6c`, not on main. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 09:25).
- The pinned base image had Gatekeeper disabled, so earlier manual first-launch checks did not test it; the GUI harness enables it. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 00:36).
- VM runs are recorded as result files in the hand-off folder (Dyno: `dyno-1.0-vm-results-3.md`, 12 smoke steps passed, tag blocked by login findings B3/B4 that were later root-caused). Source: src: `handoff/dyno-1.0-vm-results-3.md`; src: `handoff/merge-steward.md` (entries of 2026-10-31 06:46 and 07:37).
- Before a tag the steward also runs the copies check (`copies --tag <tag>`): [Copies, rule 55](../rules.md#copies-user-2026-10-31-adr-0034-rule-40). The C58 command itself is a follow-up of ADR 0034, so until it exists the register is checked by hand. Source: [ADR 0034](../adr/0034-one-implementation-per-mechanism.md).
- Order of the apps in the steward log: shared batch s26b, then the shared widgets with the Dyno widgets batch, then the Dyno release,
  then the Spotter batch; Pitwall 1.1 (batch s23b) is not on the Dyno critical path; the Debrief bench fix is paused until the Dyno,
  Spotter and Pitwall tags. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 00:45, 00:55, 02:50, 08:10).
- s26b landed on main as `233c1339b` (ci 7279 and debrief 7280 green) on 2026-10-31. Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 21:30).

## What the Dyno 1.0 tag waits for (state of the steward log's last entries, 2026-10-31 evening)
- Merge main into the Dyno batch `steward/s28-dyno-1.0` and get ci.yml plus a gated `dyno-release` run with installers. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 21:30 and 01:20).
- Open review findings on the Dyno screenshots at head `0b5a8f041`: the disk activity series capped at 100 percent (a product bug)
  and the tray default width over the family cap (decided: keep the cap, fit Dyno's default); no later entry records either fixed. Source: src: `handoff/merge-steward.md` (entry stamped 2026-11-01 04:35, written on 2026-10-31).
- VM checks on those installers, including the widget retarget checklist (Customize with Settings open, macOS and Windows),
  a `--gui` rerun and the blank-WebContent test; the login agent's verdict was that nothing in the login findings blocks 1.0. Source: src: `handoff/merge-steward.md` (entries of 2026-10-31 20:50 and 05:50); src: `handoff/dyno-vm-checklist-retarget.md`.
- The user's own GUI look, then the tag. Source: src: `handoff/merge-steward.md` (entry of 2026-10-30 12:50).
- The logging work for Dyno (D1 to D7, plan WP-6a) was decided to be part of 1.0; its completion is not recorded in the log. See [logging-and-diagnostics](logging-and-diagnostics.md). Source: src: `handoff/merge-steward.md` (entry of 2026-10-31 21:45).

## See also
[ci-and-runners](ci-and-runners.md), [process-starts](process-starts.md), [open-decisions](open-decisions.md).
