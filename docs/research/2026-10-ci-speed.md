# CI speed options for the Paddock app family

Date: 2026-10-24. Scope: Debrief's `build.yml` (1,178 lines) as the largest case, the four other app workflows, and
Paddock's own `ci.yml`. Fits plan items X10 (shared CI steps and workflows) and X53 (CI image and cache) in
[shared-next.md](../plans/shared-next.md). Nothing was built or run for this note; savings are **estimates from reading
the workflows**, not measurements. Step 0 below exists to replace them with measurements.

## 1. What the workflows do today (facts read from `build.yml`)

- Linux jobs run in `garage:ubuntu-24.04-20261023-r5` (Rust tools, Tauri libs, Node 24, Playwright browsers). Windows and
  macOS jobs run on the host (Mac mini, Windows VM) with a persistent `CARGO_TARGET_DIR` under `~/.debrief-ci`.
- **No `sccache` and no `RUSTC_WRAPPER` anywhere in the Gitea workflows** (grep of Debrief and Paddock), although
  [dev-setup.md](../dev-setup.md) and rule 41 name sccache as the shared cache.
- `rust-lint` (clippy) restores/saves `target/` keyed on `Cargo.lock`. `rust-test` (Linux) caches **only** the registry
  (`test-registry-*`), because its target folder was 5.6 to 10 GB and took 72 to 163 s to restore or save. So every Linux
  test run compiles the whole workspace (about 45 members, Tauri, DuckDB bindings) from scratch.
- The workspace is compiled separately by `rust-lint` (check profile), `rust-test` (dev), `rust-test --release` (memory
  budget), `tauri-smoke` Linux (debug app), `bundle` Linux (dist) and `plugin-sdk`. Different profiles share few
  artifacts, but their dependency crates (`tauri`, `wry`, `serde`, `duckdb` bindings, ...) are identical per profile.
- Repeated per-job setup: `apt-get update && install` of Tauri libraries in 4 Linux jobs (the image already has them),
  `dtolnay/rust-toolchain`, `taiki-e/install-action` (nextest), `actions/setup-node` with npm cache, libduckdb fetch and
  `actions/cache`, `npm ci`, and in each e2e shard `npx playwright install --with-deps chromium webkit` (image has the
  browsers). Actions without a host resolve to github.com (Gitea docs), an external dependency in every job.
- `e2e`: 2 shards, `workers: 2` in CI, `fullyParallel: true`, 162 files in `app/e2e`; about 10 min of tests per shard
  (memory note ci-speed-later, 2026-10-18). Each shard installs deps and builds/serves the UI itself. It is the long
  pole of the Linux side. `frontend` runs `ng test --coverage` and a full `ng build`.
- No path filtering: a change to docs-only or UI-only code runs every Rust job (except `[skip ci]` commits and the plan
  job's nightly/tag skips). `concurrency: cancel-in-progress` is already on.
- The release needs every job; `tauri-smoke` and `bundle` run 3 OS legs in parallel with the checks.
- Already done (2026-10-18): licences cached (21 to 2 min), save-only-on-lockfile-change, Playwright browsers cached,
  persistent Windows target dir.
- Open from the same note: custom image (done: garage), standalone cache server with `external_secret` (d7-nas got 404
  then 401 against d-gtr9's built-in cache), raise `runner.capacity` (d7-nas ran 1 job, d-gtr9 9).

Biggest structural finding: the dominant cost is almost certainly **cold Rust compilation on the Linux containers**
(no compiler cache, no target cache in the test job), followed by **e2e wall time** and **per-job setup repetition**.

## 2. Findings from the research

Sources are cited by link and date; where a result did not cover a point I say so.

- **sccache** supports local disk, S3/R2, Redis, Memcached, GCS, Azure, GHA, WebDAV and multi-level caches
  ([sccache README, 0.17](https://docs.rs/crate/sccache/0.17.0/source/README.md), accessed 2026-10-24). Depot measured
  it on the Zed workspace ([Depot, 2025-08-01](https://depot.dev/blog/guide-to-faster-rust-builds-in-ci.md)): cold cache
  16.8% slower than no cache, warm cache 11.5% faster; plus nextest 35% faster in total; dependency-only `~/.cargo`
  caching 8% faster; mold about 0.7% slower (negligible); Cranelift failed to build the workspace (inline asm in a
  dependency). The same article warns sccache can slow release builds up to 50%, so use it for test/check builds and not
  for the `dist` bundle. Limits: sccache cannot cache incremental builds, link steps or crates that need the linker
  (bin, cdylib, proc-macro), so linking stays.
- **Why sccache over target caching for us:** the Linux job containers are ephemeral, so only a restore/save through the
  cache server works, and that cost 72 to 163 s each way for a 5.6 to 10 GB target. A bind-mounted local sccache
  directory has no transfer at all. Depot's later comparison ([sccache in GitHub Actions, March 2025](https://depot.dev/blog/sccache-in-github-actions))
  makes the same point (plain Cargo caching needs a persistent disk; sccache was designed for ephemeral runners).
  [Rust-cache vs sccache, 2023 benchmark](https://xuanwo.io/reports/2023-04.md): roughly comparable, rust-cache starts
  from nothing on a miss. `Swatinem/rust-cache` is built around GitHub's cache API; on Gitea it would use the same
  `actions/cache` server we already find slow, so it adds nothing over what `build.yml` does by hand.
- **cargo-chef** targets Docker layer caching of dependencies ([Depot, Rust Dockerfile practices](https://depot.dev/blog/rust-dockerfile-best-practices)).
  It only helps if the layer persists; for a CI image it would pre-build dependencies for one `Cargo.lock`, which goes
  stale on every dependency bump and bloats the image. Not recommended; the bind-mounted sccache gives the same effect
  without image rebuilds.
- **nextest archive and partition** ([nextest archiving docs](https://nexte.st/docs/ci-features/archiving/), accessed
  2026-10-24): `cargo nextest archive --workspace --archive-file x.tar.zst` once, then `cargo nextest run --archive-file
  x.tar.zst --workspace-remap .` on N machines, with partitioning (newer releases add `slice:m/n`, which balances tests
  across binaries, and deprecate `count:m/n`; [release 0.9.127](https://newreleases.io/project/github/nextest-rs/nextest/release/cargo-nextest-0.9.127)).
  The archive does not contain sources and the target needs the same checkout; tests must not depend on build-time
  `CARGO_MANIFEST_DIR` (read it at run time) and must use `NEXTEST_BIN_EXE_*` for binaries. Our tests start child
  processes and a pinned Chrome and use a `child-processes` test group of 1 thread: those group limits apply per
  partition, so the serial group must stay in one partition (put it in its own shard by filterset).
- **Linkers and backends:** mold is not worth it per Depot (above). Wild is on the Rust project's 2026 fast-builds roadmap
  and Cranelift is reported 5 to 12% faster on Zed with line tables ([Rust Fast Builds roadmap 2026](https://rust-lang.github.io/goals/2026/roadmap-fast-builds.html))
  but is nightly-only and breaks on inline-asm dependencies. Our `dev` profile already uses `debug = "line-tables-only"`
  and `opt-level = 2` for dependencies. I did not find a source on split-debuginfo effects; it matters on macOS (already
  default `unpacked`) and little on Linux.
- **cargo-hakari** (workspace-hack): helps `-p` builds avoid recompiling a dependency with different feature sets. Our
  jobs build `--workspace`, so features are unified anyway. Search results did not cover it; judgement only. Low value.
- **Playwright:** official sharding uses a matrix, blob reporter per shard and a `merge-reports` job; set
  `fail-fast: false` ([Playwright sharding](https://playwright.dev/docs/test-sharding), accessed 2026-10-24). We already
  shard 2 ways and upload a report on failure. Blob merge is a reporting nicety, not a speed-up. The speed levers are
  more shards (the Linux side has capacity if runners are raised), one shared UI build, workers per container, and
  trimming WebKit/prod-CSP to main and nightly (already proposed 2026-10-18).
- **Gitea Actions limits:** `upload/download-artifact@v3` only on the Gitea of 2026-10 (moved to the v4 forks 2026-11-01, see 2026-11-ci-artifacts-v4.md) (v4 needs newer Gitea/act_runner support:
  see [Forgejo runner issue #144](https://code.forgejo.org/forgejo/runner/issues/144), which cites Gitea PR #28965; I
  could not confirm the version that shipped it, check ours before relying on v4). Cross-repo `uses:` accepts absolute
  URLs and the `self:` prefix; `$/` references local composite actions ([Gitea Actions comparison](https://docs.gitea.com/usage/actions/comparison),
  accessed 2026-10-24); `builtin:checkout` skips the download but accepts only `repository, ref, token, path,
  fetch-depth`. Not covered by the docs I could fetch: reusable workflows across repos (D7's spike), the cache server's
  `external_server` settings, runner capacity. `act_runner` `config.yaml` exposes `runner.capacity` (default 1),
  `container.valid_volumes` (empty by default, so bind mounts are blocked until listed) and `host.workdir_parent`
  ([config.example.yaml](https://gitea.psi.ch/gitea/act_runner/src/branch/main/internal/pkg/config/config.example.yaml)).
  Gitea has no merge queue; its option is "auto-merge when checks succeed". Our model (green locally, main pushes, CI
  confirms; rules 3 and 4) does not need one.
- **Nx and Turborepo:** Nx covers Rust through plugins with `nx affected` and Nx Cloud remote cache (paid beyond
  self-hosting); Turborepo is JS-task oriented ([Nx on Nx vs Turborepo](https://nx.dev/docs/guides/adopting-nx/nx-vs-turborepo),
  vendor-authored, accessed 2026-10-24). I did not verify cargo handling in either. For one app per repo, the benefits
  they sell (affected graphs across many packages, shared task cache across many projects) are small: Cargo already has
  the package graph, sccache is the remote compiler cache, `ng`/Vitest/Playwright are the JS tasks. Adding a second
  task graph over Cargo and Angular adds a dependency, config in 6 repos and a cache server for little gain.
  A lighter equivalent: a `plan` job that computes changed paths and a nextest filterset (section 4, option 8).

## 3. Ranked options

Saving is wall-clock on the critical path of a typical push to `main` or a PR (the release path always runs
everything). All numbers are my estimates from the workflow structure and the cited benchmarks; "(guess)" means no
source or timing backs the figure. Effort is agent-days for one app plus the shared action. Risk is risk of a wrong or
flaky CI result.

| # | Option | Expected saving (how estimated) | Effort | Risk | Fits |
|---|---|---|---|---|---|
| 0 | **Measure first:** pull per-job and per-step durations of the last ~30 runs from the Gitea API into a table (`paddock/tools/ci-timings.mjs`) | Enables replacing every guess below; cost is small | 0.5 | none | X53, test-speed |
| 1 | **Bind-mounted local sccache + cargo registry on the Linux runner host**, `RUSTC_WRAPPER=sccache` in `setup-rust` action; same on the Mac runner (host env, already installed; share the dev Mac's cache dir) | Linux `rust-test`, `rust-lint`, smoke and plugin-sdk compile cold today (test job has no target cache). Warm cache removes most dependency and unchanged-crate compiles. Estimate 40 to 70% off those jobs' compile phase (Depot: nextest + warm sccache 35% of a whole 28 min run; ours starts from zero reuse, so more) and removes the 72 to 163 s cache restore/save in lint. Roughly 4 to 9 min off the Rust critical path (guess: assumes a cold Linux compile of 8 to 15 min) | 1.5 | low: needs `valid_volumes`, a cap (`SCCACHE_CACHE_SIZE`), concurrency-safe on one disk; check path-dependence (checkout path is stable in containers, worktrees differ on dev Mac) | X10 `setup-rust`, X53 |
| 2 | **Strip repeated per-job setup**: drop `apt-get update/install` (image has the libs; add a one-line `dpkg -s` assert), bake `cargo-nextest`, `cargo-deny`, `sccache`, `tauri-driver`, rust toolchain, pinned Chrome for Testing, libduckdb (version in image tag) into garage; stop using `dtolnay/rust-toolchain` and `taiki-e/install-action` in Linux jobs; skip `playwright install --with-deps` | 20 to 90 s per Linux job x about 8 jobs; mostly parallel, so 1 to 2 min wall, but also removes the github.com download dependency (fewer flakes) (guess for seconds; the `apt-get update` alone is typically 10 to 30 s) | 1 | low; image rebuild per tool bump (tag already carries a date) | X53 |
| 3 | **Build the UI once, share by artifact**: `frontend` job builds `dist`; e2e shards download it (artifact v3) and serve it instead of building/serving their own; `npm ci` once per job via the image-level npm cache | A full Angular production build per e2e shard is typically 1 to 3 min (guess) x 2 shards, and avoids duplicate `ng build` (frontend job builds again). 1 to 3 min on the e2e critical path | 1 | low-medium: dev-server vs prod-bundle behaviour differs; keep the prod-CSP leg on the built bundle | X10, test-speed |
| 4 | **Path-based plan job**: extend `plan` with a changed-paths classifier (`git diff --name-only base..head`) setting `rust`, `ui`, `docs`, `ci` outputs; skip Rust jobs for UI-only pushes and UI/e2e jobs for Rust-only ones; always full on tags, main-after-merge nightly, and when `.gitea/`, `Cargo.lock`, `package-lock.json`, `api.types.ts` change | Depends on commit mix. Our history is many docs, UI-only and engine-only commits; I estimate 30 to 50% of pushes skip half the jobs (guess, count with option 0). Saves runner minutes more than wall time, which matters because runner capacity is the bottleneck on the home server | 1.5 | medium: a wrong filter hides a failure; mitigate by full run on main merges and nightly (CI confirms, rule 3) | X10 `plan` action |
| 5 | **Raise runner capacity and balance** (d7-nas capacity 1 vs d-gtr9 9; standalone cache server with `external_secret`; per-runner memory caps for link-heavy jobs; `tmpfs` for `/tmp` and the target of the test job) | Removes queueing when 4+ apps push, and removes the 404/401 cache misses on d7-nas. Queue time is currently invisible to me (option 0 measures it); tmpfs for temp-heavy tests (guess 5 to 10%) | 1 (user action on the server) | medium: load (rule 41: 1-minute load 6 to 16); more parallel jobs may slow each | X53 |
| 6 | **nextest archive + partition for Linux tests**: `build-tests` job runs `cargo nextest archive` (sccache warm), N shard jobs run `--archive-file --workspace-remap . --partition slice:m/N`; child-process group kept whole in one shard | After option 1 the test run itself (not compile) is the cost. Splits the run phase by N (2 to 3). Archive transfer through artifact v3 for a debug workspace with DuckDB is likely 300 MB to 1 GB zstd (guess), which can cost as much as it saves. Worth it only if the test run phase exceeds about 6 min (check with option 0) | 2 | medium: archive size, relocatable tests, `libduckdb` path in each shard, `xvfb` per shard | X10, test-speed |
| 7 | **Run coverage only where needed**: `ng test --coverage` only on main, nightly and tags; branches and PRs run `ng test` without coverage. Vitest: check `isolate`/`pool` settings (`vitest-base.config.mts`) and `--shard=m/n` if the Angular unit-test builder forwards it (not verified) | Coverage instrumentation typically adds 20 to 40% to the unit-test step (guess); `frontend` job is a lint+build+test chain, so gain is on the longest of its steps | 0.5 | low (coverage gate stays on main; rule 3 allows fewer checks in branches) | test-speed |
| 8 | **Affected-only Rust tests in branches**: script maps changed files to workspace packages with `cargo metadata`, then `cargo nextest run -E 'rdeps(pkg)'`; full workspace on main (the "lighter Nx") | Branch-only; local runs benefit most (rule 3 already says only affected tests). In CI it only helps PRs, and our flow is direct-to-main with CI as confirmation, so low CI gain; keep for agent branches and `workflow_dispatch` | 2 | medium (missing a dependency edge such as build.rs or data files; `core` changes select almost everything anyway) | test-speed |
| 9 | **e2e trimming**: WebKit and prod-CSP specs only on main/nightly/tag; Chromium-only on pull_request; 3 to 4 shards when capacity allows; stable `workers` per container (CPU count, not fixed 2) | If ~10 min per shard today: WebKit-off on PRs roughly halves browser time (guess 40 to 50%); a third shard cuts about 3 min | 1 | low-medium: WebKit-only bugs found later (main, within minutes) | X10, test-speed |
| 10 | **Smaller dev-profile edits**: `[profile.dev] debug = "line-tables-only"` is already set; add `[profile.test.package."*"] opt-level` review (opt-level 2 for deps costs compile time but speeds DuckDB-heavy tests), `CARGO_INCREMENTAL=0` in CI (sccache cannot cache incremental crates) | 5 to 15% of compile time (guess, Cranelift report: 5 to 12% on a similar change) | 0.5 | low | test-speed |
| 11 | **Shared sccache over MinIO/Redis/S3** across more than one Linux host or the dev Mac | Only valuable with two or more Linux runner hosts; one host with a local disk cache is simpler and faster (no network). Revisit when a second Linux runner exists | 2 | medium (secrets, network on a home LAN) | X53 |
| 12 | Linker swap (mold/wild), Cranelift for tests, cargo-hakari, cargo-chef, `Swatinem/rust-cache` | mold about 0 per Depot; Cranelift nightly-only and failed on a similar workspace; hakari irrelevant to `--workspace`; chef redundant with 1; rust-cache duplicates `actions/cache` | n/a | n/a | not recommended |
| 13 | Nx or Turborepo | None meaningful for one app per repo (section 2) | 3 to 5 | medium (new tool, 6 repos) | not recommended |

## 4. Recommended order

1. **Option 0** (half a day): measure. Everything after is reordered if the numbers disagree with the estimates.
2. **Option 1** and **option 2** together, in a single X10 `setup-rust` action plus a garage image bump (X53).
   This is the largest, safest saving: it attacks the cold Linux compile and the per-job downloads, and every app gets it
   by consuming the action. Trial it on Debrief first (one app at a time, rule 1), then Pitwall.
3. **Option 4** (plan job paths) inside the X10 plan action, and **option 7** (coverage on main only): cheap, and they
   cut runner minutes, the real bottleneck on the home server.
4. **Option 3** and **option 9** together (one UI build, e2e trimmed and reshaped): shortens the e2e long pole.
5. **Option 5** (capacity, cache server, tmpfs) as the user's server action, in parallel with step 2.
6. Only if option 0 shows the Rust test run phase (not compile) over about 6 min after steps 2 to 5: **option 6**
   (nextest archive and partition) and **option 10**.
7. Leave options 8, 11, 12 and 13 unless a measured problem points to them.

Expected result of steps 1 to 4 (estimate): a warm push to `main` drops its Linux critical path by about 8 to 15 min,
and docs/UI-only pushes use roughly half the runner time. Confirm with option 0's table before and after each step.

## 5. Open questions and unverified points

- Does the Gitea version here resolve cross-repo composite actions and reusable workflows (D7 spike)? The docs I fetched
  do not say.
- Does our Gitea support `actions/upload-artifact@v4`? Until proven, plan on v3 (sizes limited, no merge-multiple).
- Does the Angular unit-test builder accept `--shard` for Vitest? Not verified.
- sccache and path dependence: container checkouts have a stable path, but verify hit rates (`sccache --show-stats` in the
  job summary) before trusting the saving; remapping paths changes RUSTFLAGS and so Cargo fingerprints (see dev-setup).
- Whether the Mac runner's host environment sets `RUSTC_WRAPPER`: the workflow itself does not; the runner's launchd
  environment would need it.
- Rust 1.90 and later use `lld` by default on x86_64 Linux, which would explain mold's near-zero gain; I did not
  confirm this from a source in this pass.
