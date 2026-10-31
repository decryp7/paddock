---
title: Releasing
verified: 2026-10-31
sources: [apps/dyno/docs/releasing.md, .gitea/workflows/app-release.yml, scripts/vm-smoke/README.md, rule 54, rule 9, rule 25, rule 55, handoff/merge-steward.md]
---
How an app release goes from a green batch to a tag today, the VM checks that sit before the tag, the order of the apps, and
what the Dyno 1.0 tag still waits for. Rules 9, 25, 54 and 55 are authoritative and linked, not restated. Steward-log
citations are `merge-steward.md:<line>`.

## Current state
- An app is released by an `<app>-vX.Y.Z` tag in CI: the app's `<app>-release.yml` runs the app's legs of ci.yml as gates, then
  the shared `app-release.yml` builds installers on three systems, strips whitespace from the updater keys (rule 25), and publishes the Gitea release.
  Shared packages release from a `v*` tag through `publish.yml`. Source: [app-release.yml header](../../.gitea/workflows/app-release.yml); [AGENTS.md](../../AGENTS.md) (Workflows are named after their app); [rule 25](../rules.md#platform-and-release).
- Where releases come from: [Way of working](../rules.md#way-of-working) (rule 9).
- Before tagging, on one commit: merge every 1.0 branch, bump the version, run one manual `dyno-release` with gates on, read the
  verdict job's `installers: built`, run the scripted VM check on those installers, run the install and update plan, then tag
  that same commit. Source: [Dyno releasing, Before tagging](../../apps/dyno/docs/releasing.md#before-tagging).
- The release-path proof (nightly installers, a verdict, a scripted VM check) is on main as `5262ceb03`; what must be proved before a
  release-path change lands is [rule 54](../rules.md#release-path-user-2026-10-30). Source: src: `git log origin/main` (commit `5262ceb03`).
- The VM check `scripts/vm-smoke/vm-smoke.sh --app <app> --run <run id>` installs the CI macOS installer in a fresh Tart VM clone,
  cold-starts it and judges the process tree, the discovery files and Quit; it runs locally only, CI never starts a VM. It enables
  Gatekeeper first (`spctl --global-enable`), because the pinned base image ships with it off. Source: [vm-smoke README](../../scripts/vm-smoke/README.md); [checks.mjs](../../scripts/vm-smoke/checks.mjs).
- The GUI automation (`--gui`: Safari download, Gatekeeper, Login Items) is on branch `origin/feat/vm-smoke-gui`, head `5beceb348`, not on
  main. Source: src: `git branch -r --contains 5beceb348`; src: `merge-steward.md:367, 452`.
- VM runs are recorded as result files in the hand-off folder (Dyno round 3: 12 smoke steps passed; login findings B3 and B4 blocked the
  tag and were then root-caused). Source: src: `dyno-1.0-vm-results-3.md`; src: `merge-steward.md:399` (results) and `merge-steward.md:415` (root causes).
- The steward also runs the copies check before a tag; the register and its trailers are [rule 55](../rules.md#copies-user-2026-10-31-adr-0034-rule-40) and [ADR 0034](../adr/0034-one-implementation-per-mechanism.md); the C58 command is a follow-up of that ADR. Source: [ADR 0034](../adr/0034-one-implementation-per-mechanism.md).
- Order of the apps in the log: shared batch s26b, then the shared widgets with the Dyno widgets batch, then the Dyno release, then the
  Spotter batch (`merge-steward.md:665`). Pitwall 1.1 (batch s23b) is off the Dyno critical path (`merge-steward.md:683`); the Debrief
  bench fix waits for the Dyno, Spotter and Pitwall tags (`merge-steward.md:666`); the Spotter batch is not dispatched before the Dyno tag (`merge-steward.md:276`). Source: src: `merge-steward.md:276, 665, 666, 683`.
- s26b landed on main as `233c1339b` (ci 7279 and debrief 7280 green). Source: src: `merge-steward.md:707`.

## What the Dyno 1.0 tag waits for (state at the log's last entries)
- Main merged into the Dyno batch `steward/s28-dyno-1.0` (`merge-steward.md:707`), then ci.yml and a gated `dyno-release` run with installers. Source: src: `merge-steward.md:676, 707`.
- The macOS socket sweep must be replaced, which the user required for 1.0 because it caused the EBADF flake: `fix/ipc-poll-waits` at
  `c028d3f39` (approved) and `dyno/macos-socket-list` at `5defc8fcc` (ready); both are now being merged into s28. Source: src: `merge-steward.md:429, 473, 478`; src: coordinator note of 2026-10-31 (merge into s28 in progress).
- Open screenshot findings on the Dyno batch: the disk activity series capped at 100 percent (a product bug) and the tray default width
  over the family cap; no later entry records them fixed. Source: src: `merge-steward.md:681, 689`.
- VM checks on those installers, including the widget retarget checklist (Customize with Settings open, on macOS and Windows), a `--gui`
  rerun and the blank-WebContent test; the login agent's verdict was that nothing in the login findings blocks 1.0. Source: src: `merge-steward.md:640, 653, 697`; src: `dyno-vm-checklist-retarget.md`.
- The user's own GUI look, then the tag on the same commit. Source: src: `merge-steward.md:294`.
- The logging work for Dyno (D1 to D7, plan WP-6a) was decided to be part of 1.0; its completion is not recorded. See [logging-and-diagnostics](logging-and-diagnostics.md). Source: src: `merge-steward.md:708`.

## See also
[ci-and-runners](ci-and-runners.md), [process-starts](process-starts.md), [open-decisions](open-decisions.md).
