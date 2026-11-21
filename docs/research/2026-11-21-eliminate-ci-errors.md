# Eliminating CI errors fast (research, 2026-11-21)

The question: how do we get fewer red runs, and a shorter time from red to green? I read our own run history first, then
how large projects handle it, then ranked what to change. No builds or tests were run for this note.

## Evidence heads (ADR 0091)

Read on 2026-11-21, re-read before the commit:

| Ref | Sha |
| --- | --- |
| origin/main | 3d776b605a943b330d663bfa8b20d3b77396a46b |
| origin/train/2026-11-21-18 | 75ce0d2c8f2dc7684be09c09db28d2bd5d358b91 |
| origin/feat/member-gate-other-os-default | d1fe10843dc6320802518fbdc997fa1ea694f270 |
| origin/feat/ci-d4-and-package-filter | 8966419f1add68efd3af9a3554cf7a61560c4bda |
| origin/feat/train-fix-ladder | 700578097e80f5392130e6e36da3eaf00477a3c0 |
| origin/ci/staged-fast-then-long | 13d02a1ca0e0018ce7bb49ac494f30f6a8615acd |
| origin/feat/ci-health-gate-no-prompts | d7b27387c59d1579ce375908087b90736ff6c565 |

Run data: the Gitea runs API, runs 7938 to 8518 (2026-11-11 18:53 to 2026-11-21 17:50, +08), and the jobs API for every
failed run. Causes: the commits on origin/main and train 18 that cite a run since 2026-11-11 (140 commits), the
`docs/lessons.md` entries from line 1853 on, and the coordinator's queue notes. No user logs were read.

## Part 1: what made our runs red

### The totals (measured from the API)

- 350 runs: 169 green, 153 red, 28 cancelled.
- The red runs held runners for about 347 runner-hours: 219 on trains, 111 on main, 16 on other branches. This is the sum
  of each failed run's job durations, so it counts parallel jobs separately.
- Trains 5 to 18 spent about 160 hours between a red ci.yml or debrief.yml run and the next green one on the same train.
  Trains overlap and both workflows are counted, so this is not wall time. The worst were train 9 (ci.yml 23.3 h), train 12
  (ci.yml 18.2 h, debrief.yml 15 h) and train 14 (13.9 h and 9.2 h).
- **Main went red after almost every landing.** Of the 9 landings whose post-landing ci.yml and debrief.yml runs finished,
  8 had at least one red workflow (7939/7941, 8002/8004, 8015, 8047, 8054/8056, 8134/8136, 8198/8200, 8241/8243, 8315).
  Main's tree was the train head that had just been green.
- **The same commit was red and green.** 34 (commit, workflow) pairs ran more than once. 22 of them were red at least once
  and green at least once, with no code change. Some of the green runs were partial plans (a base dispatch or a
  failed-only round), so 22 is an upper bound on pure flakes. It is still the strongest signal in the data.
- Red train and main runs of the main workflows (109) by the OS of their failed jobs: Linux only 31, Mac only 22, Windows only 8, two or more OS 48.

### The causes, in buckets

I counted distinct causes, not runs: one cause often turned several runs red. The counts are my classification of the
fix commits and lessons entries, so treat them as approximate (plus or minus a few).

| Bucket | Causes | Examples (run, commit or lesson) | Found in | Could have been found in |
| --- | --- | --- | --- | --- |
| Flaky tests: UI end-to-end waits | about 20 | Debrief WebKit toasts 7941/8004, chip 8136, four in 8297, three in 8300, 8301, 8338, two in 8373, 8377, live tail 8243; Pitwall axe 8134 and tray 8315; gallery 8215 | train CI, main | a stress run on the branch (ER-28), a nightly repeat run |
| Flaky tests: Rust timing and harness races under load | about 12 | kept-clone budgets 8066/8090/8099 (train 9, 23 h), 8174 (two), paddock-ipc 8214 and 8425, assistant Stop 8241, queue 8359, macOS watcher 8389 and 8466, dyno-agent disk 8495 | train CI, main | the member gate on the real runner, under load |
| OS-only failures | about 20 | Windows: PATHEXT 8154, git launcher 8161, curated PATH 8138, per-command links 8152, paths 8327 (a720dd85d), winpath sigma 442f9bfca, slot handles bcd97bf9d; train 18: unused `mut` and helper in Windows clippy, three lane-folder path failures, the sccache eviction pair (8495). Linux: zombie holder 8110, ETXTBSY 8174, kernel-released process d68f912aa | train CI | the other-OS member gate before the merge; xwin clippy over test targets before the push |
| Combination failures | about 11 | eight fixes after train 8's merges (run 8050, lessons "Eight combination failures"), nested test runs on train 14 (48fa2449a), the lessons union-merge blank lines (9ec43be3c, b9bf5fc4b) | train CI | the merge-time gates (ADR 0098), which exist |
| Infra | about 9 | runner overlap 7960/7961 and 7969-7971, scheduled runs starving a train 8112-8114, the CI image Dockerfile 8043, EMFILE under Pitcrew's launchd 8367, the shared build folder E0463 8373, sccache evicting the whole mount, Windows oversubscription (8335 against 8359), d-gtr9's runner offline 2026-11-21 | dispatch, train CI | a health gate at dispatch; no writable folder shared between lanes |
| Tool or toolchain drift | about 4 | Rust stable 1.99.1 to 1.100.0 in the middle of runs 8027/8028 (40e1d7736, 4c581a781, 9f411ab6f), toolchain install races (02aa10a14, d6b2ed57e) | train CI | closed by pinning (ER-26) |
| Goldens and fixtures | about 5 | Mac-only goldens with uid 501 (C131), a fake token in a snapshot fixture (train 16 gitleaks), Spotter after midnight UTC 8154 (dfbadcdc7), Pitwall moved-log order 8124, ci.yml line endings (4d7c69bb2) | train CI | at capture (C131, ER-46) |
| Real logic bugs | about 9 | paddock-dialogs proptest 8466, the ipc completion slot 8214, a menu ran its neighbour's command 8116, a required choice dismissed early 8315 (d3abb4265), Dyno timed action lock (25cbb7f15), grid jump (92375f669), Spotter close slowed by leaked temp folders 8346 | train CI, main | a unit or property test on the branch |

What the table says:

1. **Flakes are the largest bucket, about 32 of about 90 causes, and they are what turns main red after a landing.** The
   UI half is one class (a read before the page is ready or still), and the Rust half is mostly fixed time budgets under a
   loaded host. Three "flakes" were real bugs (8214, 8116, 8346), so a flake is worth a look, not only a retry.
2. **OS-only failures are second, about 20, and all of them reached train CI.** Train 12 alone spent about 17 hours in
   Linux- and Windows-only rounds (ER-9, runs 8138 to 8154). Train 18's only blockers were Windows-only.
3. **Almost nothing was found where it could have been found first.** Flakes were found on the train or on main, not by a
   stress run on the branch. OS-only failures were found on the train, not by the member gate.

### Mechanisms we have but do not use

- **The other-OS member gate (068186e2d, on main).** It dispatches the affected plan of one member on the real runners.
  It reported NOT MEASURED for every member of train 18 because `PADDOCK_TRIAL_DIR` was not set
  (`tools/paddock-dev/src/merge_check/other_os.rs`, `Gitea::from_env`), and the integrator merged anyway. The coordinator's
  own note says it would have caught the Windows sccache failure. The unlanded follow-up branch
  feat/member-gate-other-os-default (d1fe10843) "takes an accepted not-measured" in its subject. That keeps the same hole
  open. A fix branch, fix/member-gate-self-dispatch, is named in the queue notes; I did not find it on origin.
- **The flake history (`./dev ci flakes`, cli.rs:370).** It is read-only by design and feeds triage. Nothing reads it to
  decide a landing or a tag. The tag gate (tag.yml lines 16-18, `./dev ci verdict`) needs a fully green run, so one FLAKY
  test in an untouched feature blocks a tag although ER-12 says it does not block a landing. On 2026-11-21 that forced a
  second ci.yml run on 3d776b605 (8466).
- **The nightly runs.** ER-29's shuffled order and the nightly full run would find flakes off the train path. The user
  paused 7 scheduled workflows on 2026-11-20 until Dyno 1.2.0.
- **A quarantine list exists only for Debrief** (`apps/debrief/docs/testing/quarantine.md`, checked by
  `scripts/ci/check-quarantine.mjs` with an expiry date). It is for environment-bound tests, not flakes.
- **Built but not landed:** D4 run reuse and the package filter (feat/ci-d4-and-package-filter), the fix ladder
  (feat/train-fix-ladder: failed-only rounds, then the full run), staged CI with a fast gate per OS
  (ci/staged-fast-then-long), the health gate (feat/ci-health-gate-no-prompts, in train 18). The push-gate fix
  (fix/push-gate-queue-time) is on main.

## Part 2: common practice

All sources read on 2026-11-21. "Secondary" means a summary, not the original.

| Practice | What it buys | What it needs | Fits us? |
| --- | --- | --- | --- |
| Chromium CQ and try bots ([cq.md](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/infra/cq.md)) | A dry run before submit. A failing shard is retried, then rerun without the patch; a test fails the change only if it failed both tries with the patch and passed without it. A green build is reused for up to 1300 commits. Median under 40 minutes. | A large build fleet | The "retry, then compare with the base" rule fits and is cheap: our triage already compares with main |
| Chromium flake tooling ([gtest_flake_tips.md](https://chromium.googlesource.com/chromium/src/+/main/docs/testing/gtest_flake_tips.md)) | Flake clusters with bugs; repeat a test locally or on bots. Exonerating flaky clusters at presubmit is from a search result only (secondary) | LUCI Analysis | The idea fits; the tool does not |
| Google TAP and presubmit ([SWE book ch. 23](https://abseil.io/resources/swe-book/html/ch23.html)) | Presubmit runs only fast, reliable tests of the changed project; post-submit runs the rest. A change that passes presubmit passes the rest 95%+ of the time. Flaky tests may leave presubmit while they are fixed. | Huge scale | The split fits: our member gate is presubmit, the train is post-submit |
| Google flaky tests ([blog, 2016](https://testing.googleblog.com/2016/05/flaky-tests-at-google-and-how-we.html)) | About 1.5% of runs flaky, 16% of tests have some flakiness, 84% of pass-to-fail transitions involve a flaky test. Automatic quarantine off the critical path, with a bug filed | A results database | Yes: our 22 of 34 same-commit flips match the pattern |
| GitHub flaky builds ([blog](https://github.blog/engineering/engineering-principles/reducing-flaky-builds-by-18x/)) | 1 in 11 commits red from flakes, down to under 0.5%. Comparing builds caught 25% of flakes; rerunning three ways (same process, clock moved, other host) caught 90%. Issues filed to recent authors | Test history | Yes, the detection idea |
| Rust bors and rollups ([CI guide](https://rustc-dev-guide.rust-lang.org/tests/ci.html), [rollups](https://forge.rust-lang.org/release/rollups.html)) | Try builds before approval; one merge at a time; rollups of 10-20 low-risk PRs. A failed rollup: retry if spurious, else a person finds the culprit and rebuilds without it | A bot and a database | Our trains are rollups. Culprit-finding by hand is also their practice |
| GitHub merge queue ([docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)) | Queue branches of base plus the PRs ahead; a failing PR is removed and the ones behind rebuilt | Hosted Actions | The removal rule fits (ER-9 already pulls a red member) |
| GitLab merge trains ([docs](https://docs.gitlab.com/ci/pipelines/merge_trains/)) | Up to 20 pipelines in parallel; a failed member is removed, the ones behind restart without it | Premium tier | Same as above |
| Uber SubmitQueue ([summary, secondary](https://blog.acolyer.org/2019/04/18/keeping-master-green-at-scale/)) | Mainline green 52% of the time before. Speculative builds chosen by a predictor | One worker per change, a build graph | No: three runners and a few members per train |
| Bazel remote cache and test caching ([caching](https://bazel.build/remote/caching), [manual](https://bazel.build/docs/user-manual)) | A test is not rerun when its inputs did not change; flaky-tagged tests get 3 attempts | Bazel | The idea fits as D4 (a verdict reused per commit, ER-49); Bazel itself does not |
| Meta predictive test selection ([paper](https://arxiv.org/abs/1810.05286), [blog](https://engineering.fb.com/2018/11/21/developer-tools/predictive-test-selection/)) | Half the test cost, still 99.9% of faulty changes caught | Large history, ML | No: `./dev plan` already selects by the dependency graph |
| Trunk and Buildkite quarantine ([Trunk](https://docs.trunk.io/flaky-tests/quarantining), [Buildkite](https://buildkite.com/docs/test-engine/reduce-flaky-tests)) | Quarantined tests keep running and reporting; a job fails only on a non-quarantined failure. Flaky means different results on the same commit | SaaS, JUnit output | The rule fits; we can do it with our own history |
| nextest retries ([docs](https://nexte.st/docs/features/retries/)) | `flaky-result = "fail"` keeps a retried pass red | nextest 0.9.131 | In use (crates/.config/nextest.toml lines 35-36) |
| Hermetic builds ([Bazel](https://bazel.build/basics/hermeticity), [SLSA](https://slsa.dev/spec/v0.1/requirements)) | Output depends only on declared inputs | Sandboxing | Partly: our infra bucket (shared lane folders, launchd limits) is a hermeticity gap |
| Pre-commit and pre-push hooks ([pre-commit](https://pre-commit.com/), [lefthook](https://lefthook.dev/)) | Fast checks on the dev machine | A hook runner | In use (`./dev check --pre-push`) |

The shared pattern: bounded retries recorded as flaky; quarantine that keeps the test running and files a fix item; a
queue that removes the failing member; and a cheap check before the merge on the same kind of machine as CI.

## Part 3: what to change, ranked

Ranked by red runs prevented or hours saved per agent-day. Costs are raw agent-days. Every item is common practice or
cheaper (ER-37); none needs more than 1 agent-day beyond it.

### 1. The other-OS member gate runs for every member and blocks (0.5 to 1 day)

- Exists: yes, on main (068186e2d). Used: no. Train 18 got NOT MEASURED for every member.
- Missing: `./dev merge check` must set up its own trial folder (a default under the worktree root) and fail, not warn,
  when it cannot measure. Do not land d1fe10843's "accepted not-measured" path without the user's yes.
- Buys: the OS-only bucket, about 20 causes, found before the merge instead of in train CI. Train 12's 17 hours and train
  18's whole blocker set were this bucket. Common practice: Chromium's dry run, Google presubmit, rust-lang try builds.
- Measure: per train, the count of member rows measured against not measured (target: none not measured), and the share
  of red train and main runs whose failed jobs are on Linux or Windows only (now 39 of 109).

### 2. One flake rule in the verdict reader, fed by same-commit flips (1 to 1.5 days)

- Exists: ER-12's per-test rule for landings, nextest flaky-as-failure, `./dev ci flakes` history, `./dev ci verdict`.
- Missing: the verdict reader (and so tag.yml and the landing check) applies ER-12 itself: a FLAKY in a feature the tag's or
  train's diff does not reach does not block, and is written as a fix item with its run and log. A both-tries failure
  still blocks. A test that flips on one commit is listed by `./dev ci flakes` as a fix item automatically.
- Buys: main stops looking red after landings for flakes in untouched code (8 of 9 landings), and no second full run just
  to tag (8466). Common practice: Google and Trunk quarantine, Chromium's "without patch" compare.
- Measure: post-landing runs on main that are red only with FLAKYs in untouched features (now most of 8 of 9), and the
  number of re-dispatches per tag.

### 3. Land what is built for infra and the pre-push gate (no new build; review time only)

- Exists, unlanded: the health gate (train 18), no writable folder shared between lanes (fix/ci-no-shared-lane-folders,
  ADR 0111, train 18), xwin clippy over paddock-dev's test targets (feat/path-guards-and-devtool-xwin).
- Buys: the infra bucket (about 9 causes; E0463 8373, sccache eviction, runner down) and the train 18 Windows clippy miss,
  found at dispatch or before the push.
- Measure: red runs whose failed step is setup, cache or runner (from the jobs API step names), per week.

### 4. A stress run on the branch for any change to a test that has flaked (0.5 day)

- Exists: ER-28 asks for it; the heavy queue exists.
- Missing: a `./dev` step that repeats the touched specs or tests (for example 20 runs at 4 workers for e2e, nextest
  `--retries 0` repeated for Rust) and records runs x workers x engine, run by the member check when the diff touches a
  test listed by `./dev ci flakes`.
- Buys: the flake bucket found on the branch instead of on the train or main. GitHub's data says repeated reruns under
  changed conditions catch about 90% of flakes, against 25% from comparing builds.
- Measure: flake fix items reopened within a week (a fix that did not hold).

### 5. Name the member behind a red train run (1 day)

- Exists: ER-9 pulls a red member; `./dev plan` knows which projects each member's diff reaches; triage lists new failures
  against main.
- Missing: `./dev ci triage` adds, per new failure, the members whose diff reaches its project. Today the coordinator does
  it by hand. Common practice: GitLab and GitHub remove the failing member automatically; rust-lang finds the culprit by
  hand, so this is already more than their baseline.
- Buys: shorter red-to-green. One fix round instead of guessing.
- Measure: fix rounds per red train (shas with a run, minus one; now 1 to 10 per train).

### 6. Land D4, the fix ladder and staged CI (already built; about 1 day of review and fixes in total)

- D4 reuses a green verdict per commit (ER-49) and narrows member runs with the package filter. The fix ladder runs
  failed-only rounds and then one full run (ER-10). Staged CI fails each OS's fast jobs first.
- Buys: hours, not fewer red runs. A red signal in minutes instead of after the long jobs, and no repeat runs of a tested
  commit. Bazel test caching and Chromium's build reuse are the common practice.
- Measure: minutes from dispatch to the first failed job, and runner-hours per train.

### 7. Turn the nightly back on as a flake finder (0.25 day, after the user's pause ends)

- Exists: nightly.yml and ER-29's shuffled order; paused on 2026-11-20.
- Missing: nothing to build. When the pause ends, the nightly writes its FLAKYs into the same fix-item list as item 2.
- Buys: flakes found off the critical path, at night, on an idle Linux host.
- Measure: flakes first seen by the nightly against first seen on a train.

### Not recommended

- Speculative builds (Uber), ML test selection (Meta) and Bazel: they need fleets and history we do not have. Our trains
  have 3 to 4 members and three runners.

## Decisions for the user

1. **Member gate: block on NOT MEASURED, or allow an accepted not-measured?** I recommend block (item 1). The branch
   d1fe10843 allows an accepted one.
2. **Tag gate: apply ER-12's per-test rule, or keep "fully green only"?** I recommend the per-test rule in the one verdict
   reader (item 2). The cheaper common-practice option is the same rule; the alternative is to fix every flake before each
   tag.
3. **Quarantine list against ER-46.** A flake list that grows needs your approval under ER-46. I recommend no list:
   flakes stay in the run and become fix items (item 2), so nothing needs to grow.
4. **When to re-enable the nightly** (item 7).
5. **Item 5 goes past rust-lang's practice** (they find the culprit by hand), at 1 day. Build it, or keep doing it by hand?

Recurring: flaky waits and time budgets under load (lessons from line 1909 on: start waits, ready reads, "Tests summed
their steps under one bound", the WebKit live-tail entries); OS-only failures found on the train (ER-9's runs 8138 to
8154, train 18); a gate that exists but reports NOT MEASURED and is accepted (ER-33; train 18's member gate).
