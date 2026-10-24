# npm, pnpm and Bun for the Paddock UIs (2026-10-25)

Question: would another package manager make installs (and so CI and new worktrees) noticeably faster, and does the rest
of the tool chain keep working on its tree? Decision asked for: a recommendation only, no switch.

## Setup

- Mac mini (M-series, 14 cores), fast network, 1-minute load 11-20 during the runs (other agents' builds).
- npm 11.19.1 (Node 24.22), pnpm 12.15.0 (Homebrew), Bun 1.4.4 (official release binary, SHA-256 checked).
- Each manager in its own copy of the project. pnpm needs a `pnpm-lock.yaml`: `pnpm import` converts `package-lock.json`
  in about 1.3 s. Bun reads `package-lock.json` directly (`--frozen-lockfile`) and writes no lock file of its own.
- Cold: empty cache folder (`npm ci --cache`, `pnpm --store-dir`, `BUN_INSTALL_CACHE_DIR`) and no `node_modules`. Warm:
  the cache the previous run filled, no `node_modules`. Three runs each; the table shows each run.
- Install scripts run under none of them (npm 11 and pnpm 12 both hold back esbuild, lmdb, @parcel/watcher and
  msgpackr-extract until approved; nothing needs them: the platform binaries come as optional dependencies).

## Install times (seconds)

`paddock/ui` (242 packages):

| Manager | Cold | Warm |
| --- | --- | --- |
| `npm ci` | 2.9, 2.9, 2.9 | 2.4, 2.4, 2.3 |
| `pnpm install --frozen-lockfile` | 4.1, 3.8, 4.1 | 0.5, 0.4, 0.5 |
| `bun install --frozen-lockfile` | 2.2, 2.4, 2.3 | 0.4, 0.4, 0.5 |

Debrief's `app/` (495 packages; install only):

| Manager | Cold | Warm |
| --- | --- | --- |
| `npm ci` | 4.6, 4.7, 4.4 | 3.7, 3.6, 3.7 |
| `pnpm install --frozen-lockfile` | 6.6, 6.2, 5.9 | 0.8, 0.8, 0.7 |
| `bun install --frozen-lockfile` | 4.2, 3.7, 4.0 | 1.4, 1.0, 1.0 |

## Does the tool chain work on each tree?

On `paddock/ui`, with each manager's `node_modules`: `scripts/build-css.mjs`, `ng build gallery`, `ng test paddock-ui`
(Vitest, 62 spec files) and `playwright test --project=gallery` (7 tests) all passed unchanged under npm, pnpm and Bun.

Changes the switch would need:

- pnpm: a `pnpm-workspace.yaml` (`strictDepBuilds: false`, or `allowBuilds` for the four packages above), the converted
  lock file, and the apps' `"packageManager": "npm@…"` pin replaced: pnpm 12 refuses to install a project pinned to npm
  (Debrief's `app/`) unless `pmOnFail: ignore` / `packageManagerStrict: false` is set.
- Bun: none for installing; every script would still run on Node (`node_modules/.bin`), so Bun would only be the installer.
- Both: CI's `setup-node` npm cache, `npm ci` steps, Renovate's lock file handling, `npm audit` (pnpm has `pnpm audit`,
  Bun has none) and the docs.

## Recommendation

Stay on npm. A CI job installs in 3-5 s with npm, so the most another manager saves is about 3 s per job with a warm
cache, and nothing cold (pnpm is slower cold; Bun saves about 0.5 s). A new worktree saves 2-3 s once. That does not pay
for a second lock file format, Renovate and audit changes, and the package-manager pin in every app. This matches the
earlier measurement on Pitcrew (garage `docs/research.md`). Revisit only if installs become a measurable share of CI
time (for example a much larger dependency tree), and then prefer Bun as an installer only (no lock file change).
