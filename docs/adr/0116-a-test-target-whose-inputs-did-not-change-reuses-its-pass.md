# 0116. A test target whose inputs did not change reuses its pass

- Status: **Accepted** 2026-11-25 by the user: all decisions D1 to D9 as recommended (target-level reuse on the graph model, clippy, fmt
  and e2e legs in phase 2, coverage never cached in phase 1, the runner's OS build reported and not keyed, a separate bucket, the
  48-hour limit while the nightly is paused, the three additions beyond common practice, full runs may reuse exact-key passes).
  Build started 2026-11-26 on `feat/adr-0116-test-result-cache`.
- Date: 2026-11-23 (accepted 2026-11-25)
- Related: ER-8 (full suite once on the combined tree), ER-12 (FLAKY never turns green), ER-15 and ER-49 (one verdict reader; test a
  commit once), ER-20 (cheaper operation), ER-33 (positive controls; not measured is never a pass), ER-37 (common practice and cost),
  ER-41 (report-only first), ER-46 (lists only shrink), ER-51 (every shared cache has an owner, a size limit and an eviction rule);
  rule 50 (CI runs only what a change needs, once), rule 55 (one implementation per mechanism); ADR 0042 §5 (CI once per tree; local and
  CI reuse share one key), ADR 0089 (typed outcomes), ADR 0111 (keyed caches), ADR 0113 (a run declares its scope), ADR 0115 (cache
  steps report their own outputs); docs/research/2026-11-19-sccache.md §8 (the RustFS store, its key policy and poisoning walls).
- Number: 0116. On 2026-11-23 the ADR files under `docs/adr/` of every `origin` ref were listed with `git ls-tree` (489 refs, the
  `feat/*`, `fix/*`, `docs/*` and `train/*` refs among them, 348 of those four kinds): the highest number was 0115 (on
  `origin/train/2026-11-22-20`, `feat/schedule-reuse`, `feat/trial-push-profile` and `feat/worktree-base-open-train`); 0113 is on
  `fix/run-scope-reuse-p2`, `feat/os-scoped-tests` and `feat/schedule-reuse`, 0114 on one branch. No file or text on those refs
  names ADR 0116 to 0119 (`git grep 'ADR 011[6-9]|adr/011[6-9]'` over `docs/`).
- Evidence heads (ADR 0091), read 2026-11-23 and re-read before the commit: `origin/train/2026-11-22-20` = `803aed12d` (this
  branch's base), `origin/main` = `b3c254a2d`, `origin/fix/run-scope-reuse-p2` = `bab7a4a97` (ADR 0113, `run_scope.rs`),
  `origin/feat/check-result-reuse` = `f67b7f6a1` (`pre_push/stamp.rs`, `pre_push/reuse.rs`), `origin/feat/schedule-reuse` =
  `c10bccb57` (ADR 0115 text). `feat/infra-only-smoke`, named in the brief, had **no ref on origin** when read.
- Shipped code: nothing here is released. `git tag --contains 803aed12d` is empty; every mechanism cited below is on `origin/main`,
  on the open train, or on a named branch, as each citation says.

## Context: why a monorepo builds everything, and what a split repository gets for free

When an app lives in a repository of its own, a change to another app never starts its CI: the other repository has no commit.
In one repository, every push starts one run, and that run decides what to test. Paddock's planner (`./dev plan`) narrows by the
Cargo and npm graphs, but any change that reaches a shared input (a shared crate, `@decryp7/paddock-e2e`, a conformance check that
every app leg runs, an action every job uses) reaches every app, and every reached app reruns all its tests, even the ones whose
own inputs are byte-for-byte the same as in the last green run.

Monorepo build systems solve exactly this with a **task or test-result cache**: Bazel, Nx and Turborepo hash each test target's
real inputs, and a target whose hash is unchanged reuses its earlier pass instead of running. That gives each app the behaviour of
a split repository: a change elsewhere costs it nothing, because nothing it reads has changed. The planner decides which *jobs*
a change might reach; a result cache decides, per target, whether there is anything new to test. The two are complementary; the
planner's precision is audited separately and not designed here.

### Measured: the cost of rerunning unchanged tests

Synthetic-free, read from the Gitea API (`/actions/runs/{id}/jobs` and each job's log, read with the token in the environment,
never printed) on 2026-11-23. Step times come from the `::group::Run` boundaries in each job log; job totals from the first and last
log timestamps (they agree with the API's `started_at`/`completed_at` within a minute).

| Run | Commit | Event | Result | Jobs | Job-minutes |
|---|---|---|---|---|---|
| 8635 | `0ae8b4794` (train 20) | dispatch | failure (one FLAKY, below) | 48 (1 skipped) | 296.5 |
| 8654 | `803aed12d` (train 20) | dispatch | **cancelled**; two jobs failed before the cancel | 48 (1 skipped) | 327.3 |

Run 8654 is the round after 8635 on the same train (ER-33: it is reported as cancelled, never as a failure). Between the two
commits, 59 files changed: `tools/paddock-dev` (16), `apps/debrief` (10; not a ci.yml leg), `ui/e2e` (7), `ui/projects/paddock-e2e`
(6), conformance (C156 and `runner-slots.json`), five workflows, one action, docs. Nothing under `crates/`, no app's Rust, no app's
Angular source outside Debrief, nothing under `eslint/`, `icons/`, `tools/wiki`, `tools/heavy` or the VM tooling scripts changed.

Jobs of 8654 whose every test target had the same inputs as in 8635 (by the input closure of the design below):

| Job | 8654 min | 8635 min |
|---|---|---|
| `crate` macOS, Linux, Windows | 2.4 + 5.6 + 26.1 | 3.0 + 2.6 + 11.4 (8635 Windows had no `./dev` build) |
| app legs on macOS and Windows: dyno, pitcrew, pitwall, spotter (Rust gates only) | 1.5 + 10.3, 1.3 + 13.2, 0.8 + 3.1, 4.7 + 5.0 | as listed in the API (1.8 + 14.7, 7.7 + 3.9, 0.6 + 2.2, 1.3 + 10.7) |
| eslint preset, icons, wiki tool, VM tooling tests | 5.6, 6.0, 2.4, 3.6 | 4.6, 4.0, 2.6, 5.0 |
| **Whole jobs that need not have run** | **91.4 of 327.3 (28%)** | the same jobs cost **73.8** in 8635 |
| of which on the Windows runner | 57.6 of 81.2 (71%) | 42.9 of 61.2 |
| of which on the Mac runner | 14.2 of 38.3 | 19.3 of 46.4 |
| Steps inside jobs that still had to run (the Linux app legs' `rustChecks`, Rust gates and vitest for the four apps, whose only changed input was the app conformance step; heavy tool's clippy and nextest, whose `cargo deny` reads the network) | **19.6** | 19.6 |

**Waste per round: about 75 to 110 job-minutes, 28 to 34% of a ci.yml train run, and about 70% of the Windows runner's minutes**,
with no test result that could differ. Every other job of 8654 had a changed input (`paddock-e2e` reaches every e2e leg and the
reference apps; conformance, paddock-dev and the tools tests changed) or is never cached (plan, commits, audit, gitleaks, the C102
register, C99 reads Debrief's source).

The same set holds against main. `./dev plan b3c254a2d 803aed12d --explain` (run in this worktree) marks **everything**
(`"all": true`); its only global reason is `global .gitea/actions/README.md` (an action-folder file with no user is treated as
untraceable, `plan_gitea.rs` lines 325-328 and `plan.rs` line 538 on the train). Between main and the train head, `crates/`, every
app's Rust and `eslint/`, `icons/`, `tools/wiki` are unchanged (only app `*.spec.ts` files, `ui/projects/paddock-ui/testing` and
`tools/heavy` changed outside Debrief, paddock-dev, conformance and `.gitea`), so the same ~91 job-minutes of whole jobs reuse.

What the cache does **not** buy in these two rounds: wall time. The critical path is `app dyno (ubuntu-latest, e2e-webkit)` (31 to
40 min), and `paddock-e2e` changed, so it reruns. The saving is runner capacity, chiefly the single Windows machine; the effect on
wall time in rounds where e2e inputs are unchanged (a tooling-only train) is **not measured**.

Two things the logs show that the design must respect:

- **A FLAKY on unchanged inputs.** In 8635, `pitcrew-service::supervision keep_longer_postpones_the_stop_and_cleanup_can_be_turned_off`
  failed on try 1 and passed on try 2 on macOS (job 45054, `TRY 2 PASS`, `flaky-result = "fail"` made the job red); in 8654 the same
  inputs passed. A FLAKY result must never become a cached pass (ER-12).
- **A network check on unchanged inputs.** In 8654 the heavy tool job's 157 tests passed and the job failed in `cargo deny`: "failed to
  fetch advisory database" (job 45326). A check that reads the network or the advisory database is never cached.

Also seen, not this ADR's subject: 41 job-minutes of 8654 were `./dev` building itself from source (`dev: built paddock-dev ... in N s`;
6.4 min in `app pitcrew (windows-latest)`, not published for `x86_64-pc-windows-msvc`); ADR 0115 owns that.

## Common practice (ER-37)

| Tool | Unit | What the key hashes | Failures and flakes | Who writes | Source |
|---|---|---|---|---|---|
| **Bazel** test caching and remote cache | a test target | the test action's inputs: the test binary and its runfiles (so the declared dependency closure), the command line and the test environment | `--cache_test_results=auto` reruns when "Bazel detects changes in the test or its dependencies", the test is `external`, `--runs_per_test` is set, or "the test failed"; tags `external` (always run), `no-cache`, `no-remote-cache`; a pass after retries is reported `FLAKY` | "You may want only your CI system to be able to write to the remote cache"; readers use `--remote_upload_local_results=false`; "Create a clean cache after a cache was poisoned" | <https://bazel.build/docs/user-manual>, <https://bazel.build/reference/be/common-definitions>, <https://bazel.build/remote/caching> |
| **Nx** task cache | a project's task (`test`, `lint`) | the project's source files and its dependencies' sources, configuration, external dependency versions, CLI flags; environment variables and runtime values through `namedInputs`; shared files only when listed (`sharedGlobals`, `globalDependencies`) | (read, not verified on the current page: Nx replays only successful tasks) | CI writes the remote cache; local machines read | <https://nx.dev/docs/concepts/how-caching-works>, <https://nx.dev/reference/inputs> (search summary; the current page returned 404 to the fetch) |
| **Turborepo** task cache | a package's task | per task: the package's source-controlled files (or its `inputs`), its `package.json`, "Lockfile changes that affect the package", its `turbo.json`; global hash: `globalDependencies`, `globalEnv`, flags that affect the run, "the root package's external dependencies from the lockfile" | the page does not say (not verified) | (not covered on the page read) | <https://turborepo.dev/docs/crafting-your-repository/caching> |
| **Gradle** build cache | a task; `Test` is cacheable | "the task type and its classpath", output property names, the Gradle distribution, `buildSrc` and plugin classpaths, the build script where it affects the task | | CI "populates it from clean builds"; developers "should load cache entries from the remote build cache" | <https://docs.gradle.org/current/userguide/build_cache.html> |
| **go test** | a package | "the same test binary", flags from a fixed set, and the files and environment variables the test read | "caches successful package test results" only; `-count=1` disables | local only | <https://pkg.go.dev/cmd/go#hdr-Test_packages> |
| **cargo / cargo-nextest** | none | no test-result cache. cargo issue #10673 "cargo test: Don't re-run tests that already passed" is open; a Cargo team member: "It might be abit risky to cache and skip previously passed tests as cargo cannot 100% sure about what has been changed". nextest offers archived builds, not cached results (search, not verified in nextest's own docs) | | | <https://github.com/rust-lang/cargo/issues/10673>, <https://nexte.st/docs/ci-features/archiving/> |
| **sccache** | one compiler call | the compiler, its arguments, the preprocessed input, selected environment variables | | Paddock: any CI job with the key | <https://github.com/mozilla/sccache>; docs/research/2026-11-19-sccache.md |

sccache makes the compile of an unchanged test cheaper; it never skips running the test, the setup, or the link. A result cache
skips all three. Every tool above that caches tests stores **passes only**, keys on the **declared input closure** rather than on
"what changed in the diff", lets a target opt out, and lets **only CI write** the shared store. Shared files enter a target's key
only when the target declares them: Nx's `namedInputs` and `sharedGlobals`/`globalDependencies`, Turborepo's `globalDependencies`
and `globalEnv`, Bazel's explicit `deps` and `data`. CI configuration that does not change what a test does (how the runner installs
a tool, where it caches) is in no tool's key. This ADR is that design. What it adds beyond the common practice is listed with its
cost in §Cost.

## Decision

### 1. Targets

A target is the smallest unit Paddock's CI can already select and report on, so no runner changes its granularity:

| Kind | Target | Selection when partly cached |
|---|---|---|
| Rust | a Cargo package's tests (unit, integration, doc tests) × runner class (`Linux-X64`, `macOS-ARM64`, `Windows-X64`) | nextest `-E 'not (package(=a) \| ...)'` and `cargo test --doc --exclude a ...`, the same filterset form as `plan_ci::package_filter` (on main) |
| Rust check | `clippy` of a package × runner class, and `fmt` of a workspace (phase 2, decision D2) | `--exclude` |
| vitest | an npm workspace project's unit tests (`./dev ci run <app> ui`, `ui specs`) | the project's command is skipped |
| Playwright | an app's e2e leg (app × engine × shard count), the reference-app shards (phase 2, decision D3) | the leg is skipped |

The repository has 161 `Cargo.toml` files (64 under `crates/`), so a ci.yml run has about 300 to 500 Rust targets across three
runner classes, plus about 20 npm projects and about 30 e2e legs.

A job whose every target is reused is switched off by the plan job before it takes a runner (the setup, the `./dev` build, the
compile and the tests are all skipped). A job with some reused targets runs the rest, with the filter above.

### 2. The input key: the graph's per-node input hash

**Dependency.** The key is the per-node input hash of the repository graph model that the repo-graph ADR defines (branch
`docs/adr-repo-graph-model`, being written by the designer; not on origin when this was read, number not yet assigned). That ADR
makes `crates/paddock-repo-graph` (on main today: `Graph`, `cargo_graph` over `cargo metadata`, `npm_graph` with the Tauri
`frontendDist` and the `tsconfig`/`angular.json`/ESLint path edges, `lock_diff`, `History`; used by `./dev plan`, the push gate and
`gates`) the one model, with nodes for projects, CI workflows, jobs, actions and scripts and **test targets**, declared inputs per
node in place of the planner's `GLOBAL` regexes, per-OS reach, and a per-node input hash. This ADR designs no input set of its own
(ER-22); it consumes that hash. The detail of what a node declares, and how it is hashed, is owned there.

What this cache **requires** of that hash, which the graph ADR's tests must show (each is a positive control in §8):

- it is computable **from the tree before anything is built** (Nx and Turborepo hash sources; Bazel hashes the built binary), so the
  plan job can switch a whole job off;
- it covers the test target's source closure through declared edges, the **resolved subset** of each lockfile the target uses (a
  `url` bump such as `7c0a483d5` reaches only the targets that link `url`: Turborepo's "lockfile changes that affect the package"),
  and the **declared test environment as values**: the toolchain pin's compiler lines, the pinned nextest, Node and test browser,
  the CI image digest, the hashed build variables of `input_hash::is_hashed_var`, feature flags, and the test step's own command and
  `env:`;
- it is **per OS** (runner class) for targets that behave differently by OS;
- shared files and CI configuration enter a target's hash **only through a declared edge** (Nx `namedInputs` and
  `globalDependencies`, Bazel's explicit `deps`): the shared actions, other workflow text, docs, conformance and other apps are not in
  an app's test hash unless the app's node declares them, so a change to `.gitea/actions/README.md` or `setup-rust` costs no app
  anything, where today it marks everything;
- it is computed through `pre_push::stamp::digest` (on `feat/check-result-reuse`; ADR 0042 §5 and rule 55: local reuse and CI reuse
  share one key function).

The cache adds three fields of its own around that hash: the **schema** (in code), a **cache epoch** in `docs/test-cache.toml`
(bumping it invalidates every record: the "clean cache" of Bazel's docs, as reviewed data), and the target's **runner class**. The
residual risk (a CI change that alters a test through something not declared) is caught by the nightly bypass (§6) within a day.

### 3. What is stored, and what never is

A **record** is written for a target only when, in this run, the target ran in full (no `since_run` failed-test filter) and every
test in it passed on the first try. A test that failed, ended `FLAKY`/`FLKY-*`, or passed on a retry (the parser of
`ci_runs/logfind.rs` on main, the one `ci flakes` and `triage` use; nextest's JUnit and Playwright's JSON report as structured
inputs where the job writes them) makes the target's run not a pass, and nothing is written (ER-12, ER-33: "Passed" comes only from
the check, ADR 0089). A record holds: schema, key, target, runner class, run id, job id, commit, test count, duration, the runner's OS
build as observed (reported, not keyed; decision D5).

**Never cached** (built in, plus a list in `docs/test-cache.toml`):

- anything that reads the network, the advisory database or history: `cargo deny`, `audit`, `gitleaks`, the commit lint, the C102
  register, the member audits (the same exclusions as `reuse::class_of`);
- conformance and the repository-wide checks (they read the whole tree and expire waivers by date);
- coverage (a coverage figure of a partly reused run is not the workspace's; decision D4);
- the VM, GUI and service smoke tests and any e2e against a live service or real program;
- tests marked in their project's `paddock.ci.json` `noTestCache` (tests that read the clock or the machine despite ADR 0041 and
  rule 7; Bazel's `no-cache`/`external` tags).

The `noTestCache` list is the opposite of an exception list: an entry makes CI stricter. Adding one needs no approval; **removing**
one is a weakening and needs the user's approval in the change (ER-46 read in its own direction), and a test compares the list
against the merge base for removals.

### 4. Store, owner, size and eviction (ER-51)

- **Where:** the user's RustFS S3 server, the one CI's sccache uses (endpoint from the repository variable
  `PADDOCK_SCCACHE_S3_ENDPOINT`), in a **separate bucket** `paddock-test-results` (decision D6; the sccache bucket is
  `PADDOCK_SCCACHE_S3_BUCKET`, whose key is scoped to `sccache-paddock/paddock-ci/*` and is held by every Rust job, including branch
  jobs: research note §8). Layout: `v<schema>/<runner-class>/<kind>/<target>/<key>.json`, about 1 KB each.
- **Keys:** two new RustFS keys, as Gitea repository secrets. `PADDOCK_TEST_CACHE_READ_*` (read-only on the bucket) goes to the plan
  job only; `PADDOCK_TEST_CACHE_WRITE_*` (write-only on `v*/`) goes to one record step at the end of each test job, which runs only on
  a trusted ref (§7). A conformance check in the shape of C141 (on main) keeps each secret to its one step and out of `$GITHUB_ENV`.
  **Amended 2026-11-26 (user):** CI's key is one separate RustFS key in two Gitea secrets, `PADDOCK_TEST_CACHE_KEY_ID` and
  `PADDOCK_TEST_CACHE_SECRET` (not the read/write pair above), bucket `test-results-paddock`; C141 itself allows them only in steps that run
  `./dev ci test-cache`. Locally the machine's usual credential source (the key file outside the repository, else `AWS_*`) is used.
- **Owner:** `./dev ci test-cache` (paddock-dev), registered as a shared cache with its limit in the cache register of ADR 0111/ER-51.
- **Size:** about 500 records per full run, 0.5 MB; the bucket's quota is set to 2 GiB.
- **Eviction, oldest first:** a RustFS lifecycle rule expires records 30 days after they are written (`rc bucket lifecycle rule add
  <alias>/paddock-test-results --expiry-days 30`, the documented command the sccache note uses; expiry runs in RustFS's background
  scanner). `ci test-cache audit` reports the bucket's size and the oldest record and warns over 1 GiB (report-only first).

### 5. How a run uses records, and what it says

The plan job computes each target's key, asks the store for it (one GET per target, bounded concurrency, a 5 s budget per request
and a 60 s budget for the whole lookup; an unreachable store, a refused key or a slow store turns reuse **off** for the run with one
`::notice::` naming the cause, and everything runs, never the other way round). For every record found it checks provenance (§7).
The plan's outputs per job are the reused targets and their filter; a job with nothing left is switched off.

Each reused target prints one line in the plan job's log and the job summary, and the skipped job's summary names them:

```
test-cache: reused target=rust:crates/paddock-ipc@Windows-X64 key=3f9c1a... from run 8635 job 45047 (commit 0ae8b4794, 812 tests, 2026-11-22)
```

A conformance test fails when a job skips a target (by filter or by being switched off) without such a line.

### 6. Verdicts (ER-15, ER-49) and the nightly bypass

- **A reused target counts as tested only when its key matches exactly** and the record passed the provenance check. ADR 0113's
  declared scope gets a `reused` list beside `ran`; `Declared::complete` treats a target as covered when it ran or was reused, and the
  verdict reader prints "reused from run N" for each one it relied on. A record whose key differs in any field is a miss, never a
  partial match.
- **Never read** (every target runs, nothing is skipped): a tag push, every `*-release.yml` run, `tag.yml`'s gates, rule 54's
  installer builds (ER-15: a release is proven from its own tag), any dispatch with `test_cache=off`.
- **The nightly bypass is the net.** `nightly.yml` runs every target uncached (it reads nothing). It writes fresh records for its
  passes, and for every target that **fails** it deletes the records under that target's current key and reports each deleted record
  with the run it came from: that is a cached pass the world no longer supports (an environment drift, a test reading something not in
  its key), and its key definition gets a fix item. The nightly is paused today (memory: "Scheduled CI paused" until Dyno 1.2.0): while
  it is paused, `ci test-cache` refuses records older than 48 hours, so drift is bounded until the nightly runs again (decision D7).

### 7. Trust: only main and train runs write; branch runs read

- **Writer side.** The record step runs only when the ref is `refs/heads/main` or `refs/heads/train/*` (the step's `if:`), the
  write secret is passed only to that step, and `ci test-cache record` itself refuses any other `GITHUB_REF` and any run whose plan
  declared `rerun` (ADR 0113).
- **Reader side, independent of who wrote.** Before counting a record, the plan job asks the Gitea API (through `ci_runs::client`, the
  one run reader) for the record's run and job: the run's ref must be main or a train, the job must have concluded `success`, and the
  target's key recomputed at the run's head commit must equal the record's key (the plan job fetches that commit and reads the tree
  ids of the target's input paths there, which is cheap). One API call per
  distinct run referenced (a handful per plan), cached for the plan job. A record that fails any check is ignored and reported.
- **Honest limit.** In Gitea Actions any branch can edit a workflow to pass a secret to a step (single-user repository; the same
  accepted limit as the sccache store, research note §8, user 2026-11-20). The reader-side check is what makes a forged record
  harmless: it can only point at a trusted run that really passed the same inputs.

### 8. Positive controls (ER-33)

Each lands with the mechanism it guards:

- **Key:** for each requirement in §2, the graph ADR's tests change that one input and show the node hash changes (a file in the
  package, one in a path dependency, one registry version in the closure, the toolchain pin, a hashed variable, the test step's
  command) and change a file outside the closure (another app, `.gitea/actions/README.md`, a doc) and show it does not; this ADR's own
  test shows the epoch and the runner class change the key.
- **Never-pass-from-a-flake:** fixture logs with a `TRY 2 PASS`, a `FLKY-FL` and a Playwright `1 flaky` each produce no record; deleting
  the flake check makes these tests fail.
- **Trust:** a record pointing at a branch run, a failed job, or a commit whose recomputed key differs is ignored; a record step on a
  branch ref refuses.
- **Bypass:** a tag run, a release run and the nightly read nothing (a test removes the guard and shows a read happens).
- **Fail-open direction:** an unreachable store makes everything run; a test with a black-hole endpoint shows it.
- **In production, shadow mode first (ER-41).** The first phase computes and reports every would-be reuse but still runs every target.
  A target whose key matched a valid record and then **failed** in the same run is a soundness finding (the key misses an input) and
  blocks the switch. The switch turns on in the tool step that records the first two green train runs with zero soundness findings,
  by run id, in `docs/test-cache.toml`; until then nothing is skipped.

## Parallelism and cost

- **Lookup:** about 500 GETs of 1 KB records from the LAN store, 16 at a time: well under 10 s (the store's health check answered in 1
  to 119 ms from the three runner classes, research note §8). The node hashes come from the graph the plan job builds anyway; their
  cost is the graph ADR's to measure. The provenance check is a handful of API calls.
- **Writes:** one PUT per passed target at the end of a job, batched in one tool call per job (ER-20, bulk first).
- **Saving:** 75 to 110 job-minutes per train round on the two runs measured; on the single Windows machine about 70% of its minutes
  in those rounds. Wall time: not measured (the critical path, the Dyno WebKit e2e leg, changed in both rounds).

## Alternatives

- **A. Smoke runs for infra-only changes** (a separate effort, named in the brief as `feat/infra-only-smoke`; no such ref on origin
  when read). Complementary: it narrows what an infra change *starts*; this cache skips unchanged targets in every run, infra or not.
  It does nothing for a train that touches one shared crate and leaves 63 alone.
- **B. Job consolidation** (fewer, larger jobs, less setup). Saves setup minutes, not test minutes, and makes skipping coarser.
- **C. Whole-job reuse only** (a job key = the graph's hash of the job node; skip the job when a trusted green job had the same key).
  About 2.5 agent-days with a hand-built key, 1.5 on the graph model. On 8654 it gets the 91.4 job-minutes of whole jobs but not the 19.6 inside partly changed jobs, and a change to
  one crate of 64 reruns all 64. Offered as the cheaper first step in D1.
- **D. Planner precision only** (audited separately). It shrinks what is marked but cannot know that an unchanged target in a reached
  job has nothing new to test.
- **E. Derive records from run logs** (no store: the plan job reads `test-cache:` lines from recent trusted runs' logs, like `ci
  cache-audit`). No new secret, provenance built in, but it reads megabytes of logs per plan and is bounded by log retention; kept as
  the fallback if the store is not available.

## Work packages (agent-days)

- **WP-0, prerequisites, not costed here:** the repo-graph model ADR (`docs/adr-repo-graph-model`) accepted and its per-node input
  hash with test-target nodes built; `pre_push/stamp.rs` (`digest`) landed from `feat/check-result-reuse`; ADR 0113's `run_scope.rs`
  landed from `fix/run-scope-reuse-p2`. None is on `origin/main` or the open train when read.
- **WP-1, targets on the graph (0.5 d):** `test_cache::Target` over the graph model's test-target nodes, the epoch and the
  `noTestCache` defaults in `docs/test-cache.toml`, the key wrapper (schema, epoch, runner class around the node hash); lookup time
  measured and recorded in `docs/benchmarks.md` (rule 48). The node hash itself is the graph ADR's work and cost.
- **WP-2, results (0.75 d):** per-target pass from nextest JUnit (binary id to package), vitest's JSON reporter, Playwright's JSON
  report, sharing `logfind.rs`'s flake rules; flake fixtures.
- **WP-3, store and trust (1 d):** the S3 client (SigV4 over the existing HTTP stack, or a small signing crate checked against
  `deny.toml`), `ci test-cache lookup|record|audit`, the writer guard, the reader's provenance check through `ci_runs::client`, the
  fail-open budget; the C141-style secret check (report-only, ER-41).
- **WP-4, CI wiring (1.25 d):** the plan job's lookup and per-job outputs, the filters in the `crate`, `paddock-dev`, app and `crates`
  jobs, switching off fully reused jobs, the per-target lines and summary, the record step; the conformance test that a skipped target
  always has its line; the planner golden updated.
- **WP-5, verdicts (0.5 d):** `reused` in ADR 0113's scope, `Declared::complete` and `ci verdict` printing "reused from run N"; the
  never-read list for tags, releases and the nightly.
- **WP-6, nightly net and invalidation (0.5 d):** the nightly writes and deletes on failure; the 48-hour limit while it is paused.
- **WP-7, shadow mode and switch (0.5 d plus two train runs):** report-only phase, soundness findings, the recording step that turns it
  on (ER-41).
- **Phase 2 (each its own decision, not in the total):** clippy and fmt targets (0.5 d, D2); e2e legs and reference-app shards (1 to
  1.5 d, D3: needs the built-UI closure and the browser pin in the key).

**Total, phase 1: 5.0 agent-days** on top of the graph model (the brief's 4 to 6 estimate; the input hash it included moved to the
graph ADR), Rust and vitest targets on ci.yml. The app workflows
(`debrief.yml`, the release workflows' gates) are not wired in phase 1; Debrief's gates would follow with WP-4's pattern (about 0.5 d).

## Cost against common practice (ER-37)

The common-practice design (Bazel and Gradle: CI-only writes enforced by who holds the write key, passes only, opt-out tags, a source
key over declared inputs) is WP-1, WP-2, WP-4, WP-5 and the store half of WP-3: about **3.5 agent-days**. This ADR adds:

| Addition | Why this project | Cost |
|---|---|---|
| Reader-side provenance check | any branch can pass a secret to a step in Gitea; the check makes a forged record harmless | 0.5 d |
| Nightly invalidation and the 48-hour limit while it is paused | the nightly is the only uncached net, and it is paused now | 0.5 d |
| Shadow mode before the switch | ER-41; a key that misses an input would hide failures silently | 0.5 d |

That is 1.5 agent-days beyond common practice, more than the 1 day ER-37 allows without the user's yes: decision D1 and D8.

## Decisions for the user

Each lists the recommended option and the common-practice option.

- **D1. Granularity and phasing.** Recommended: target-level (per Cargo package × OS, per npm project) as designed, 5.0 d on the
  graph model. Cheaper: whole-job reuse (alternative C), about 2.5 d without the graph model (1.5 d with it), about 80% of the measured saving (91.4 of 111 job-minutes in 8654).
  Common practice (Bazel, Nx, Turborepo) is target- or task-level.
- **D2. Clippy and fmt as targets.** Recommended: phase 2, after the test cache has its first switched-on week (0.5 d). Common
  practice: Bazel and Nx cache lint tasks too.
- **D3. E2E legs.** Recommended: phase 2 (1 to 1.5 d), after phase 1's shadow mode: they are the most minutes (Dyno's three e2e legs
  were 59 job-minutes of 8654) and the most timing-sensitive. Common practice: Nx and Turborepo cache e2e tasks; Bazel caches any test
  not tagged `external`.
- **D4. Coverage.** Recommended: never cached in phase 1 (the `crates` job keeps measuring whole). Common practice: coverage runs
  uncached or on a schedule.
  **Amended 2026-11-26 (user):** coverage leaves the trains. The Linux `crate` leg runs plain nextest on a train, a branch and a push to
  main (cacheable like every other leg); `cargo llvm-cov` and `scripts/coverage-gate.mjs` (still report-only) run in the nightly, a
  release's gates and a tag only. `plan_ci::coverage_run` and the `crates` job's step condition say the same three things, and
  `conformance/test/coverage-placement.test.mjs` keeps them equal.
- **D5. The runner's OS build in the key.** Recommended: not in the key, reported in each record, the nightly as the net (ER-26 keeps
  runners on the stable channel). Common practice: Bazel keys on the execution platform's declared properties, not the host's patch
  level.
- **D6. Store.** Recommended: a separate bucket `paddock-test-results` with a read key for the plan job and a write key for trusted
  record steps (the user creates the bucket, the two keys, the four repository secrets and the 30-day lifecycle rule, and sets the
  2 GiB quota). Cheaper: a prefix in the sccache bucket with the existing key (no new secrets; branch jobs could write, the reader
  check still guards). Common practice: a store only CI can write (Bazel, Gradle).
- **D7. While the nightly is paused.** Recommended: refuse records older than 48 hours until the nightly runs again. Common practice:
  no age limit; the nightly or a periodic clean cache is the net.
- **D8. The additions beyond common practice (1.5 d).** Recommended: all three (provenance check, nightly invalidation, shadow mode).
  Common practice: none of them, 3.5 d.
- **D9. Full runs on a train's final head and on main.** Recommended: they may reuse exact-key passes (the purpose; ER-8's "full
  suite once" is then "every target ran or has an exact-key pass from a trusted run", and the nightly runs everything). Stricter: full
  runs never read the cache, only member and fix rounds do (saves less: the measured rounds above were full train runs). Common
  practice (Bazel on CI, Nx Cloud) reads the cache on main too.

## Amendment 2026-11-26: declared input sets for the whole-tree rows, measured and not built

The user approved declared input sets for `conformance-tests` and `member-audits` in shadow, after a trace of what each reads. The trace
(docs/benchmarks.md, "what the two whole-tree gate rows read") found 13,675 and 36,087 distinct paths in every top-level area of the tree, the
apps and shared crates included. A sound declared set is therefore the whole tree, which is the key those rows already have (the tree, the
base ref, the day), so a `Class::Declared` would add a second key with no extra reuse. A per-run read set needs a tracer per OS and a key no
one can review (ER-37, common practice: Bazel and ccache do it with sandboxes the build system owns). Not built. What remains open: adding
`member-audits` to the whole-tree class of `reuse::class_of` for an exact-tree repeat (a one-line change in the gate owner's file).

## Consequences

- A change to shared CI configuration, docs or another app no longer reruns an app's unchanged tests; a change to a shared crate reruns
  only the packages that link it, on each OS.
- Every skipped target is named with the run it rests on, in the plan log, the job summary and the verdict.
- A key that misses an input would hide a failure until the nightly. Shadow mode, the epoch, the `noTestCache` list and the nightly's
  invalidation are the four answers; a missed input found by any of them is a fix to the key definition, with a failing test first.
- The planner keeps deciding which jobs a change can reach; this cache only removes work inside that set.

Recurring: none as a defect class. The cost it addresses (unchanged work rerun because a shared input was touched) is the one ADR 0042
§5 (whole-tree reuse), `ci reuse` (ER-49) and `feat/check-result-reuse` (local rows) each address at a coarser grain.
