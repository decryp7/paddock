# Monorepo spike (ADR 0014 §7 and §11 step 0)

Date: 2026-10-25. The trial ADR [0014](../adr/0014-monorepo-under-paddock.md) asks for before the frame is built. Nothing
real was changed: every step ran on fresh clones under `/Volumes/d8sst7/dev-targets/monorepo-spike/` on the dev Mac (Mac
mini, 14 cores, `CARGO_BUILD_JOBS=2`, each heavy step behind the machine-wide 5-slot semaphore). Nothing was pushed, no
Gitea repo, release or tag was created. The throw-away monorepo is kept at `monorepo-spike/mono` (branch
`spike/monorepo`) for reuse.

**Measurement noise:** the 1-minute load was 18 to 35 for most of the run (other agents' builds), so wall times vary by
up to about 2x between identical steps (an uncached Pitwall build took 286 s, 326 s, 356 s and 365 s). Hit rates and
counts are exact; treat times as rough.

## Summary: go / no-go per ADR criterion

| ADR criterion | Result | Verdict |
|---|---|---|
| §2 history import with filter-repo, tags renamed | 8 repos imported in seconds, 2,178 commits, 11 tags renamed, `log` and `blame` work under `apps/<app>/` | **Go** |
| §2 "about 100 MB packed" | **703 MiB** packed after `gc --aggressive`; 99% of it is PNG screenshot history (696 MB), 203 MB of PNG in `HEAD` | **Go, with the size estimate corrected** (no LFS needed for function, see below) |
| §7.1 `affected.mjs` "never misses a project whose tests failed" | Two missing edge kinds found and fixed: a Tauri shell embedding `ui/dist` (a real CI failure in the replay) and a `tsconfig` path to `ui/dist` (Spotter's edge vanished after the npm switch). After the fixes, 0 misses in 351 replayed commits; 9 failing commits not selected are all flakes, red-`main` inheritance or a tag release, which the ADR's last-green base and release gates cover | **Go**, with the rule changes in §7 |
| §7.2 sccache shares compiled dependencies across apps and worktrees | **No.** Hits need the same checkout path **and** the same `CARGO_TARGET_DIR` path: 100% then, 6.5% otherwise (other worktree, other target dir, separate repo). Across two apps in one checkout 6.1%, because feature sets differ. `SCCACHE_BASEDIRS` (0.18) does not fix it for Rust | **Go for CI with fixed paths; no-go for the dev-Mac claim.** §1's "sccache shares compiled dependencies across them, which is most of what one workspace would have saved" is wrong as written |
| §7.3 moon v2 + `bazel-remote`, adopt only if > 20% saving | moon does not infer the crate graph across Cargo workspaces (or npm/`tsconfig` edges). Remote hits work (2 s vs 11 s after a shared-crate change), but all of that saving comes from not running UI tests for a Rust-only change, which `affected.mjs` now reports itself (`kinds`); over the scripts with that split moon saves 0% | **No-go** (below the 20% bar against plain scripts with the rust/ui split); keep moon as the fallback |
| §7.4 one root npm workspace, `ng build`, `ng test`, Playwright | One lock, 948 packages, one copy of every Angular package; 600 MB `node_modules` instead of 2.6 GB; all five apps build, 3 Vitest suites (6,261 tests) and 2 Playwright sets pass after one mechanical fix | **Go** |
| §9 CODEOWNERS on our Gitea | Gitea reports API version 28.2.0; CODEOWNERS is supported, but the rules are **Go regular expressions**, not gitignore globs | **Go**, write it as regexes |
| §4 bridge: token can upload `latest.json` to another repo | The usual token has `write:repository, write:package, read:user`; Gitea tokens are not repo-scoped, so it can write releases of every repo the user owns (checked without uploading) | **Go**, but see the `latest` finding below |

Findings that change the ADR are collected in [§7](#7-changes-to-adr-0014).

## 1. History import

Fresh `git clone` of the eight remotes (1 min 30 s), `git filter-repo --to-subdirectory-filter apps/<app>
--tag-rename '':'<app>-'` per app (git-filter-repo 2.47.0, installed with Homebrew for the spike), then a merge per app
into a clone of `paddock`, smallest first, each merge naming the old repo's last commit in its body.

| Step | Measured |
|---|---|
| filter-repo per repo | 0.3 s (Pacenotes) to 1.8 s (Debrief, 1,357 commits) |
| Seven import merges | 8 s |
| `git gc --aggressive --prune=now` | 172 s |
| Commits / tags | 2,178 commits; tags `debrief-v1.0.0`, `-v1.0.1`, `-v1.1.1`, `pitcrew-v1.0.0`, `-v1.0.1`, `pitwall-v1.0.0` to `-v1.0.3`, Paddock's `v0.1.0`, `v0.2.0` unchanged (Spotter, Dyno, Sighter, Pacenotes have no tags) |
| Packed size | 789 MiB after the merges, **703 MiB** after aggressive gc. Per repo today: Dyno 234, Debrief 187, Paddock 124, Pitwall 101, Spotter 61, Sighter 50, Pitcrew 47, Pacenotes 27 MiB |
| Working tree | 8,093 tracked files; full worktree 324 MB in 4 s; sparse worktree (`crates ui scripts docs apps/pitwall`) 58 MB in 1 s; local full clone 16 s, 1.0 GB |
| History tools | `git log -- apps/debrief/crates/debrief-core/src/lib.rs` and `git blame apps/debrief/README.md` show the original commits without `--follow` |

**Where the size is:** PNG blobs are 696 of the 703 MiB (2,894 blobs). By folder: `apps/dyno/docs/screenshots` 234 MB
(878 versions), `apps/debrief/docs/screenshots` 132 MB, `apps/pitwall/docs/screenshots` 51 MB, `icons/screenshots`
38 MB, `apps/pitcrew/docs/screenshots` 38 MB, `apps/spotter/docs/screenshots` 36 MB. Code is small (Rust 8 MB, TS
6.5 MB). Each recapture of an app's screenshot set adds its PNGs again (tens of MB) to every clone for good. The ADR's
table counted only packs, and several repos held most objects loose ("loose", "+ loose" in its Pack column), so its
sum understated the history.

Options, for the owner: (a) accept 700 MB (a one-time clone; worktrees share the object store, so agents pay it once);
(b) at import, drop old screenshot versions (`filter-repo --path-glob` on `docs/screenshots/**` for all but the last
commit); this loses image history only, about 500 MB saved; (c) LFS for `**/docs/screenshots/**` and `**/docs/images/**`
from the import on. My recommendation: (a) for the import, plus a rule that screenshot sets are recaptured only for a
release, and revisit (c) if the pack passes 1 GB.

Problems the import surfaced (none blocks it; all go into the import commits):

- **Sibling-repo paths break.** 20 imports in the diagram sources of every app (`apps/*/docs/diagrams/src/*.mjs`) read
  `../../../../paddock/diagrams/tools/*.mjs` and `../../../../paddock/docs/images/src/*`, and `canary.yml` walks
  `../../../paddock/crates`. Inside the monorepo they must become `../../../../../diagrams/tools/...`. A grep for
  `\.\./(\.\./)*(paddock|debrief|pitwall|...)/` belongs in the import checklist.
- **The registry config must move to the root.** `crates/Cargo.toml` declares its own crates with
  `registry = "decryp7"`. An app workspace that depends on a shared crate by path fails `cargo metadata` with "registry
  index was not found in any configuration: `decryp7`", because the registry is defined in `crates/.cargo/config.toml`,
  which cargo does not read from `apps/<app>/`. Moving it to the root `.cargo/config.toml` fixed it (or drop `registry`
  from the workspace dependencies once §5 stops publishing). Cargo finds config from the **current directory**, not the
  manifest, so every cargo call (including `affected.mjs`) must run inside the repo.
- **The path in the ADR is one level short for the apps.** From `apps/<app>/app/src-tauri/` the shared crates are
  `../../../../crates/<crate>`; `../../../crates` is right only for `apps/shell-demo/src-tauri`.
- **Feature unification did not sneak in:** `cargo tree -e features` of Pitwall before and after adding
  `paddock-jobs` by path differs only by the two `paddock-jobs` lines.
- The npm package name of Debrief's UI is `app`; npm workspaces need unique names (renamed to `debrief-app` in the
  spike). Root `.gitignore` needs `node_modules/`.

## 2. `scripts/affected.mjs`

Written as the ADR specifies, in the spike repo: `scripts/affected.mjs` (about 240 lines, no dependencies) and
`scripts/affected.test.mjs` (9 tests, all pass). It reads the graph from the working tree once per call (0.4 s warm,
1.8 s cold, including `cargo metadata --no-deps --offline` of all nine Cargo workspaces) and prints
`{"apps":[...],"crates":bool,"ui":bool,"icons":bool,"docs":bool,"packages":{"<workspace>":[crates]}}`. `packages` is the
reverse-dependency closure per Cargo workspace, ready for a nextest `-E 'rdeps(...)'` filter.

Graph sources: Cargo path dependencies of every workspace (crosses workspaces, so `crates/paddock-jobs` reaches
`apps/pitwall/app/src-tauri`); npm dependencies by package name across every tracked `package.json`; and, added after
the replay found a miss, **Tauri `build.frontendDist`**: a shell whose `tauri.conf.json` points into another package
depends on it (`apps/shell-demo` embeds `ui/dist/shell-demo`); and `tsconfig*.json`/`angular.json` paths that point
into another package (Spotter's `tsconfig` path to `ui/dist/paddock-ui` after the npm switch, see §5). It also reports
`kinds` per app (`rust`, `ui` or both), so a matrix leg can run only the half a change reaches.

Rules: `.gitea/**`, `.cargo/**`, `rust-toolchain*`, root `package.json`, `.gitattributes`, `scripts/release/**` and the
script itself mark everything; `release-<app>.yml` / `<app>-release.yml` mark one app; `.gitignore`/`.editorconfig`
mark nothing; root `package-lock.json` marks only workspaces whose resolved dependency closure changed (lock v3,
resolved per workspace with node's lookup order); any change in `ui/` marks every `ui` package and so every consumer;
`scripts/**` marks `ui` (its `test:scripts` runs them); docs mark only the link check, **except declared doc inputs**.
Unknown root files mark everything (fail safe).

**Replay.** The spike gives Pitwall and Pitcrew a path dependency on `paddock-jobs` (no app consumes a shared crate yet,
so without it no Rust edge crosses a project boundary) and keeps Spotter's `@decryp7/paddock-ui` edge. Each repo's
last 50 first-parent commits were diffed against their parent and against the last ancestor with a green `build.yml`/
`ci.yml` run (the ADR's base), and compared with the real CI result of the old commit (Gitea API, read-only; the API
keeps 13 to 340 runs per repo, so older commits have no result).

| Repo | Commits | Nothing to run (docs only) | Commits with a failed CI run | Failed but not selected |
|---|---|---|---|---|
| debrief | 50 | 3 | 4 | 2 → 1 with last-green base; both flaky (same SHA later green) |
| pitwall | 50 | 13 | 4 | 1: red `main` inherited from the commit before (see below) |
| pitcrew | 50 | 12 | 5 | 1: the empty `chore(release): v1.0.0` commit, failed in the tag's release job |
| spotter | 50 | 10 | 8 | 0 |
| dyno | 50 | 12 | 4 | 0 |
| sighter | 34 | 33 | 0 | 0 |
| pacenotes | 17 | 16 | 0 | 0 |
| paddock | 50 | 27 | 6 | 5: docs commits on a red `main` |

- **One real miss, fixed.** Paddock `234f6911` (a `ui/` shell change) broke the `shell demo app` job ("The UI the app
  embeds"). The first version selected `ui` and Spotter but not `shell-demo`, which has no npm or Cargo edge to `ui/`;
  it builds `../../../ui/dist/shell-demo`. The `frontendDist` edge fixes it; the 5 following docs-only Paddock commits
  failed only because `main` stayed red. With the ADR's last-green base their diff reaches back to `234f6911` and
  selects `shell-demo`; the replay could not show that because the API no longer has the older green runs, so it fell
  back to the parent. The same holds for Pitwall `2c7b6312` (docs, after the failing `d52e8c70`).
- **Docs rule.** Code and tests read markdown: `apps/shell-demo/CHANGELOG.md` (`include_str!` in the shell, What's
  New), `apps/debrief/CHANGELOG.md` (`whats-new.mjs` and its test), `apps/debrief/docs/ai/sql-views.md` (read by
  `debrief-ai/tests/sql_tool.rs`), `apps/debrief/docs/testing/quarantine.md` (`scripts/ci/check-quarantine`),
  `app/src-tauri/resources/THIRD_PARTY_NOTICES.md` (`include_str!` in `legal.rs`), `samples/README.md` (a
  `debrief-core` test). The ADR's literal rule ("`**/*.md` and `docs/**` mark nothing") would therefore be wrong for
  those files. Over the whole history (about 1,880 non-merge commits) it would have skipped CI on 2 commits that changed
  only such files (Debrief's notices regeneration `43d92bab`, a `samples/README.md` edit); neither failed CI, so the
  replay could not catch it, but the next edit of `sql-views.md` alone would ship untested. The script keeps a declared
  `DOC_INPUTS` list (an app's or crate's `CHANGELOG.md`, `*.md` under `resources|samples|src|tests|templates|e2e`, the
  two Debrief docs; Paddock's root `CHANGELOG.md` stays docs) and a test that greps the code for markdown it reads and
  fails on an undeclared one. One harmless over-selection remains (`apps/pitcrew/docs/diagrams/src/README.md`).
- **Precision.** Over the whole history: 348 commits (18.5%) select nothing, 1,476 select one project, 40 select two to
  six, 18 select everything (root CI, toolchain or root `package.json` changes). Inside Debrief a commit selects a median of 9% of its 46
  crates (mean 17%) for `rdeps`; Pitwall 20%, Pitcrew 25%; a shared-crate change selects 73% of `crates/` on average
  (`paddock-jobs` and `paddock-commands` are low in the graph). Coarse by design: a change to the shell-demo UI in
  `ui/projects/shell-demo` also marks Spotter (any `ui/` change marks every consumer, as the ADR intends).
- **Hand check of 13 commits** (one or more per repo, plus the three Paddock commits that cross into apps): all
  selections were what I would choose; the only disagreement was the `shell-demo` miss above.

## 3. sccache on the dev Mac

Private sccache 0.18 server (own port and cache dir), `CARGO_INCREMENTAL=0`, `cargo test --workspace --no-run` (the
compile part of the test job), each step with a fresh target directory unless noted.

| Step | Wall | Hit rate (hits / cacheable) |
|---|---|---|
| N1 Pitwall, no sccache | 356 s | n/a |
| A1 Pitwall in `mono`, empty cache | 286 s | 0% (0/400; 103 not cacheable: bin, proc-macro, cdylib) |
| A2 Pitcrew in `mono`, after A1 | 171 s | **6.1%** (25/413) |
| A3 Pitwall in a second worktree `mono2` | 189 s | **6.5%** (26/400) |
| B1/B2 Pitwall, Pitcrew in separate old-layout clones | 302 s / 236 s | 6.5% / 6.4% |
| C1 `paddock-jobs` edited, Pitwall rebuilt in its kept target | **13 s** | cargo rebuilt 4 crates |
| C2 same change, fresh target dir in the same checkout, warm cache | 224 s | 6.5% |
| C3 same change, Pitcrew in its kept target | 52 s | |
| D2 Pitwall, **same checkout and same target path**, fresh target, warm cache | 175 s (vs 365 s cold) | **100%** (400/400) |
| D3 Pitwall in `mono2` with `SCCACHE_BASEDIRS=<mono>:<mono2>`, targets inside each worktree | 301 s | 6.5% |

- **sccache hits only when the checkout path and the target directory path are both identical.** C2 changes nothing but
  the target dir name and loses 93.5% of hits; D2 restores them. Presumably the 26 hits that survive are leaf crates
  whose arguments carry no target path, while the rest carry it through `OUT_DIR` and `--extern`/`-L` paths (not
  traced further). `SCCACHE_BASEDIRS` (strips base directories from paths in the cache key) did not change the result
  for rustc in 0.18, with both worktrees and their target dirs below the listed bases.
- **Across apps the gain is small even with equal paths:** Pitwall and Pitcrew share 553 of about 594 locked
  crate versions, but 33 of the 304 shared crate versions resolve different features (e.g. `hashbrown`, `getrandom`,
  `idna`, `nix`, `hyper`, `objc2-app-kit`), and cargo's metadata hash propagates that to every dependent (`tauri`,
  `wry`, ...). That is per-app feature resolution working as §1 wants; it means sccache does not replace a shared
  workspace's dependency reuse.
- **With a 100% hit rate the build still takes half the time** (175 s vs 365 s): linking, build scripts and the 103
  non-cacheable units (proc-macros, bins, the cdylib) remain. That matches the research note (warm cache cuts compile,
  not link).
- **What works:** a kept target directory per app per worktree (C1, C3: 13 s and 52 s after a shared-crate change).
  That is cargo's own incremental reuse, no sccache needed.

Consequences: on the **Linux CI containers** the checkout path is the same in every run and `CARGO_TARGET_DIR` can be
fixed (e.g. `/target/<app>`), so a bind-mounted sccache should reach D2's 100% and save about
half of each Rust job's compile; that is still worth doing (research option 1), but it must set the target dir
explicitly. On the **dev Mac**, sccache gives almost nothing across worktrees; the ADR's per-worktree
`$TMPDIR/cargo/<worktree>/<app>` layout guarantees misses. Keeping a target per app per worktree and reusing worktrees
for the same app is what saves time. The Linux runner and a Mac host runner were not measured in this spike (the ADR
asked for both); the D2 result says what to check there: identical paths between runs.

## 4. moon v2 with a local `bazel-remote`

moon 2.6.4 (npm `@moonrepo/cli`, installed into the scratch folder) with `bazel-remote` 2.6.2 in a container started
and removed by the script (`--storage_mode uncompressed`, gRPC on `127.0.0.1:9092`). Config: `.moon/workspace.yml`
(projects, `remote.host`), `.moon/toolchains.yml` (`rust: {}`) and a `moon.yml` per project with explicit `inputs`
(65 lines for four projects). Two tasks for Pitwall: `clippy` (`cargo clippy --workspace --all-targets -D warnings`,
inputs: its Cargo files, `crates/**`, `app/src-tauri/**`, `/crates/**`) and `ngtest` (`ng test`, inputs: `app/src/**`,
app config, `/ui/projects/paddock-ui/**`, `/package-lock.json`). Each moon run deleted `.moon/cache` first, so every
hit came from `bazel-remote`, as on a fresh CI runner. Same warm target directory and sccache for both sides.

| Scenario | Plain commands (clippy + ng test) | moon |
|---|---|---|
| First run, empty caches | 183 s (ran first, so it compiled the check profile) | 11 s (target now warm), both tasks run, 276 KB uploaded |
| No change, fresh local cache | (would re-run both, about 11 s) | **1 s**: both tasks "cached from remote" |
| `crates/paddock-jobs` changed | 11 s | **2 s**: clippy re-runs (1.4 s), `ng test` (6 s of Vitest) cached from remote |
| Pitwall UI file changed | 11 s | 10 s: `ng test` re-runs, clippy cached |

- **Graph inference: no.** `moon project pitwall` shows "Depends on: —" although Pitwall's Cargo graph reaches
  `crates/paddock-jobs` by path; moon v2's Rust toolchain does not infer dependencies across separate Cargo
  workspaces (its handbook assumes one root workspace) and did not infer the npm/`tsconfig` edges either. Every edge
  would have to be written as `dependsOn` by hand, and the cache's correctness rests on hand-written `inputs` globs:
  a missing input is a false green replayed from the cache, which is worse than a wasted run.
- **What the hit saves is the split, not the cache.** The whole saving in the shared-crate case is "do not run the UI
  tests for a Rust-only change". `affected.mjs` now reports that directly (`kinds`: `rust`, `ui` or both per app):
  in the replay 121 of 285 app selections (42%) need only one half. With that split the plain scripts reach moon's 2 s
  without a cache server. What moon adds beyond it is replaying a task whose inputs did not change on a re-run of the
  same commit, which affected CI and the last-green base already avoid.
- **Against the ADR threshold:** on these two tasks moon saves 82% (11 s to 2 s) over the plain scripts as the ADR
  wrote them, but 0% over the scripts with the rust/ui split, and the tasks it can skip are not on the critical path of
  the real workflow (Pitwall's CI wall time is set by its Rust test and e2e jobs, e2e 2 to 5 min in
  [2026-10-ci-timings.md](2026-10-ci-timings.md), which no task cache skips once the app's inputs changed). Not measured here: a remote hit for e2e (it would need exact inputs for a whole app) and the
  Linux runner.

## 5. One root npm workspace

Root `package.json` with workspaces `ui`, `icons`, `apps/shell-demo` and the five `apps/*/app`; the eight lockfiles
removed; Spotter's `@decryp7/paddock-ui` dependency replaced by a `tsconfig` path to `ui/dist/paddock-ui` (the ADR's
library-in-workspace pattern) and its `.npmrc` dropped.

| Item | Measured |
|---|---|
| `npm install` (npm 11, Node 24.22) | 35 s, 948 packages, one 17,071-line lock (the eight locks were 59,858 lines) |
| `node_modules` | **600 MB** at the root, versus 2.6 GB for the separate installs today (Debrief 542, Pitwall 418, Pitcrew/Spotter/Dyno 410 each, `ui` 383, `icons` 32) |
| Duplicates | one copy each of `@angular/*` 22.2.3/22.2.4, `rxjs`, `typescript` 6.0.3, `vitest` 5.0.5, `@playwright/test` 1.64.1, `@ngrx/signals`, `echarts`, `@tauri-apps/api`. Only `@tauri-apps/cli` twice (2.12.3 pinned exactly in `apps/shell-demo` and `icons`, 2.12.4 elsewhere). 304 nested entries are deep transitive ones (`ansi-styles`, `string-width`, ...) |
| `ng build` of the library, Spotter, Pitcrew | passed first time |
| `ng build` of Pitwall, Dyno, Debrief, `ng test` of Pitwall, Playwright of Pitwall | failed first: `angular.json` styles named `node_modules/@angular/cdk/overlay-prebuilt.css` (Pitwall, Dyno) and `node_modules/dockview/dist/styles/dockview.css` (Debrief), which no longer exist below the app. Writing them as package paths (`@angular/cdk/overlay-prebuilt.css`) fixed all five |
| After the fix | all five apps build; Vitest: Debrief 4,977 tests (460 files), Pitwall 791, Spotter 493 pass; Playwright Chromium: Pitwall 13 tests (2 files), Spotter 16 tests pass |
| Spotter on `paddock-ui` from `HEAD` (0.2.0 source instead of the 0.1.0 package) | builds and its 493 unit and 16 e2e tests pass |

No hoisting or builder problem beyond the style paths; the per-app-lock fallback in §1 is not needed. Notes: npm 11
prints an `install-scripts` warning for `msgpackr-extract` (its native build script needs approval and did not run;
nothing in the checks above needed it);
Debrief's `window-weight.mjs` matches `node_modules/` anywhere in a path, so the new `../../../node_modules/` input paths
still classify. WebKit and the full suites were not run (rule 3: full suites once on the combined result).

## 6. Gitea checks (read-only)

- **Version:** `/api/v1/version` and the swagger document say **28.2.0** (Gitea's API version; not a 1.x number).
- **CODEOWNERS:** supported ([Gitea docs: code owners](https://docs.gitea.com/usage/code-owners), accessed
  2026-10-25). Gitea reads the first of `./CODEOWNERS`, `./docs/CODEOWNERS`, `./.gitea/CODEOWNERS`; it requests the owners
  as reviewers and can be enforced by branch protection. **Patterns are Go regular expressions**, not GitHub's globs:
  `apps/debrief/.*  @decryp7`, and dots must be escaped. Not tried live (it needs a PR).
- **Token scopes:** the token in `~/.zshenv` (`GITEA_TOKEN`) has `write:package, write:repository, read:user` (read
  from the 403 message of a `read:admin` endpoint; the token was never printed). A request to upload an asset to a
  release id that does not exist in `pitwall` and `Pitcrew` returned 404 "not found" rather than 403, so scope
  middleware allows release writes in other repos. Gitea access tokens are user-wide, not per repo, so one token with
  `write:repository` covers the bridge. The automatic Actions job token is meant for the workflow's own repository; that
  was not tested, so plan on a user token stored as a secret.
- **No `latest` release exists.** `pitwall` has releases `v1.0.0` to `v1.0.3` and no tag `latest`, yet
  `/decryp7/pitwall/releases/download/latest/latest.json` returns 200 with v1.0.3's file: **Gitea resolves `latest` in
  that URL to the newest release.** (Spotter and Dyno return 404: no release yet.) This changes §4 of the ADR: in the
  monorepo, `paddock/releases/download/latest/...` would resolve to whichever app released last, so per-app tags such
  as `<app>-latest` (or the app's newest `<app>-vX.Y.Z` release URL) are required, and the bridge upload to an old repo
  must replace `latest.json` on that repo's **newest release** (or create a new release there), because there is no
  moving `latest` release to upload to.

## 7. Changes to ADR 0014

1. **§2 size:** "about 100 MB packed" → about 700 MB, 99% PNG screenshot history. Decide (a) accept, (b) drop old
   screenshot versions at import, or (c) LFS for screenshots; recommended (a) plus recapturing screenshots only per
   release.
2. **§2 import checklist:** rewrite sibling-repo paths (`../../../../paddock/...` in all diagram sources, `canary.yml`);
   move `crates/.cargo/config.toml`'s registry to the root (or drop `registry` from workspace dependencies); unique npm
   package names (Debrief's `app`); `node_modules/` in the root `.gitignore`.
3. **§1 path example:** `../../../../crates/<crate>` from `apps/<app>/app/src-tauri`.
4. **§1 and §9 sccache:** sccache does not share compiled dependencies across apps (features differ) or across worktrees
   and target dirs (path-keyed, `SCCACHE_BASEDIRS` does not help rustc). Replace "sccache shares compiled dependencies
   across them" with: CI uses fixed checkout and `CARGO_TARGET_DIR` paths so sccache hits between runs; locally, keep one
   target dir per app per worktree and reuse worktrees for the same app. Per-worktree `$TMPDIR/cargo/<worktree>/<app>`
   stays, but expect no cross-worktree cache.
5. **§6 affected rules:** add the Tauri `frontendDist` edge; replace "docs mark nothing" with "docs mark nothing except
   declared doc inputs, checked by a test that greps code for markdown it reads"; per-app release workflows mark one app;
   `scripts/**` marks `ui`. Run cargo from the repo root (config discovery). The last-green base is necessary, not an
   optimisation: 6 of 9 failing-but-unselected commits in the replay were red-`main` inheritance.
6. **§1 npm:** keep the root workspace; app `angular.json` files must not name `node_modules/...` paths (write package
   paths). The per-app-lock fallback is not needed.
7. **§4 updater bridge:** Gitea's `releases/download/latest/` means "newest release of the repo", not a release named
   `latest`. Per-app moving tags (`<app>-latest`) are mandatory in `paddock`; the bridge writes `latest.json` to the old
   repo's newest release or creates a new one there. Use a user token with `write:repository` stored as a secret.
8. **§9 CODEOWNERS:** Go regex syntax.
9. **§7 task runner:** plain scripts confirmed; add the rust/ui split (`kinds`) to the plan output so matrix legs run
   only the affected half of an app. Record that moon v2 does not infer cross-workspace Cargo edges (the open question
   in §7), so adopting it later means hand-maintained `dependsOn` and `inputs`.
10. **§1 npm and §6:** the `tsconfig` path to `ui/dist` hides the app's dependency on `ui/` from the npm graph (Spotter's
    edge vanished after the switch). `affected.mjs` therefore also reads `tsconfig*.json` `paths` and `angular.json`
    references outside the package; keep that in the frame and its tests.

## Reproduce

All under `/Volumes/d8sst7/dev-targets/monorepo-spike/`: `import/` (filtered clones), `mono/` (the monorepo, branch
`spike/monorepo`: import merges, then spike commits for the shared crate by path, `scripts/affected.mjs` with tests,
the root npm workspace and the `kinds` split; moon config uncommitted), `tools/` (replay, CI-run fetch, benchmark and
read-only API scripts), `sccache-bench.log` and `moon-bench.log` (raw numbers).
