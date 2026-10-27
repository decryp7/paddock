# Nx, Turborepo or our own scripts for the monorepo (2026-10-27)

Question (user, 2026-10-27): should the Paddock monorepo adopt Nx? Compare Nx and Turborepo with the current setup
(ADR 0014 §6–§7: `scripts/affected.mjs`, sccache, Gitea Actions) and with improving it. The user's main worry is that the
custom scripts may not be maintainable, so maintainability is a first-class criterion here.

Research only: nothing in the build changed. Measurements used the real Gitea run history of `decryp7/paddock` (read
only), the tree of `origin/main` (`932b1728b`), and one throw-away Turborepo experiment on synthetic crates in the
scratch folder. **Verified** means I ran it or read the source; **read** means it comes from documentation or a third
party and was not reproduced.

## Summary and recommendation

| Rank | Option | Bespoke code after | Runner time saved | Effort | Verdict |
|---|---|---|---|---|---|
| **1** | **Consolidate**: move the four imported apps (and most of Debrief) onto the shared kit that already exists (`app-gates`, `app-release.yml`, `conformance/release/*`, the composite actions), and delete the per-app copies | **about −4,900 lines** (of about 21,200) | none directly; fewer places to fix | 4–6 agent-days | **Do it** |
| **2** | **Job-level last green in `affected.mjs`** (a test-result cache built from Gitea's own run records: skip a job whose inputs did not change since it last passed) | +150 to +250 lines | **25% of runner minutes overall, 37% on main and integrate**, about 17% of a wall-time proxy (measured, §4) | 1.5–2 agent-days | **Do it after 1** |
| 3 | moon (ADR 0014's fallback) | config per project, hand-written edges | the same as 2 at best, with finer hashing | 6–10 agent-days | Keep as the fallback; not now |
| 4 | Turborepo | about −1,100 lines (affected script and plan jobs), plus new config and a forced single Cargo workspace | JS tasks only, unless Cargo is restructured | 6–10 agent-days | **No** |
| 5 | Nx (+ `@monodon/rust`) | about −1,100 lines, plus `nx.json`, `project.json` files, a Rust graph plugin we would write, and a cache server | as 2, if the inputs are declared correctly | 10–15 agent-days, plus a migration every major (about every 4–7 months) | **No** |
| – | Bazel / Buck2 | rewrite of every build | most, in theory | weeks | No |

**Recommendation:** do 1, then 2. Do not adopt Nx or Turborepo. moon stays the fallback, as ADR 0014 §7 says. Revisit
only if the job-level cache in 2 turns out too coarse (§9 says how we would know).

Why, in four lines:

- **Most of our bespoke tooling is not something a task runner replaces.** Of about 21,200 non-test lines of CI and build
  tooling, affected detection and the plan jobs are about 1,150 lines (6%). The rest is release, signing, bundle
  verification, sidecars, conformance, OS-specific runner set-up and app checks. Those stay whichever runner we pick.
- **About 4,900 of those lines are copies:** per-app release scripts, bundle checks, commit-lint and per-app workflows
  that duplicate the shared kit built in plan X10. The kit was built, but the imported apps never switched to it.
  Deleting the copies removes four times as much bespoke code as any task runner would.
- **This week's fixes were not affected-detection bugs.** Of 111 `fix:`/`ci:` commits on tooling paths since
  2026-10-13, none fixed a wrong selection in `affected.mjs`. They were about OS specifics, signing and bundles, test
  portability, cache and build-folder paths, the browser and secrets (§5). A task runner fixes none of them.
- **Neither Nx nor Turborepo can read our Rust layout.** Both expect one root Cargo workspace. We have 15 (verified,
  §2), on purpose (ADR 0014 §1: feature unification). The cache savings we measured (§4) can be had from our own
  script, without a cache server and without the poisoning risk that made Nx deprecate its bucket caches.

## 1. What we have today (verified, `origin/main` 932b1728b)

- **Plan:** `scripts/affected.mjs` (561 lines, 341 lines of tests, no dependencies). It reads the Cargo path graph of
  every workspace (`cargo metadata --no-deps`), the npm graph, Tauri `frontendDist` edges and `tsconfig`/`angular.json`
  path edges. It applies the GLOBAL, DOCS and DOC_INPUTS rules and reports apps, `kinds` (the Rust or UI half of an app)
  and per-workspace `packages` for nextest. For a push to `main` its base is the last green `ci.yml` run on main, read
  from the Gitea API (`chooseBase`, `ciRuns`).
- **Jobs:** `ci.yml` (524 lines) has the shared jobs, crate legs on three OSes and one `app` leg per affected app
  through `.gitea/actions/app-gates`. Debrief, Pitwall, Pitcrew, Spotter and Dyno still have their own workflows (1,088,
  557, 462, 575 and 602 lines), each with a `plan` job (41–53 lines) that asks `affected.mjs --ci --app <name>`.
- **Caches:** sccache 0.19 on the runner's local disk (`setup-rust`, with fixed build folders from `build-folder` and
  `stable-workspace` so its keys repeat), npm and Angular caches (`node-caches`), and `cache-restore`/`cache-save` on
  the `actions/cache` server. There is no test-result cache.
- **Cargo workspaces:** 15 roots with `[workspace]` (`crates/`, every app, `apps/debrief/fuzz`, Dyno's plugin template,
  five fuzz workspaces). There is no root `Cargo.toml`: `cargo metadata` at the repo root exits 101 with "could not find
  `Cargo.toml`". There are 136 `Cargo.toml` and 73 `package.json` files.

## 2. Nx today

| Item | Finding | Kind |
|---|---|---|
| Version and licence | `nx` 24.0.2 (2026-10-26), MIT. Majors: 21.0 2025-05-05, 22.0 2025-10-22, 23.0 2026-06-16, 24.0 2026-10-21 | verified (npm registry) |
| Core features | project graph, `nx affected`, task pipeline (`dependsOn`), local computation cache: MIT, free | read ([nx.dev](https://nx.dev/docs/kb/self-hosted-caching)) |
| Remote cache, managed | Nx Cloud (free Hobby tier, free for OSS). The docs call it "recommended for everyone" | read |
| Remote cache, self-hosted buckets | `@nx/s3-cache`, `@nx/shared-fs-cache` (and GCS, Azure): licence "Commercial", **deprecated on npm** 2026-05-22: "The flaw is in their design and cannot be patched" (CVE-2025-36852, CREEP: one read-write credential, no record of which branch wrote an entry, so a PR can poison `main`). Nx advises Nx Cloud or no remote cache | verified (npm metadata); read ([deprecation page](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages), [CVE post](https://nx.dev/blog/cve-2025-36852-critical-cache-poisoning-vulnerability-creep)) |
| Remote cache, own server | `NX_SELF_HOSTED_REMOTE_CACHE_SERVER` with an OpenAPI 1.0 spec (`/v1/cache/{hash}`, bearer token), Nx 20.8+. Community servers: `enxtur/nx-caching-server` (Go, disk), `ikatsuba/nx-cache-server` (S3). Nx calls building your own "high-risk". The poisoning problem is then ours to solve | read ([spec](https://nx.dev/docs/guides/tasks--caching/self-hosted-caching), [enxtur image](https://hub.docker.com/r/enxtur/nx-caching-server)) |
| Old custom runners | `nx-remotecache-custom` is deprecated: "Starting with Nx 21 custom task runners wont work anymore" | verified (npm) |
| Gitea Actions | runs as a CLI; Nx Cloud's distributed agents are not an option. The usual base-SHA helper ([`nrwl/nx-set-shas`](https://github.com/nrwl/nx-set-shas)) reads the GitHub API, so the last-green base for Gitea stays our code | read; Gitea compatibility not tried |
| Supply chain | malicious `nx` builds were published on 2025-08-26 (s1ngularity) | read ([Wiz](https://www.wiz.io/blog/s1ngularity-supply-chain-attack)) |

**Rust:** there is no Nx-maintained Rust plugin (`@nx/rust` does not exist on npm). The community plugin
`@monodon/rust` 3.0.0 (MIT, 2026-06-01) depends on `@nx/devkit ^22`, two majors behind Nx 24. Its repository
(`cammisuli/monodon`, 55 stars, 24 open issues) had one release in the last 14 months. Its graph plugin
(`packages/rust/src/graph.ts`) runs one `cargo metadata --format-version=1` in the process's working folder, the
workspace root (`utils/cargo.ts`, `execSync` without `cwd`). On our tree that command fails (verified above), so the
plugin would find **no Rust projects**. Using it would need either the single root workspace that ADR 0014 rejected
(feature unification across Debrief's DuckDB, `windows` and Tauri `unstable` features) or our own `createNodesV2`
plugin that does what `cargoGraph()` already does. It has no nextest executor and no sccache integration; caching
`target/` is impractical (tens of GB), so a cache miss compiles as today. (Source verified on GitHub; plugin not
installed.)

**Angular and Playwright:** this is Nx's strength. `@nx/angular` and `@nx/playwright` 24.0.2 are MIT and current
(verified on npm). The `nx init` migration of an Angular CLI workspace **splits `angular.json` into `project.json`
files** (read, [Nx guide](https://nx.dev/docs/kb/migrate-angular-cli-to-nx)). We have 7 Angular workspaces. The
Angular CLI's own build cache is already kept between runs (`node-caches`), and the UI-only or Rust-only split is
already in `kinds`.

## 3. Turborepo today

| Item | Finding | Kind |
|---|---|---|
| Version and licence | `turbo` 2.11.10 (2026-10-20), MIT; minor every 1–3 months, no major since 2.0 (2024-06) | verified (npm) |
| Rust | [Rust guide](https://turborepo.dev/docs/guides/tools/rust): `futureFlags.experimentalCargoWorkspaces`, "experimental and can change at any time", added in 2.10.4 (mid-2026). Needs **a root virtual `Cargo.toml` with `[workspace.metadata] name` and a root `Cargo.lock`**. Built-in tasks map to `cargo test --package=<crate> --locked`. Inputs are the crate's local dependency closure plus the root Cargo files, `Cargo.lock` and `rustc -vV`. Files outside the crate (`include_str!`, `#[path]`, `build.rs` reads) must be declared by hand, or "a stale cached result can be restored". Only final `bin`/`cdylib`/`staticlib` outputs are cached, never `target/`. The target folder must be inside the repo. sccache and nextest are not mentioned | read (guide); the version from [release notes](https://releases.sh/release/rel_GWrBbp7pjX_J6sKCDZmkl) |
| Our layout | Experiment: two separate Cargo workspaces with a path dependency between them, `turbo run test --dry=json` → `"monorepo": false`, **no crate tasks**. With a root virtual workspace over the same crates → per-crate tasks `a#test`, `b#test` with `cargo test --package=… --locked`, and `--filter=...a` selects `b` too | **verified** (turbo 2.11.10, synthetic crates) |
| Remote cache | Vercel Remote Cache, free on all plans (SaaS). Self-hosted: any server that implements the open Remote Cache API (v8), e.g. `ducktors/turborepo-remote-cache` (MIT, 1.5k stars, active) or `brunojppb/turbo-cache-server`. Optional HMAC signing of artifacts (`remoteCache.signature`): it detects tampering, but every CI job holding the key can still write | read ([remote caching](https://turborepo.dev/docs/core-concepts/remote-caching)); activity verified on GitHub |
| Privacy | telemetry is on by default (the first run printed the notice); `TURBO_TELEMETRY_DISABLED=1` turns it off | verified |
| Angular and Playwright | no Angular integration: runs `package.json` scripts (`ng build`, `ng test`, `playwright test`) and caches their declared outputs | read |
| Gitea Actions | a CLI with `TURBO_API`, `TURBO_TOKEN`, `TURBO_TEAM`; `--affected` needs a base ref, so the last-green base stays our code | read |

Conclusion: Turborepo is the lightest of the three for the JS half. For Rust it requires collapsing our 15 Cargo
workspaces into one, which is the change ADR 0014 §1 rejected, and its support is experimental. Turborepo without Rust
caches only `ng test`, lint and Playwright. Those are the jobs that `kinds` already skips for Rust-only changes.

## 4. What a test-result cache would save: replay of real CI runs

**Method (verified; tool and raw data in `/Volumes/d7sst7/dev-targets/research-nx-tools/`).** All 142 workflow runs
the Gitea API still holds for `decryp7/paddock` (2026-10-22 to 2026-10-27: pushes to `main`, `integrate/monorepo`,
`shell/integrate`, dispatches on feature branches) and their 1,342 jobs. Each executed (success or failure) job is
mapped to its project and half: crate legs → `crates`; `ui` and reference-app jobs → `ui`; `debrief.yml` `rust tests`
→ `app:debrief` Rust half; `end-to-end` → UI half; bundles and smoke → both. Plan, release and mark jobs are never
cached. In time order, a job counts as a **cache hit** if an earlier run of the same job (same workflow and name, any
branch) succeeded on a commit from which `affected.mjs`, run on the diff between the two commits, would **not** select
that job's project or half. That is `affected.mjs` used as the input hash: coarse, so it errs towards misses. The graph
is read from today's tree. Two variants:

- **strict:** `affected.mjs` as it is (`.gitea/**` marks everything, so CI configuration is an input);
- **loose:** `.gitea/**` changes ignored, as a task runner's default hash would do.

| | strict | loose |
|---|---|---|
| Jobs executed / runner minutes | 996 / 3,252 | same |
| Cache hits / minutes saved | 296 / **823 (25.3%)** | 369 / 1,033 (31.7%) |
| ...on `main` + integrate branches | 262 of 653 jobs, **748 of 2,045 min (36.6%)** | 834 min (40.8%) |
| ...on feature branches | 34 of 343, 75 of 1,207 min (6.2%) | 199 min (16.5%) |
| Hits on an identical tree (re-run, re-push, integrate → main) | 105 jobs, 286 min | same |
| Hits where the tree differs but the job's inputs do not | 537 min | 746 min |
| Sum of each run's longest job (a wall-time proxy) | 1,087 → 906 min (−17%) | → 826 min (−24%) |
| **Failed jobs a cache would have replayed as green** | **11 of 295** | **27 of 295** |

Top savings (strict): crate legs on Windows 143 min, macOS 104 min, Linux 86 min. The Windows and macOS legs run on host
runners, the scarcest capacity we have. Then the `ui` job 49 min, reference-app shards 74 min, and Debrief e2e shards.

What the numbers say:

- **The saving is real and comes mostly from coarse run-level bases, not from a fine hash.** `affected.mjs` decides per
  *run* from the last green *run*. After one red job, every job re-runs on the next push, even those whose inputs did
  not change since they last passed. A per-*job* last-green base captures exactly the strict column. No hash store is
  needed: Gitea's job records are the cache index.
- **CI configuration must be an input.** The loose variant gains 6 more points, but turns 16 more real failures into
  false greens, because this week's failures were caused by CI changes (runner paths, sccache, signing). That is the
  default behaviour of a task runner that does not hash workflow files.
- **The 11 strict false greens** are 3 identical-tree re-runs of `tools and shared scripts` on `shell/ci-sccache-host`
  (host state), a nightly macOS crate leg on the same tree as a green push (flake or host state), two macOS crate legs,
  `tools` on `shell/x14-chromium` and `shell/integrate`, one `crates` coverage job and one Pitwall Linux Rust job. All
  are flakes or runner state. A cache hides them, which is acceptable only because `nightly.yml` runs everything
  uncached (keep it that way).
- **Caveats:** one atypical week (the monorepo frame was being built, with many re-runs, which inflates the hits); the
  graph is today's, applied to older commits; durations include queue-free job time only. Treat 25–37% as an upper
  band for a normal week. The spike's point still holds: a cache saves nothing on the first run of a changed app.

## 5. What a task runner would not fix

`fix:`/`ci:` commits touching `scripts/`, `conformance/`, `.gitea/` or app scripts since 2026-10-13: 111 of 268
commits. Classified by keywords in the subject (overlapping, rough):

| Theme | Commits | Would Nx/Turborepo help? |
|---|---|---|
| Windows/macOS/Linux specifics (lld, junctions, WebKit, host runners) | 33 | no |
| Signing, bundles, release, `latest.json` | 31 | no |
| Test portability, races, flakes, zombies | 22 | no (a cache hides flakes) |
| sccache, build folders, stable paths | 19 | no (no runner caches `target/`) |
| Browser, Playwright, Chrome for Testing | 11 | no |
| Secrets, tokens, environment | 5 | no |
| Lints (clippy, deny, audit) | 5 | no |
| Wrong affected selection | **0** (`affected.mjs` has 4 commits ever: creation, two app hookups, one new edge with its test) | the only thing it would replace |

## 6. Maintainability: what is bespoke, and what each option deletes

Inventory (verified; non-test `.mjs .js .cjs .sh .ps1 .yml .ts` files, tests counted apart):

| Area | Files | Lines | Test lines |
|---|---|---|---|
| `scripts/` (affected, publish, new-app, demo release, coverage gate, timings) | 13 | 2,626 | 1,357 |
| `conformance/` (checks, `ci/`, `release/`, sidecar, notices, sync) | 63 | 4,111 | 3,460 |
| `.gitea/workflows/` | 24 | 5,906 | 0 |
| `.gitea/actions/` | 22 | 815 | 0 |
| Debrief `scripts/` + `app/scripts/` | 38 | 4,229 | 1,187 |
| Pitwall / Pitcrew / Dyno / Spotter | 18 / 12 / 11 / 9 | 1,066 / 840 / 688 / 685 | 377 / 286 / 328 / 444 |
| Pacenotes / Sighter | 2 / 2 | 134 / 134 | 71 / 71 |
| **Total** | **214** | **about 21,200** | **about 7,600** |

(The coordinator's count, which leaves out `.ts` and some app UI scripts, was about 20,000. The order of magnitude is
the same.) Churn since 2026-10-13: 268 commits touch these paths, 111 of them `fix:`/`ci:`. Workflows 112 commits
(46 fixes), actions 27 (18), conformance 51 (28), app scripts 120 (55).

**Duplicates that the shared kit already replaces (verified by checksum and diff):**

| Copy | Copies × lines | Shared replacement (exists) | Lines removed |
|---|---|---|---|
| `scripts/release/publish.mjs` | 6 identical × 93 | `conformance/release/publish.mjs` | 558 |
| `scripts/release/latest-json.mjs` | 4 × 118 (identical except the app name) | `conformance/release/latest-json.mjs` ("byte-equal to what the apps' own copies wrote") | 472 |
| `scripts/ci/macos-verify-bundle.sh` | 4 × 104 + Debrief 85 | `conformance/release/macos-verify-bundle.sh`, `macos-verify` action | 501 |
| `scripts/commit-lint.mjs` | 4 identical × 59 + Debrief 67 | `conformance/src/commit-lint.mjs`, `licences` action | 303 |
| `scripts/export-diagrams.sh` | 6 identical + Dyno variant, 41 each | `diagrams/scripts/export-diagrams.sh` | 284 |
| `check-icons.mjs` | 3 × 121 | one shared check (icons pipeline) | 242 |
| `check-index.mjs` | 5 (3 identical) | one shared check | about 120 |
| `sidecar.mjs`, `tauri-bundle.sh`, `macos-cleanup.sh` | Pitwall, Pitcrew, Dyno | `paddock-sidecar`, `tauri-bundle`, `host-cleanup` actions | about 150 |
| **Scripts subtotal** | | | **about 2,600** |
| Per-app workflows Pitwall, Pitcrew, Spotter, Dyno (2,196 lines; any two differ in only 226–265 lines once the app name is normalised) | 4 | `app-gates` leg in `ci.yml` (driven by `paddock.ci.json`) + `app-release.yml` + `app-e2e.yml`; each app keeps 60–120 lines of its own checks | about 1,800 |
| Debrief workflow (1,088 lines; benchmarks, plugin SDK and smoke stay its own) | 1 | the same kit for licences, bundle, verify, release | about 500 |
| **Total** | | | **about 4,900** (plus the duplicated tests of the deleted copies) |

**What each option deletes and adds:**

| Option | Deletes | Adds | Stays bespoke regardless |
|---|---|---|---|
| Consolidate | about 4,900 lines of copies | about 200–400 lines (`paddock.ci.json` entries, kit inputs) | release, signing, verification, sidecars, conformance (all in the shared kit, written and tested once) |
| Job-level last green | none | 150–250 lines in `affected.mjs` and tests, per-job outputs in `ci.yml` | as above |
| Nx | `affected.mjs` (561), its tests (341), plan jobs (247): about 1,150 | `nx.json`, about 25 `project.json` files with named inputs (the `frontendDist`, `tsconfig` and DOC_INPUTS edges become `implicitDependencies`/inputs), a Rust `createNodesV2` plugin (about 200–300 lines TS; monodon cannot read 15 workspaces), a Gitea last-green SHA step, a cache server and its access control, `nx migrate` every major | everything in §5 |
| Turborepo | the same about 1,150 | `turbo.json` with per-package inputs, a single root Cargo workspace (feature unification problem) or no Rust at all, a cache server | everything in §5 |

The least bespoke code to maintain comes from **consolidate + job-level last green**: about 4,700 lines fewer, one
script we already understand, and no new configuration language. Nx and Turborepo move about 1,150 lines from our
JavaScript into their configuration. The new configuration is about as long, and agents must keep it correct (a
missing input is a false green).

## 7. Other alternatives (re-checked)

- **Content-hash result cache in `actions/cache`:** key = hash of a project's input files (from `affected.mjs`'s graph)
  plus a marker file. It would work, but the cache server at 172.16.1.188:8089 is writable by every job, PR jobs
  included: the CREEP pattern Nx deprecated its plugins for. The job-level last-green base (option 2) gives the same
  saving from Gitea's run records, which only CI writes, and needs no storage. Prefer option 2.
- **moon:** still 2.6.4 (2026-10-23), the version the spike measured two days ago. The spike's findings stand: no
  cross-workspace Cargo graph, hand-written `dependsOn`/`inputs`, `bazel-remote` as a cache. Remains the fallback.
- **Bazel / Buck2:** `rules_rust` with `crate_universe` and rules for JS exist, but Tauri bundling, the Angular CLI
  builders and our signing would all be rewritten as rules. Weeks of work, a large new language for agents. No (read,
  [rules_rust](https://github.com/bazelbuild/rules_rust); not tried).
- **cargo-only tools:** nextest [partitioning](https://nexte.st/docs/ci-features/partitioning/) (`--partition
  hash:m/n`) and [archives](https://nexte.st/docs/ci-features/archiving/) (build once, run on several runners) are
  worth using for the long Rust test jobs (Debrief) and fit with the scripts. `cargo-hakari` is already used.
  `affected.mjs`'s `packages` output already feeds nextest `package(=…)` filters on PRs.

## 8. Migration cost and risks

| | Consolidate | Job-level last green | Nx | Turborepo |
|---|---|---|---|---|
| Agent-days | 4–6 (one app per day, Debrief 1.5) | 1.5–2 | 10–15 | 6–10 |
| Lock-in | none | none | high (executors, `project.json`, cache format, migrations) | medium (`turbo.json`, a single Cargo workspace) |
| Debugging | as today, fewer copies | one more reason in `--explain` ("unchanged since job X passed in run N") | a daemon, a project graph, an inferred-target layer and a cache between us and `cargo`/`ng` | a cache layer and an experimental cargo mode |
| Main risks | a missed per-app difference during the switch (mitigate: one app per PR, a full app run plus `workflow_dispatch` of its release before deleting its copies) | a flake or host-state failure skipped on the next push (11 of 295 in the replay); the nightly runs everything | false greens from incomplete inputs; plugin two majors behind; supply chain | experimental Rust; feature unification across apps; telemetry on by default |

## 9. Work packages (if 1 and 2 are approved)

1. **WP-C1 Pitcrew onto the kit** (implementer, 0.5–1 day): `paddock.ci.json` checks, drop `pitcrew.yml`'s gates in
   favour of the `app` leg, release via `app-release.yml`, delete `scripts/release/*`, `macos-verify-bundle.sh`,
   `commit-lint.mjs`, `export-diagrams.sh`, `check-index.mjs`. Verify: one full Pitcrew run plus a release dry run
   (`workflow_dispatch`) before the deletion merges.
2. **WP-C2–C4 Pitwall, Spotter, Dyno** the same way (0.5–1 day each; Dyno's `check-bundle.sh` and
   `check-capabilities.mjs` stay app checks).
3. **WP-C5 Debrief** (1.5 days): licences, bundle, verify and release through the kit; benchmarks, plugin SDK, real-app
   smoke and corpus stay in `debrief.yml`.
4. **WP-C6 shared checks** (0.5 day): one `check-icons` and one `check-index` in `conformance/`, with a conformance rule
   that fails when an app re-adds a copy of a kit script (rule 40: a check over every instance).
5. **WP-A1 job-level last green** (researcher designs, implementer builds; 1.5–2 days): `ciRuns` also reads each run's
   jobs. For every job key, base = newest **push** run on `main`/`integrate/*` (never PR or dispatch runs) where that
   job succeeded and which HEAD contains. `affected.mjs` outputs one boolean per job (`crates_linux`, `crates_macos`,
   ...), and `.gitea/**` stays an input. `--explain` names the run it skips against. `nightly.yml`, tags and releases
   ignore it. Tests: the replay's cases as fixtures (an identical tree, a red run where only one job failed, a CI-only
   change).
6. **Measure after two weeks:** re-run the replay tool on the new runs. If the job-level hits stay below 10%, or a
   finer per-crate cache shows more than 20% on top, reopen the moon fallback with that number.

## 10. Decisions for the user

1. **Nx and Turborepo:** not adopted (recommended). This confirms ADR 0014 §7. Add a line to the ADR pointing here.
2. **Consolidate first:** approve WP-C1 to C6 (about 4,900 lines of copies deleted, 4–6 agent-days), Pitcrew first.
3. **Job-level last green (WP-A1):** approve, accepting that a flake on a job whose inputs did not change is not
   re-run until the nightly. Only push runs on `main`/integrate count as green evidence.
4. **Nightly stays uncached and full** (recommended; it is the safety net for 2 and for flakes a cache would hide).

## Sources (accessed 2026-10-27)

- npm registry metadata: [nx](https://registry.npmjs.org/nx), [@monodon/rust](https://registry.npmjs.org/@monodon/rust),
  [@nx/s3-cache](https://registry.npmjs.org/@nx/s3-cache), [turbo](https://registry.npmjs.org/turbo),
  [@moonrepo/cli](https://registry.npmjs.org/@moonrepo/cli), [nx-remotecache-custom](https://registry.npmjs.org/nx-remotecache-custom)
- Nx: [self-hosted caching spec](https://nx.dev/docs/guides/tasks--caching/self-hosted-caching),
  [deprecated self-hosted cache packages](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages),
  [CVE-2025-36852 CREEP](https://nx.dev/blog/cve-2025-36852-critical-cache-poisoning-vulnerability-creep),
  [migrate Angular CLI to Nx](https://nx.dev/docs/kb/migrate-angular-cli-to-nx), [nx-set-shas](https://github.com/nrwl/nx-set-shas),
  [Wiz: s1ngularity](https://www.wiz.io/blog/s1ngularity-supply-chain-attack)
- monodon: [repository](https://github.com/cammisuli/monodon), `packages/rust/src/graph.ts` and `src/utils/cargo.ts` on `main`
- Turborepo: [Rust guide](https://turborepo.dev/docs/guides/tools/rust), [remote caching](https://turborepo.dev/docs/core-concepts/remote-caching),
  [v2.10.4 release notes](https://releases.sh/release/rel_GWrBbp7pjX_J6sKCDZmkl),
  [ducktors/turborepo-remote-cache](https://github.com/ducktors/turborepo-remote-cache), [brunojppb/turbo-cache-server](https://github.com/brunojppb/turbo-cache-server)
- Community Nx cache server: [enxtur/nx-caching-server](https://hub.docker.com/r/enxtur/nx-caching-server)
- moon: [remote cache](https://moonrepo.dev/docs/guides/remote-cache); Bazel: [rules_rust](https://github.com/bazelbuild/rules_rust)
- nextest: [partitioning](https://nexte.st/docs/ci-features/partitioning/), [archiving](https://nexte.st/docs/ci-features/archiving/)
- Paddock: [ADR 0014](../adr/0014-monorepo-under-paddock.md), [monorepo spike](2026-10-monorepo-spike.md),
  [CI speed](2026-10-ci-speed.md), `.gitea/actions/README.md`
