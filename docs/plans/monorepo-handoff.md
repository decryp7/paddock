# Monorepo hand-off: app branches before the import

ADR [0014](../adr/0014-monorepo-under-paddock.md) §2 and §11 step 1: branches other than `main` are not imported. Before
an app's import every branch is merged, kept as a hand-off branch in the old repo (listed here), or deleted. This list is
the freeze inventory; **nothing was merged or deleted** while making it. The coordinator decides each row with the user;
"Next step" is a proposal.

- Read 2026-10-25 from the remotes on dev.decryptology.net (blobless bare clones, read-only). "Ahead/behind" count
  commits against that repo's `main`; "merged" means every commit of the branch is on `main` (`ahead 0`).
- Paddock is tagged `pre-monorepo` (annotated) on `d7f4a00`, the 0.3.0 release head of `main`: the rollback point of
  §11.

| Repo | `main` | Branches | Merged (delete) | Open |
|---|---|---|---|---|
| debrief | `e42bb85` | 85 | 81 | 4 |
| pitwall | `50d01ee` | 7 | 0 | 7 |
| Pitcrew | `bd2264d` | 5 | 2 | 3 |
| spotter | `25cb995` | 6 | 3 | 3 |
| dyno | `4131e7c` | 8 | 6 | 2 |
| pacenotes | `9c9c772` | 0 | 0 | 0 |
| sighter | `4ead821` | 5 | 1 | 4 |

## Open branches

No open branch contains another one; each is its own line of work from an older `main`.

| Repo | Branch | Last commit | Ahead / behind | State | Next step (proposed) |
|---|---|---|---|---|---|
| debrief | `chore/rename-trial` | `459d3f7` 2026-10-22 chore: apply the Debrief rename (trial output of the rename script) | 2 / 468 | Trial output; the rename landed on `main` since | **Decided:** stays in the old repo as a hand-off |
| debrief | `docs/usability-review-2026-10-23` | `8aefb60` 2026-10-23 docs: usability review of 2026-10-23 with screenshots | 1 / 271 | Docs only (30 files, screenshots); its fixes merged as `fix/usability-2026-10-23` | **Decided:** stays in the old repo as a hand-off |
| debrief | `perf/test-speed` | `37efa6a` 2026-10-24 perf(test): one integration-test binary per crate, nextest ci profile, one e2e build | 1 / 54 | Not on `main` (272 files); `main` still has `tests/*.rs` binaries | **Done:** rebased onto `main` (one conflict in build.yml, resolved) and imported |
| debrief | `test/x50-shell-baseline` | `8671f1b` 2026-10-25 test(e2e): shell baseline screenshots for the workbench migration (X50) | 1 / 0 | Current; on top of `main` | **Done:** merged (fast-forward) and imported |
| pitwall | `feat/shell` | `25270f6` 2026-10-24 style(app): rustfmt | 33 / 2 | Shell adoption, 301 files; not merged | Hand-off; redo on the in-tree shell after Pitwall's import (path dependencies replace the registry) |
| pitwall | `feat/shell-settings` | `ce56179` 2026-10-24 feat(settings): Pitwall's sections are pages of the shell's Settings window | 28 / 2 | Shell Settings pages, 287 files, overlaps `feat/shell` | Hand-off, together with `feat/shell` |
| pitwall | `feat/shell-specs` | `aa99e1c` 2026-10-24 test(ui): specs on the shell's notify, dialogs and jobs | 19 / 2 | Specs for the shell adoption, 207 files | Hand-off, together with `feat/shell` |
| pitwall | `feat/debrief-client` | `612b4cd` 2026-10-24 feat(api): X-Pitwall-Job-Id; a queued stream answers at once | 13 / 2 | API for Debrief's client, 24 files | Merge before the import (Debrief 1.1 uses Pitwall for AI) |
| pitwall | `feat/segmented-downloads` | `35effe4` 2026-10-24 feat(server): model and engine downloads on paddock-download | 14 / 2 | 40 files; uses `paddock-download` from the registry | Hand-off; redo by path after the import |
| pitwall | `fix/pre-shell` | `1b648ee` 2026-10-24 feat(app): an exit watchdog ends a quit whose engine stop never finishes | 9 / 2 | Fixes before the shell, 13 files | Merge before the import |
| pitwall | `fix/hardened-runtime-off` | `007e9de` 2026-10-24 fix(macos): ad-hoc bundles without the hardened runtime | 1 / 3 | Rule 23 fix, not on `main` | Merge before the import (rule 23) |
| Pitcrew | `feat/shell` | `b7ec074` 2026-10-24 feat: runner downloads through paddock-download, as jobs with phases | 4 / 3 | Shell adoption, 150 files | Hand-off; redo by path after the import |
| Pitcrew | `feature/autoscale` | `621afe8` 2026-10-23 docs(adr): 0021 local autoscaling with a burst pool | 1 / 18 | One ADR draft | Merge the ADR (docs only) or hand-off |
| Pitcrew | `fix/hardened-runtime-off` | `a25b7c2` 2026-10-24 fix(macos): ad-hoc bundles without the hardened runtime | 1 / 3 | Rule 23 fix, not on `main` | Merge before the import |
| spotter | `feat/shell` | `f240168` 2026-10-24 test(e2e): Playwright flows on the shell | 6 / 2 | Shell adoption, 134 files | Hand-off; redo on the in-tree shell after the import |
| spotter | `fix/hardened-runtime-off` | `b47c7b1` 2026-10-24 fix(macos): ad-hoc bundles without the hardened runtime | 1 / 3 | Rule 23 fix, not on `main` | Merge before the import |
| spotter | `fix/monotonic-found-count` | `4f4bc3f` 2026-10-24 fix(jobs): progress within a phase never goes back | 1 / 2 | Rule 11 fix, 2 files | Merge before the import |
| dyno | `fix/agent-after-update` | `f3834f0` 2026-10-24 fix(macos): replace the agent's launchd job once per version | 1 / 2 | Rule 24 fix, 7 files | Merge before the import |
| dyno | `fix/hardened-runtime-off` | `3ac03ab` 2026-10-24 fix(macos): ad-hoc bundles without the hardened runtime | 1 / 3 | Rule 23 fix, not on `main` | Merge before the import |
| sighter | `poc/app` | `cc1e8b8` 2026-10-24 docs(poc): state of the POC | 22 / 5 | The one POC (290 files, code) | Hand-off (old repo); the POC is reference, not product |
| sighter | `poc/macos` | `0d0b177` 2026-10-24 docs(poc): note the planned macOS host | 22 / 5 | `poc/app` plus one note (stopped before implementation) | Delete after copying the note into `poc/app`, or hand-off |
| sighter | `poc/vision` | `d976c71` 2026-10-24 docs(poc-vision): state when work stopped | 5 / 5 | Vision POC, 64 files | Hand-off |
| sighter | `poc/vm-console` | `572473e` 2026-10-24 docs(poc): superseded by the one POC on poc/app | 4 / 6 | Superseded by `poc/app` (says so) | Delete |

The four `fix/hardened-runtime-off` branches are the same rule-23 change in four apps; none is on its `main`.

## Merged branches (every commit on `main`; delete once confirmed)

- **debrief (81):** `ci/hygiene`, `docs/adr-redesign`, `docs/cleanup`, `docs/diagrams`, `docs/release-images`,
  `feat/dialog-kinds`, `feat/dialog-windows`, `feat/entity-catalogue`, `feat/human-names`, `feat/layout-constraints`,
  `feat/long-ops-a`, `feat/long-ops-a2`, `feat/long-ops-a3`, `feat/long-ops-b`, `feat/pitwall-engine`,
  `feat/segmented-download`, `feat/service-health`, `feat/video-sources`, `feat/whats-new-pane`, `feature/ai-parsers`,
  `feature/chromium-research`, `feature/design-pass`, `feature/features-a`, `feature/features-b`, `feature/features-c`,
  `feature/home-minimap`, `feature/lenses`, `feature/log-analysis-ide`, `feature/native-feel`,
  `feature/samples-and-polish`, `feature/search-everything`, `feature/shortcuts-theme-whatsnew`,
  `feature/ux-layout-perspectives`, `feature/ux-settings-polish`, `fix/ai-search-identifiers`, `fix/ai-setup-once`,
  `fix/auto-hide-button`, `fix/blank-start`, `fix/ci-licences-step`, `fix/flakes-1.1.1`, `fix/green-cov-a`,
  `fix/green-cov-b`, `fix/home-narrow-and-long-names`, `fix/home-search-followups`, `fix/import-large-folder`,
  `fix/layout-followups`, `fix/linux-analysis-memory`, `fix/linux-ci`, `fix/long-operations`,
  `fix/macos-store-and-dialogs`, `fix/main-green`, `fix/main-window-layout`, `fix/model-fit`, `fix/release-1.1.0`,
  `fix/release-1.1.1b`, `fix/release-polish`, `fix/research-after-close`, `fix/research-autoclose`,
  `fix/store-memory-after-import`, `fix/usability-2026-10-23`, `integrate/1.1`, `integrate/downloads-catalogue`,
  `integrate/engine-a`, `integrate/engine-b`, `integrate/engine-c`, `integrate/engine-d`, `integrate/home-search`,
  `integrate/redesign`, `integrate/redesign-base`, `integrate/ui-polish`, `perf/startup`, `refactor/angular-phase1`,
  `release/1.1.1`, `release/v1`, `test/coverage-and-leftovers`, and six `worktree-agent-*` branches.
- **Pitcrew (2):** `feature/final`, `fix/1.0.1`.
- **spotter (3):** `feat/engine-gaps`, `feat/ui-gaps`, `release/v1-ready`.
- **dyno (6):** `feat/wp01-helper`, `feat/wp06-wp07`, `feat/wp09-10-33-35`, `feat/wp12-wp27`, `feat/wp13-14-36`,
  `feat/wp23-25`.
- **sighter (1):** `docs/design-review`.

## Reproduce

```sh
git clone --bare --filter=blob:none https://dev.decryptology.net/decryp7/<repo>.git
git -C <repo>.git rev-list --left-right --count main...<branch>   # behind, ahead
```

Imported (2026-10-25): every app. Merged into the app mains before their import: Pitwall `feat/debrief-client` (with `fix/pre-shell`,
`fix/hardened-runtime-off`), Spotter `fix/monotonic-found-count` and `fix/hardened-runtime-off`, Dyno
`fix/agent-after-update` and `fix/hardened-runtime-off`, Pitcrew `fix/hardened-runtime-off` (docs/history.md). Every
other open branch stays a hand-off in its old repo.

Order (user, 2026-10-25, ADR 0014's Debrief-first amendment): Debrief is imported first, right after the frame; the other six
apps after the Debrief release.

Re-read the list right before each app's import (§11 step 3 to 5): branches keep moving until the old repo is frozen.
