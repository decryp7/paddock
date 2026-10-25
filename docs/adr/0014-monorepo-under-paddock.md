# 14. One monorepo under `paddock`

- Status: accepted (2026-10-24, by the user); work starts after shell 0.3.0 is tagged
- Date: 2026-10-24
- Amended 2026-10-25 (twice: Debrief is imported first, see the last amendment) after the trial ([2026-10-monorepo-spike.md](../research/2026-10-monorepo-spike.md)): history size,
  import checklist (§2), sccache wording (§1, §9), affected rules (§6), task runner decided (§7), per-app `latest`
  releases and the bridge target (§4), CODEOWNERS syntax (§9)
- Decision by the user (2026-10-24): "finish 0.3.0, then monorepo under paddock". This ADR decides how.
- Related: rules 1-4, 8, 9, 39, 41, 42, 43; ADRs 0006 (versioning and consumption, partly replaced here), 0012 §8 (the
  canary); plan [shared-next.md](../plans/shared-next.md) X9, X10, X53; research
  [2026-10-ci-speed.md](../research/2026-10-ci-speed.md), [2026-10-monorepo-spike.md](../research/2026-10-monorepo-spike.md)
- Users: every app (Debrief, Pitwall, Pitcrew, Spotter, Dyno, Pacenotes, Sighter) and the shared crates and packages

## Context

Today the family is eight repositories: `paddock` (shared crates, `@decryp7/paddock-ui`, `@decryp7/paddock-e2e`, docs,
the shell demo) and one per app. Shared code reaches an app by a tagged release: `publish.yml` sends crates to the Gitea
cargo registry and packages to the Gitea npm registry, then Renovate opens a PR in each app. That costs a full release
cycle for every shared change and leaves the apps on different versions of the shell. The nightly canary
(`canary.yml`) exists only to find breakage across the repo boundary. Rule 42 (share first) and rule 43 (test shared
behaviour once) are hard to keep when a shared change and its first user live in two repos with two CI runs.

Facts read for this ADR (2026-10-24, `main` of each repo):

| Repo | Tracked files | Tracked size | Commits | Pack | Cargo workspace members | Angular | CI lines |
|---|---|---|---|---|---|---|---|
| paddock | 1,518 | 36 MB | 197 | loose | 11 crates + `apps/shell-demo` (own workspace) | 22.2 | 4 workflows |
| debrief | 3,767 | 109 MB | 1,330 | 50 MiB | 42 + `fuzz/` (own workspace) | 22.2 | 1,791 (6 workflows) |
| dyno | 959 | 59 MB | 167 | 1 MiB + loose | 11 + `templates/plugin` | 22.2 | 603 |
| pitwall | 665 | 42 MB | 135 | 0.5 MiB + loose | 5 | 22.2 | 605 (build + release) |
| pitcrew | 604 | 29 MB | 105 | 4 MiB | 4 | 22.2 | 476 |
| spotter | 472 | 20 MB | 145 | 5 MiB | 5 | 22.2 | 575 |
| sighter | docs only | < 80 MB with history | 35 | loose | none yet | none yet | none |
| pacenotes | docs only | < 36 MB with history | 18 | loose | none yet | none yet | none |

- All apps use the same layout: UI in `app/`, Tauri in `app/src-tauri/`, crates in `crates/`, release scripts in
  `scripts/release/` (`latest-json.mjs`, `publish.mjs`), one `renovate.json` (identical base config), Tauri 2.12.4.
- No repo uses Git LFS. The largest blobs are Debrief screenshots (about 2.4 MB each, several versions) and its
  `messages.xlf` (2.4 MB). Debrief's `samples/` is 17 MB and `docs/` 61 MB. Working trees with `target/` are large
  (Debrief 40 GB, Spotter 8 GB, Pitwall 8 GB); that is build output, not history.
- Updater endpoints are fixed in every installed app:
  `https://dev.decryptology.net/decryp7/<app>/releases/download/latest/latest.json` (Debrief, Pitwall, Pitcrew,
  Spotter, Dyno; Pitcrew with a capital P in the repo name). Releases publish `latest.json` to a moving `latest` release.
- Only Spotter consumes `@decryp7/paddock-ui` (0.1) today; no app consumes the registry crates yet. The shell demo
  already lives in `paddock/apps/shell-demo` as its own Cargo workspace with path dependencies on `crates/`, and releases
  from its own tag (`shell-demo-v*`). That is the pattern this ADR extends.
- Feature differences that a single Cargo workspace would unify: Debrief enables Tauri's `unstable` feature, its own
  `windows` crate feature set and DuckDB bindings; the other apps do not.

## Decision

### 1. Layout

```
paddock/
  crates/                shared Rust crates, one Cargo workspace (as today)
  ui/                    @decryp7/paddock-ui, @decryp7/paddock-e2e, gallery, shell-demo UI (as today)
  apps/
    debrief/             the whole Debrief repo as it is today: Cargo.toml (its workspace), crates/, app/, samples/, docs/, ...
    pitwall/  pitcrew/  spotter/  dyno/  pacenotes/  sighter/
    shell-demo/          (exists)
  docs/                  family docs: rules, lessons, ADRs 0001+, plans, research
  scripts/               family scripts: affected.mjs, release/ (shared latest-json, publish), new-app.mjs
  .gitea/workflows/      ci.yml (plan + affected jobs), release-<app>.yml, nightly.yml, publish.yml
  package.json           npm workspaces: ui, apps/*/app
  renovate.json  AGENTS.md  CODEOWNERS  rust-toolchain.toml
```

Each app keeps its tree unchanged below `apps/<app>/`, so its paths, docs and scripts move by one prefix and nothing else.
App docs (ADRs, plans) stay in the app; family docs stay in `docs/`. App ADR numbers stay per app; links to them are
written as `apps/debrief/docs/adr/0056-...`.

**Cargo: one workspace per app, plus `crates/`; no root workspace.** Apps depend on shared crates by path
(from `apps/<app>/app/src-tauri/`: `paddock-shell = { path = "../../../../crates/paddock-shell" }`; the shell demo's
`apps/shell-demo/src-tauri/` is one level shallower and uses `../../../crates/...`). Reasons:

- Feature unification. A root workspace resolves one feature set for all members, so Debrief's `tauri/unstable`,
  its `windows` features and DuckDB would leak into Pitcrew's build and change what ships. Per-app workspaces build each
  app exactly as today.
- Build time. `cargo clippy --workspace` and `cargo test --workspace` stay app-sized (Debrief's 42 crates do not
  rebuild for a Pitcrew change). With one root workspace every lint would cover about 80 crates.
- Lockfiles. One `Cargo.lock` per app means a dependency bump lands app by app through that app's gates, and an app in
  release freeze is not moved by another app's upgrade. The cost is eight lockfiles; Renovate handles them (§8).
- Clippy scope. Each workspace keeps its own `clippy.toml` (its own `disallowed-methods`), which already differ.
- Disk. Each app keeps its own target directory. sccache does **not** stand in for a shared workspace (trial, 2026-10-25):
  its Rust cache key carries the checkout path and the `CARGO_TARGET_DIR` path, so it hits 100% only when both are
  identical and about 6.5% otherwise (another worktree, another target dir, a separate clone; `SCCACHE_BASEDIRS` does not
  help rustc), and two apps in one checkout share only about 6% because per-app feature resolution gives common crates
  different features. sccache is used in CI with a fixed checkout path and a fixed `CARGO_TARGET_DIR` per app, where a
  warm cache halves a Rust compile; on the dev Mac the saving comes from keeping a target folder per app per worktree
  (§9), which rebuilt Pitwall in 13 s after a shared-crate change.

`crates/` stays one workspace for the shared crates; its tests are the shared behaviour tests of rule 43.
Versions of common dependencies are kept equal by Renovate grouping (§8) and a `cargo deny` duplicate check per
workspace, not by one lockfile. Revisit a root workspace only if two or more apps need different versions of a shared
crate's dependency and the duplicate check cannot hold them together.

**npm: one root npm workspace and one `package-lock.json`** for `ui/` and every `apps/*/app`. All apps are on Angular
22.2 already, and rule 39 wants one Angular per family. Apps depend on `"@decryp7/paddock-ui": "*"`, resolved to the
workspace package; the library is built once (`npm run build -w ui`) before an app's build, and an app's `tsconfig`
maps `@decryp7/paddock-ui/*` to `ui/dist/paddock-ui/*` (the Angular CLI library-in-workspace pattern). The trial
confirmed it (one lock, one copy of every Angular package, 600 MB of `node_modules` instead of 2.6 GB; all apps build and
their unit and Chromium e2e tests pass), so the per-app-lock fallback is dropped. Two conditions: an app's `angular.json`
names packages by package path (`@angular/cdk/overlay-prebuilt.css`), never `node_modules/...`, because packages are
hoisted to the root; and every workspace package has a unique `name`.

### 2. History-preserving import

Each app is imported with its full history, rewritten into its folder, with its tags renamed:

```sh
git clone --no-local <app-repo> /tmp/import-<app> && cd /tmp/import-<app>
git filter-repo --to-subdirectory-filter apps/<app> --tag-rename '':'<app>-'
cd <paddock worktree> && git remote add import-<app> /tmp/import-<app> && git fetch import-<app> --tags
git merge --allow-unrelated-histories --no-ff import-<app>/main -m "chore(<app>): import history into apps/<app>"
```

- `git filter-repo` over `git subtree add`: filter-repo rewrites every commit's paths, so `git log -- apps/<app>/...`
  and `git blame` work without `--follow` tricks; subtree keeps the old paths in history and needs `subtree split`
  to get them back.
- Tags become `<app>-vX.Y.Z` (Debrief `v1.0.0` → `debrief-v1.0.0`). Paddock's own `v0.1.0` and `v0.2.0` stay.
- Commit hashes change. Old hashes stay valid in the old repos (read-only). Each import merge commit names the old
  repo's last commit in its body so a hash in an old issue can be found.
- Branches other than `main` are not imported. Before the import every branch is merged, pushed as a hand-off branch in
  the old repo, or deleted.
- Debrief's `.gitattributes` (byte-exact samples, CRLF fixtures) keeps working: per-directory `.gitattributes` apply
  below `apps/debrief/`. The root `.gitattributes` gets `* text=auto` once.
- Size of the monorepo history (measured in the trial): **703 MiB** packed after `git gc --aggressive`, not the 100 MB
  first estimated. 99% of it is PNG screenshot history (696 MB in 2,894 blobs; 203 MB of PNG in the current tree; Dyno's
  screenshots alone 234 MB). Decision: import the full history as it is (a one-time clone; worktrees share the object
  store) and recapture an app's screenshot set only for a release. Revisit LFS for `**/docs/screenshots/**` and
  `**/docs/images/**` if the pack passes 1 GB, and for any new binary above 5 MB. Dropping old screenshot versions at
  import (about 500 MB less) was considered and not chosen, because it rewrites more history than the move needs.
- **Import checklist** (each item found by the trial; done in the import commit of the app it concerns, or in the frame):
  - Rewrite sibling-repo paths: every app's diagram sources import `../../../../paddock/diagrams/tools/*.mjs` (and
    Sighter `../../../../paddock/docs/images/src/*`), 20 imports in all; inside the monorepo they become
    `../../../../../diagrams/tools/...`. Grep for `\.\./(\.\./)*(paddock|debrief|pitwall|pitcrew|spotter|dyno|sighter|pacenotes)/`
    before the merge.
  - Move the `decryp7` registry definition from `crates/.cargo/config.toml` to the root `.cargo/config.toml` (or drop
    `registry` from `crates/Cargo.toml`'s workspace dependencies once §5 stops publishing); otherwise every app
    workspace that uses a shared crate by path fails `cargo metadata` with "registry index was not found".
  - Cargo reads `.cargo/config.toml` from the current directory upwards, not from the manifest: every cargo call
    (scripts, CI, `affected.mjs`) runs inside the repo.
  - Shared crates by path with the right depth (§1): `../../../../crates/<crate>` from `apps/<app>/app/src-tauri/`.
  - Unique npm package names for the root workspace (Debrief's UI is named `app`; rename it, e.g. `debrief-app`).
  - App `angular.json` styles and assets by package path, not `node_modules/...` (Pitwall, Dyno, Debrief today).
  - Root `.gitignore` gets `node_modules/` (and the existing per-app ignores keep working below `apps/<app>/`).

### 3. Old repositories

Each old repo becomes a frozen pointer, in two stages:

1. **At import:** a last commit replaces the README with "This repository moved to
   `paddock/apps/<app>`" and a link, branch protection forbids pushes to `main`, issues are closed with a link to the
   new place (open ones are re-filed in `paddock` with an `app:<app>` label; there are few). The `latest` release and
   its assets stay, because installed apps read them (§4).
2. **After the updater bridge period (§4):** the repo is set to Gitea "archived" (read-only, releases still
   downloadable).

Repos are never deleted: old release URLs, old commit hashes and Renovate history stay reachable.

### 4. Tags, releases and the updater

- **Tags:** `<app>-vX.Y.Z` per app (as `shell-demo-v*` today). Shared packages, if published at all (§5),
  `paddock-vX.Y.Z`. Rule 9 still applies: releases only from a tag in CI, never a `[skip ci]` commit.
- **Workflows:** one `release-<app>.yml` per app, triggered by `tags: ["<app>-v*"]`; it checks that the tag names the
  version in `apps/<app>/app/src-tauri/tauri.conf.json`, runs that app's full gates and every shared crate and package
  the app depends on, builds bundles on three OS, and creates the Gitea release. Steps come from the shared actions of
  X10 (`paddock/.gitea/actions/*`), so the seven release workflows are short.
- **Release notes:** each app keeps its `CHANGELOG.md` and `cliff.toml` under `apps/<app>/`; `git-cliff` runs with
  `--include-path apps/<app>/** --include-path crates/** --include-path ui/**` and `--tag-pattern '<app>-v.*'`,
  so an app's notes list its own commits and the shared changes it ships.
- **Moving `latest`:** there is no release named `latest` in any app repo today: Gitea resolves
  `/releases/download/latest/<file>` to the repo's **newest release** (checked 2026-10-25: Pitwall's URL serves
  v1.0.3's `latest.json`). In `paddock` that URL would serve whichever app released last, so each app gets its own
  moving release `<app>-latest` in `paddock`, with `latest.json` as an asset, and the app's endpoint names that tag.
- **Installed users keep updating.** Their endpoint is the old repo's `latest/latest.json`, and Gitea does not redirect
  release downloads between repos. So:
  1. Every app release from the monorepo uploads `latest.json` (and nothing else: it points at the installers in
     `paddock`) to **both** `paddock/releases/download/<app>-latest/latest.json` and the old repo, where it replaces
     `latest.json` on the old repo's **newest release** (or creates a new release there for it), because that is what
     the old endpoint resolves to. The upload uses a user token with `write:repository` stored as a secret: Gitea access
     tokens are user-wide (the trial read the usual token's scopes as `write:package, write:repository, read:user` and
     confirmed release writes to other repos pass the scope check, without uploading), while the Actions job token is
     meant for the workflow's own repository.
  2. The first monorepo release of each app changes its `endpoints` to `[new URL, old URL]` (Tauri tries them in
     order), so every user who installs it moves to the new URL.
  3. The bridge stays for at least two minor releases of the app or six months, whichever is later. Then stage 2 of §3
     archives the old repo. A user who skipped every release in that time still gets the last bridged `latest.json`,
     which leads to a build with the new endpoint.
- `latest-json.mjs` and `publish.mjs` (five copies today, X9) become one `scripts/release/` with the app name and
  endpoint list as arguments.

### 5. Shared packages: stop routine publishing

Inside the monorepo every app uses the shared crates by path and `@decryp7/paddock-ui` by workspace, always at `HEAD`.
So:

- `publish.yml` stops publishing on every release. Crates and npm packages are published only for outside users, and
  only crates marked `[package.metadata.paddock] external = true` (the plugin API crates for third-party plugin authors,
  ADR 0001, when they exist). Today no crate is external; the registry versions already published stay where they are.
- ADR 0006's "Consumption" section (registry, Renovate PR per Paddock release) and the `shell_version` lockstep check
  between `paddock-shell` and `@decryp7/paddock-ui` are no longer needed for the apps; the check stays because it is
  cheap and protects a stale `ui/dist`.
- Crate `version` fields stay (cargo needs them) but no longer gate anything; per-crate changelogs are replaced by the
  apps' notes (§4).
- `canary.yml` is removed: affected CI (§6) tests every consumer of a change on the push itself.

### 6. CI: affected-only, built first

One `ci.yml` with a `plan` job and per-project jobs. Built before the first app moves (§11 step 2), so no app ever runs
the full family on every push.

- **Plan:** `scripts/affected.mjs <base> <head>` lists changed files (`git diff --name-only`), maps them to projects,
  then adds every project that depends on them:
  - Rust: `cargo metadata` of each workspace (`crates/`, each `apps/<app>/`) gives path dependencies; a change in
    `crates/paddock-jobs` marks `crates/` and every app whose metadata reaches it.
  - npm: the root workspace graph (`npm query .workspace` plus each `package.json`); a change in `ui/` marks every app
    that reaches it.
  - Two more dependency kinds that neither graph shows (both found by the trial, both missed without them):
    a Tauri shell whose `tauri.conf.json` `build.frontendDist` points into another package depends on it
    (`apps/shell-demo` embeds `ui/dist/shell-demo`; a `ui/` change broke the shell demo job in Paddock's history), and
    `tsconfig*.json` `paths` or `angular.json` references outside the package are dependencies (an app that maps
    `@decryp7/paddock-ui` to `ui/dist/paddock-ui` has no npm edge to `ui/` any more).
  - Inputs that touch everyone: `.gitea/**` (except a per-app release workflow, `release-<app>.yml`, which marks only
    its app), `.cargo/**`, `rust-toolchain.toml`, root `package.json`, `scripts/release/**`, `garage` image tag,
    `scripts/affected.mjs` itself; root `package-lock.json` marks only the workspaces whose resolved packages changed
    (by diffing the lock per workspace). Unknown root files mark everything.
  - Docs-only changes (`**/*.md`, `docs/**`, `apps/*/docs/**`) mark nothing except a link check, **except declared doc
    inputs**: markdown that code or tests read (an app's or crate's `CHANGELOG.md` for What's New and `include_str!`,
    `*.md` under `resources/`, `samples/`, `src/`, `tests/`, `templates/`, `e2e/`, and named files such as Debrief's
    `docs/ai/sql-views.md` and `docs/testing/quarantine.md`) mark their project. The list lives in the script, and a test
    greps the code for markdown it reads and fails on an undeclared file.
  - Output: a JSON matrix such as `{"apps":["debrief","pitwall"],"crates":true,"ui":true,"kinds":{"pitwall":["rust"]}}`,
    where `kinds` says which half of each app a change reaches (`rust`, `ui` or both), so a leg runs only that half (in
    the trial 42% of app selections needed one half), and `packages` lists the reverse-dependency crates per Cargo
    workspace for a nextest `rdeps(...)` filter.
- **Jobs:** `crates` and `ui` (as today), then one matrix leg per affected app, calling that app's existing gates as a
  shared action with the app folder as input. Inside an app the rule-3 targeting continues (nextest filtersets for the
  touched crates on branches; full app suite on `main`).
- **A shell change triggers all apps' tests:** `crates/paddock-shell` and `ui/projects/paddock-ui/shell` are reached by
  every app's graph, so the matrix has every app. That is the point of the move; with sccache warm, the extra cost is the
  apps' test time, not compilation.
- **Safety net:** `nightly.yml` runs the full matrix (all apps, Chromium and WebKit) on `main` every night and on
  `workflow_dispatch`; release workflows always run the full gates of their app. A wrong filter can hide a failure for at
  most one night and never reaches a release.
- **Base commit:** for a PR, the merge base; for a push to `main`, the last commit with a green `ci` run (Gitea API), so
  a red push is not forgotten by the next one. This is required, not an optimisation: in the trial's replay, 6 of the 9
  failing commits the filter did not select were docs-only commits on a red `main`, whose diff from the last green
  commit selects the project that broke.
- **Proven on history:** the trial replayed the last 50 commits of every repo (351 commits) and the whole history
  (about 1,880 commits): after the two extra dependency kinds above, no failure was missed; the remaining unselected
  failures were flakes, red-`main` inheritance and a tag release job. The script runs in about 0.4 s.
- The X10 shared actions and the research's options 1 (sccache), 2 (garage image) and 4 (path plan) are the building
  blocks; this ADR makes option 4 family-wide.

### 7. Task runner for the monorepo

The question: should a task runner (Nx, moon, Turborepo) drive affected detection and caching instead of scripts?

Criteria: affected detection across Rust crates and Angular packages; caching test and build results across CI runners
and the dev Mac, self-hosted, no SaaS; fit with Gitea Actions; Angular CLI builder compatibility; maintenance burden for
agents; lock-in.

| | Nx (+ `@monodon/rust`) | moon | Turborepo | Plain scripts + sccache |
|---|---|---|---|---|
| Affected, Rust | Plugin reads Cargo path dependencies into the project graph; third-party plugin, recent versions need Nx 22; JS lockfile changes mark Rust projects affected unless named inputs are tuned [1][2] | Rust toolchain parses manifests and lockfiles for hashing; inference of the crate graph is not documented, projects declared in `moon.yml` [3] | None; Rust only through `package.json` scripts | `cargo metadata` path graph, exact |
| Affected, Angular | Native (`@nx/angular`), project graph from TS imports | Project dependencies declared or inferred from `package.json` | Native for npm workspaces | npm workspace graph; coarse (a `ui/` change marks every app) |
| Remote cache, self-hosted | Bucket plugins (`@nx/s3-cache`, `@nx/shared-fs-cache`, ...) affected by CVE-2025-36852 (CREEP, cache poisoning), "cannot be patched", deprecated 2026-05-21 [4]; alternative: our own server for Nx's OpenAPI cache spec (Nx 20.8+) or a third-party one [5] | Any Bazel Remote Execution v2 server, e.g. `bazel-remote` (Go, local disk), since moon 1.30; moon's own service was sunset; TLS called unstable [6][7] | Open-source `ducktors/turborepo-remote-cache` (disk, S3), widely used [8] | No task cache; `sccache` on a local disk (Linux runner, dev Mac) caches compilation, the dominant cost [9] |
| What a cache hit saves us | Skips a whole target (e.g. an app's `cargo test`) if all inputs match; Rust `target/` is not cacheable as an output (too large), so a miss recompiles | Same; moon's docs say do not cache `target/` (about 50 GB) and use sccache [3] | Same, JS tasks only | Compilation only; tests always run |
| Angular CLI builders | `@nx/angular` executors or `nx:run-commands` around `ng` | Runs `ng` as a command | Runs npm scripts | Runs `ng` as today |
| Gitea Actions | Works as a CLI; no Gitea integration needed | Works as a CLI | Works as a CLI | Native |
| Maintenance for agents | Large config surface (`nx.json`, `project.json`, named inputs, migrations), fast major cadence; supply-chain incident: malicious `nx` 20.9-21.8 builds published 2025-08-26 [10] | Smaller; one binary plus `moon.yml` per project; v2 changed config | Small for JS, nothing for Rust | One script with tests, written in our style |
| Lock-in | High (executors, generators, cache format, Nx Cloud as the recommended path) | Medium (task config) | Low | None |

Sources (accessed 2026-10-24):
[1] [`@monodon/rust` on npm](https://npmjs.com/package/@monodon/rust) and [Nx: add Rust to an Nx workspace](https://nx.dev/docs/kb/add-rust-to-nx-workspace);
[2] [enapi: Maximizing efficiency with Nx](https://blog.enapi.com/maximizing-efficiency-with-nx-our-monorepo-strategy-6b5b9830a273) (named inputs for Rust);
[3] [moon Rust handbook](https://moonrepo.dev/docs/guides/rust/handbook) (moon v2 docs, 2026);
[4] [Nx: CVE-2025-36852 CREEP](https://nx.dev/blog/cve-2025-36852-critical-cache-poisoning-vulnerability-creep) (2025-06-26) and [Nx remote cache plugins](https://nx.dev/docs/reference/remote-cache-plugins);
[5] [Nx: self-hosted caching (OpenAPI spec)](https://nx.dev/docs/guides/tasks--caching/self-hosted-caching);
[6] [moon: remote cache](https://moonrepo.dev/docs/guides/remote-cache);
[7] [moon: moonbase sunset](https://moonrepo.dev/blog/moonbase-sunset);
[8] [Turborepo: remote caching](https://turborepo.dev/docs/features/remote-caching) and [ducktors/turborepo-remote-cache](https://hub.docker.com/r/ducktors/turborepo-remote-cache);
[9] [Paddock research: CI speed](../research/2026-10-ci-speed.md) (2026-10-24), [Depot: faster Rust builds in CI](https://depot.dev/blog/guide-to-faster-rust-builds-in-ci.md) (2025-08-01);
[10] [Wiz: s1ngularity](https://www.wiz.io/blog/s1ngularity-supply-chain-attack) (2025-08).
Not verified: whether `@monodon/rust` handles `[workspace.dependencies]` inheritance and several Cargo workspaces in one
repo. Answered by the trial: moon v2 does not infer the crate graph across Cargo workspaces.

**Decision (after the trial, 2026-10-25): plain scripts plus sccache in CI. moon is a no-go for now and stays the
fallback.** The trial ran moon 2.6.4 with a local `bazel-remote` on Pitwall: remote hits work (clippy plus `ng test` in
2 s instead of 11 s after a shared-crate change), but the whole saving is "do not run the UI tests for a Rust-only
change", which `affected.mjs` reports itself (`kinds`, §6), so over the scripts with that split moon saves 0%, below
the 20% bar. moon v2 also does not infer dependencies across separate Cargo workspaces (nor npm or `tsconfig` edges): every
edge would be a hand-written `dependsOn`, and the cache's correctness would rest on hand-written `inputs` (a missing input
replays a false green). The reasons below still hold.

- Our cost is Rust compilation, which no task runner caches (all advise against caching `target/`). sccache does, on a
  local disk, with no server and no new trust boundary.
- A task-result cache mostly saves re-running unchanged tests. Affected CI already skips unaffected apps; inside an
  affected app, re-running its tests is what we want after a shared change.
- The scripts' graph is exact for Rust (Cargo's own) and good enough for Angular (any `ui/` change marks all apps, which
  rule 43 wants anyway).
- Nx: the self-hosted cache path is the weakest part (the bucket plugins are deprecated for an unpatchable poisoning
  flaw; the replacement is a server we would write or trust), the Rust plugin is third-party, and its config surface
  and churn are high for agents. Turborepo does not cover Rust. moon is the best fit of the three (native Rust and JS,
  a plain `bazel-remote` on the NAS as the cache, low lock-in), and is the one to adopt if we later want a task cache.

**Spike (1 day, before step 2 of §11), on a throw-away branch with Debrief, Pitwall and `crates/` imported** (done
2026-10-25 with all eight repos; results in [2026-10-monorepo-spike.md](../research/2026-10-monorepo-spike.md); the
Linux runner and the Mac host runner were not measured and are checked when `ci.yml` is built):

1. `scripts/affected.mjs` on the last 50 commits of each repo replayed into the layout: compare its project set with
   a hand check of 10 of them; it must never miss a project whose tests failed in the real history.
2. sccache hit rate and wall time for `cargo test` of Pitwall after a `crates/paddock-jobs` change, cold vs warm, on the
   Linux runner and the dev Mac (two worktrees, different paths).
3. moon v2 on the same tree, with `bazel-remote` in a container we start and stop: does it infer the Cargo graph,
   how much config does it need, and what does a remote hit save on Pitwall's `ng test` and `cargo clippy`? Adopt moon
   only if it saves more than 20% of the affected CI wall time over step 2's result.
4. npm workspaces: build `ui` and Debrief's and Pitwall's `app/` from one lock; `ng build`, `ng test` and Playwright work.

### 8. Renovate

One root `renovate.json` (the apps' configs are identical today):

- Managers find every `Cargo.toml`, `package.json` and workflow; `ignorePaths` keeps `**/node_modules/**` and adds
  `apps/*/target/**`.
- Groups: Angular (all workspaces, one PR, as rule 39 wants one version), Tauri (Rust crates and `@tauri-apps/*`
  together, across all workspaces), Rust patch updates per workspace, GitHub Actions digests (existing preset).
- `lockFileMaintenance` weekly, one PR per lockfile (`additionalBranchPrefix: "{{packageFileDir}}-"`), so an app in
  release freeze can hold its own.
- A Renovate PR goes through affected CI like any change; a group PR touching all apps runs the full matrix.
- The Paddock-release PRs into apps (ADR 0006) disappear.

### 9. Ownership, agents and docs

- **`CODEOWNERS`** at the root (Gitea reads the first of `./CODEOWNERS`, `./docs/CODEOWNERS`, `./.gitea/CODEOWNERS`
  and requests the owners as PR reviewers): `apps/<app>/` per app, `crates/`, `ui/`, root configs and `.gitea/` to the
  coordinator. Gitea's patterns are **Go regular expressions**, not GitHub's globs: write `apps/debrief/.* @decryp7`,
  `\.gitea/.* @decryp7`, and escape dots. With one human owner it is not a gate; it is the written form of the existing rule
  "each task owns an explicit set of files": an agent brief names its owned paths, and shared integration points
  (`crates/*/src/lib.rs`, root `package.json`, `.gitea/`, `renovate.json`) are changed by the integrating agent only.
- **Worktrees:** every agent still works in its own worktree of `paddock` (rule 2). Worktrees share one object store, so
  a new one costs only its checkout (about 300 MB for the whole tree, without build output). For app work, an agent uses
  a sparse checkout: `git sparse-checkout set --cone crates ui scripts docs apps/<app>`. `CARGO_TARGET_DIR` is per
  worktree and per app (`$TMPDIR/cargo/<worktree>/<app>`) and is kept between tasks: reusing a worktree for the same app
  is what makes local rebuilds fast. sccache is shared but gives almost nothing across worktrees on the Mac (its key
  carries both paths, §1); do not count on it locally.
- **AGENTS.md:** one root `AGENTS.md` with the family rules (rules.md is then included by link, not copied) and the
  engineering rules common to all apps. Each `apps/<app>/AGENTS.md` keeps only the app's own part (architecture, where
  logic lives, app-specific checks). Agents read the root file and the nearest one. The "copy every rule into every
  app's AGENTS.md" step of rule 8 ends.
- Memory and briefs that name app repos are updated to `paddock/apps/<app>` paths in the same change as the move.

### 10. Rules that change (in `docs/rules.md` when this ADR is accepted)

- **Rule 2:** "Every agent works in its own git worktree of `paddock` (sparse to its app where possible), with its own
  `CARGO_TARGET_DIR` per app and its own e2e port; the main checkout stays on `main` for the coordinator."
- **Rule 3:** affected tests are what `scripts/affected.mjs` selects, plus the touched specs; unchanged otherwise.
- **Rule 8:** rules live once in `docs/rules.md` and the root `AGENTS.md`; no per-app copies.
- **Rule 9:** "Releases only from a `<app>-vX.Y.Z` tag in CI ..."; the rest unchanged.
- **Rule 39 and 42:** shared code is moved to `crates/` or `ui/` and adopted by the app **in the same change**; no
  publish-and-consume step. "Consume the published version" becomes "use the in-tree version".
- **Rule 1** is unchanged: one app at a time is about attention and load, not repos.

### 11. Migration

Order: after shell 0.3.0 is tagged and every app branch is merged or handed off. Apps are imported one at a time, the
smallest first, Debrief last (largest, most CI).

0. **Spike** (§7), on a branch; decides moon vs scripts and npm workspaces vs per-app locks. Done 2026-10-25: plain
   scripts, one root npm workspace.
1. **Freeze and tag:** tag `paddock` as `pre-monorepo`; no open branches in any app repo (merged, or pushed as hand-off
   branches and listed).
2. **Build the frame** in `paddock`: root `package.json` workspaces, `scripts/affected.mjs` with tests, `ci.yml` plan
   and matrix, `nightly.yml`, shared actions (X10), `scripts/release/` (X9), root `renovate.json`, `CODEOWNERS`, root
   `AGENTS.md`. Proven with `apps/shell-demo` as the first matrix leg.
3. **Import Pacenotes and Sighter** (docs only, no CI): import (§2), pointer commit in the old repo.
4. **Import Spotter, then Pitcrew, then Dyno, then Pitwall.** For each: import; switch shared dependencies to path and
   workspace; move its `build.yml` into the matrix leg and `release-<app>.yml`; run its full gates locally and one CI
   bundle build (`workflow_dispatch`); add the endpoint pair (§4); freeze the old repo (§3 stage 1). Its next real
   release is the first from the monorepo and starts the bridge.
5. **Import Debrief** the same way, with its extra workflows (`bench`, `corpus`, `fuzz`, `mutants`) as nightly jobs
   scoped to `apps/debrief`.
6. **Retire:** `canary.yml`, the registry steps of `publish.yml`, ADR 0006 consumption text; update rules (§10).
7. **After the bridge period:** archive the old repos (§3 stage 2).

**Rollback point:** until an app's first release from the monorepo (end of step 4 for that app), rollback is: revert its
import merge in `paddock` (or reset `main` to `pre-monorepo` if nothing else landed), remove the pointer commit and
branch protection in the old repo, and continue there. Commits made in `apps/<app>/` meanwhile are carried back with
`git format-patch --relative=apps/<app>`. After its first monorepo release, rollback is still possible because the old
endpoint keeps working (§4), but the old repo would have to take over publishing `latest.json`; treat that as a
forward fix, not a rollback.

### 12. Risks

| Risk | Mitigation |
|---|---|
| Installed apps stop updating | Dual `latest.json` publishing, endpoint pair, bridge of two minors or six months; release verification fetches both URLs |
| A wrong affected filter hides a failure | Nightly full run; release workflows run full gates; affected script has tests on replayed history |
| One red app blocks everyone's `main` | Matrix legs are per app; a red leg blocks only pushes that affect that app (rule 4 still applies to whoever broke it) |
| CI load: a shell change runs every app | sccache warm cache; the apps' tests run in parallel matrix legs within rule-41 limits; WebKit only nightly and on release |
| Checkout and agent disk use | Sparse checkouts, shared object store, per-app target dirs cleaned by `clean-build-dirs`; no LFS needed |
| Lost history or tags | filter-repo on a fresh clone, compare `git log --oneline | wc -l` and tag lists before and after; old repos stay |
| Old commit hashes in issues and docs no longer resolve in `paddock` | Old repos stay readable; import merge names the last old hash; a `docs/history.md` table maps repo to import commit |
| Debrief 1.1 or another release in flight during the move | Migration starts only after 0.3.0 and with no release in progress; one app at a time; its release waits for its own import step |
| Gitea features not as assumed (CODEOWNERS, cross-repo token for the bridge upload, artifact v4) | Step 0 checked CODEOWNERS (supported, regex patterns) and the token (user-wide `write:repository` passes for other repos); artifact v4 is checked in step 2; the bridge can also use a dedicated release-only token per old repo |
| History larger than planned (703 MiB, mostly screenshots) | Accepted at import; screenshots recaptured only per release; LFS revisited above 1 GB |
| Renovate opens too many PRs at once | Grouping and weekly schedule as today; one `lockFileMaintenance` PR per lockfile |
| Feature unification sneaks in via path dependencies | Per-app workspaces (§1); `cargo tree -e features` diff in the spike for one app before and after |

### 13. Effort

Agent-days, one agent unless noted; wall time with 2 to 3 agents in parallel where steps allow.

| Step | Effort |
|---|---|
| 0 Spike (scripts, sccache, moon, npm workspaces) | 1.5 |
| 2 Frame: affected script and tests, ci/nightly workflows, shared release scripts, Renovate, CODEOWNERS, AGENTS.md | 4 |
| 3 Pacenotes and Sighter | 0.5 |
| 4 Spotter 1, Pitcrew 1, Dyno 1.5, Pitwall 1.5 | 5 |
| 5 Debrief (largest CI, extra workflows, plugin SDK job) | 3 |
| 6 Retire canary and publishing, rules and docs | 1 |
| First monorepo release per app (bridge check, both endpoints verified) | 0.5 per app, about 2.5 |
| **Total** | **about 17.5 agent-days; about 2 weeks wall time** |

Steps 0 to 2 are on the critical path (about 6 days). Most of step 2 is work already planned as X9 and X10 in
[shared-next.md](../plans/shared-next.md), so the extra cost of the monorepo over that plan is about 10 agent-days.

## Consequences

- A shared change and its adoption are one commit and one CI run; the shell is the same version in every app at all
  times; the canary and the Paddock-release Renovate PRs go away.
- Every shared change now tests every consumer before it lands; shared work gets slower to merge and app work
  does not.
- Apps can no longer stay on an old shell. A breaking shell change must update every app in the same change (or be
  added beside the old API first). That is intended (rule 40: one mechanism), but it makes large shell refactors bigger.
- App release trains stay independent (own tags, own notes, own updater URL).
- ADR 0006's consumption and versioning parts are replaced by §4 and §5; its rollout order stays.

## Alternatives considered

- **Keep separate repos, improve the registry flow** (faster publishing, auto-merge Renovate PRs). Keeps the
  cross-repo lag and the canary; rejected by the user's decision.
- **One root Cargo workspace.** Simplest lockfile story, but unifies features across apps (Debrief's `tauri/unstable`,
  DuckDB) and makes every lint and test workspace-wide. Rejected for now (§1).
- **`git subtree` import.** Keeps old paths in history; filter-repo gives clean `apps/<app>` history. Rejected.
- **Import without history** (snapshot). Loses blame and the lesson trail behind many rules. Rejected.
- **A new repo instead of `paddock`.** `paddock` already holds the shared code, the rules and the shell demo in the
  target layout. Rejected.
- **Redirect old release URLs** (proxy rules on the Gitea host). Possible with a reverse-proxy rewrite for
  `/decryp7/<app>/releases/download/latest/latest.json`, but ties updates to server config outside CI. Kept as a fallback
  if the dual upload fails.

## Amendment: one updater key for every app (user, 2026-10-25)

Every app signs its updates with one shared Tauri updater key pair, stored as `paddock` repo secrets
`PADDOCK_TAURI_SIGNING_PRIVATE_KEY`, `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `PADDOCK_TAURI_UPDATER_PUBKEY`
(the owner keeps the key file as the backup). Each app's release job maps them to Tauri's standard names. The apps
shipped so far each have their own key, and an installed copy only accepts updates signed with the key it was built
with, so each app moves over once: its first monorepo release is a **transition release** signed with the app's old
key (from its old repo's secrets) and built with the shared public key. The update bridge serves that release to old
installs through the old repo's `latest.json`; every later release is signed with the shared key only. A transition
release must never be skipped; the bridge keeps pointing old installs at it until the bridge period ends. New apps
(Pacenotes, Sighter, the demos) use the shared key from their first release.

## Amendment: Debrief first (user, 2026-10-25)

The user changed the order of §11 to reach a Debrief release sooner. After the frame (step 2), **Debrief is imported
next** (step 5 moves before steps 3 and 4), with the same checklist (§2) and the same per-app steps as step 4: the
history-preserving import into `apps/debrief` with tags renamed to `debrief-v*`; its CI as an affected matrix leg; its
`bench`, `corpus`, `fuzz` and `mutants` workflows as nightly jobs scoped to `apps/debrief`; a `release-debrief.yml` for
`debrief-v*` tags; the endpoint pair (§4), so installed copies keep updating through the old repo's `latest.json`; and
the pointer commit in the old repo (§3 stage 1) last, after the import's CI is green. Before the import, Debrief's
`test/x50-shell-baseline` is merged into its `main`, and `perf/test-speed` is rebased or merged if it still applies
cleanly and passes Debrief's affected tests (otherwise it stays a hand-off branch, [monorepo-handoff.md](../plans/monorepo-handoff.md)).
The other six apps (Pacenotes and Sighter, then Spotter, Pitcrew, Dyno and Pitwall) are imported after that Debrief
release. The frame and the Debrief import land on `main` as two separate merges, each verified locally and by one CI run.
