# 0121. Whole-repository tests declare a cost class, and nextest budgets each class in one place

- Status: Accepted 2026-11-24 by the user (D1 (a) the name prefix, D2 (a) the tree class by measured cost, D3 (a) report-only first,
  blocking only in the step that records the first green CI run id; building approved the same day; ER-21). Amended 2026-11-24 by the
  user's rule "Test should run no matter the load" (D4 (b), the complete option): see the amendment at the end.
- Date: 2026-11-24
- Related: ER-12 (no limit raised without a measured cause next to it), ER-20 (cheaper or more parallel, measured), ER-21 (a recurring
  class gets a redesign), ER-28 (a cause is measured), ER-33 (a check needs a positive control), ER-37 (common practice and cost), ER-41
  (new checks start report-only), ER-44 (a complete fix), ER-46 (lists only shrink), ER-48 (lanes x threads within the cores); ADR 0071
  (tests judge themselves on progress), ADR 0095 (whole-tree tests and the repository copy; its `whole-tree` check was cut on
  2026-11-11), ADR 0103 (tests bound each step), ADR 0117 (the repository graph).
- Number: 0121. On 2026-11-24 the highest ADR on any branch, local or `origin`, was 0120 (`docs/adr-mac-browser-cap-runner`).

## Context: the class

**A paddock-dev test that does whole-repository work (runs `cargo metadata` over the workspaces, walks or parses every tracked file, runs
the conformance line) fails a landing run on a loaded shared host, although nothing in the product is wrong.** Each instance was fixed
by naming one test: a waiver with a longer budget, or one more name in the `repo-graph` group's filter. Nothing found the next one.

| # | Where | Instance | Source |
|---|---|---|---|
| 1 | macOS | graph readers started together queued on cargo's package-cache lock past 60 s; the `repo-graph` group was made, by name | run 7787 |
| 2 | Mac, 2026-11-11 | the live tests: 42-49 s quiet, over 240 s under I/O load; waivers by name | nextest.toml, 2026-11-11 |
| 3 | Mac, 2026-11-18 | check_engine::this_working_tree_ 58.8-60 s, check_c122::live_tree_ stopped at 60 s; added to the waiver by name | fix/paddock-dev-load-flakes |
| 4 | Windows | the pre-push property test, 1,651 git processes, past 60 s (a git start costs about 45-100 ms there, about 6 ms on the Mac: ER-20) | run 8138 |
| 5 | Windows | the ownership test (25 `cargo metadata` runs) stopped at 60 s, then 18.2 s; made cheaper and added to the group by name | run 8694, d0edac49c |
| 6 | Windows | repo_model::code's syn test stopped at 60 s beside the ownership test, then 23.4 s: a graph reader outside the group | run 8830, job 47457 |
| 7 | macOS | ci_run's conformance-line test stopped at 60 s on both tries (7.7-12.1 s locally); every whole-tree test ran 5-15 times slower with two heavy jobs on the host | run 8830, job 47411 |

The integrator's measured run on 2026-11-24 (fix/graph-train-r5-repo-graph-group, docs/benchmarks.md) found 15 tests that ran
`cargo metadata` outside the group, 7 of them over the real tree. Two causes are mixed in the table: **contention** (cargo's lock: 1, 5,
6) and **CPU under a shared host** (2, 3, 7: on the shared Mac a job had 8 test threads on a 4-core share, the ER-48 exception removed on 2026-11-24; whole-tree tests do not wait, they compute, so eight of them at once over-subscribe the share). Making single
tests cheaper (4, 5) is right and stays right (ER-20), but does not stop the next one.

## Common practice

- **cargo-nextest** (the runner this repository uses) has exactly the three knobs: per-test overrides selected by a filter expression,
  `test-group` with `max-threads` (mutual exclusion or a cap for tests that share a resource), `threads-required` (a heavy test takes
  several of the run's threads, counted against both the run's and its group's limit) and `slow-timeout` with `terminate-after`. Overrides
  in `[profile.default]` apply to every profile after the profile's own. nextest sets `NEXTEST_TEST_GROUP` (`@global` outside a group),
  `NEXTEST_PROFILE`, `NEXTEST_TEST_NAME` and `NEXTEST_WORKSPACE_ROOT` for each test. Sources: nexte.st/docs/configuration/per-test-overrides,
  /test-groups, /threads-required, /docs/features/slow-tests, /docs/configuration/env-vars (read 2026-11-24, nextest 0.9.152; the CI pin
  0.9.149 has the same variables).
- **Bazel** gives every test a declared `size` (`small`, `medium`, `large`, `enormous`) that sets both its default `timeout` (60, 300, 900,
  3600 s) and the resources the scheduler reserves for it; `tags = ["cpu:N"]` reserves cores and `exclusive` runs a test alone.
  `--test_verbose_timeout_warnings` reports tests whose real time does not match their declared size. Source:
  bazel.build/reference/be/common-definitions#test.size and the Test Encyclopedia (bazel.build/reference/test-encyclopedia).
- **Chromium** has a few named timeout classes in code (`TestTimeouts::tiny_timeout`, `action_timeout`, `action_max_timeout`,
  `test_launcher_timeout`, scaled for slow configurations) rather than per-test numbers, and tests that need more are marked and listed,
  not given a bigger global timeout. Source: base/test/test_timeouts.h (source.chromium.org, read 2026-11-24).

The common pattern is: **a small fixed set of declared classes, each with its resources and its timeout, set in one place; a test
declares its class; a tool reports a test whose time does not match.** What this repository had is the step before it: nextest's knobs,
selected by lists of test names.

## Decision

1. **Two cost classes, declared by the test function's name** (`src/test_cost.rs`, `Class`): `graph_` (runs `cargo metadata`, on the
   real tree or a fixture's workspaces; cargo's lock is the shared resource) and `tree_` (reads the whole real tree: every tracked file,
   a parse of every source, the conformance line). A test that does both is `graph`. A name prefix is the only declaration nextest can
   select on without a generated list, it is visible where the test is read, and a moved or new test carries its class with it. Module
   paths are left alone (the OS placement of ADR 0117 §7 and the member audits select by module).
2. **One class table** in `tools/paddock-dev/.config/nextest.toml`, as overrides of the default profile (so `ci` and `member-audits`
   inherit it): per class its filter (`test(/(^|::)<prefix>[^:]*$/)`), its group, `threads-required = 2` and its `slow-timeout`. No
   other override sets a group or threads; the per-name waivers and group lists are gone. A test of the module checks the table with the
   TOML parser (ER-42).
3. **Budgets, each with its measurement next to it (ER-12).** Both classes keep the budget the named waivers already had, four periods of
   60 s, not raised for them: the graph readers' (golden about a minute on a loaded Windows runner, the train replay 91.5-98.4 s on the
   loaded Mac; repo_model::code's syn test 29.8 s running alone on the Windows runner, run 8838, half the old 60 s stop) and the live tests' (42-49 s quiet, over 240 s under I/O load, 58.8-60 s at a load of 50-80). A test that had the unclassed
   60 s stop and joins a class by measurement (ci_run's conformance-line test, instance 7) gets the class's budget together with the
   class's lower concurrency, which is what removes the 5-15 times slow-down; its numbers are recorded with it. Unclassed tests keep 30 s
   periods (2 in ci, 4 locally).
4. **Concurrency follows the lanes (ER-48).** Each class test takes 2 of the run's test threads, each class group at most 4 threads (two
   tests), so at most four class tests run at once in a job. CI jobs get their core share as test threads (Linux 30 / 6 lanes = 5,
   Windows 16 / 3 = 5) or, on the shared Mac, the measured 8 for tests that mostly wait (3 lanes x 8 on 12 usable cores). The class tests
   compute, one busy core each: four per Mac job is its core share of 4, and on a 5-thread lane at most two run (2 x 2 = 4 <= 5). The
   layout of lanes and test threads stays in build-jobs (`ci_build_jobs.rs`), the one place that already warns when a host does not match
   it; nothing per machine.
5. **The graph class is checked exactly, at run time** (the integrator's proposal, folded in): every place in the tool that starts
   `cargo metadata` calls `test_cost::charge(Graph)` first (`plan::read_graph_placing`, `pre_push::members::read_members`,
   `pre_push::linux_inputs::left_out`, `locks_guard`'s metadata reader) or `charge_argv` for a command line it is handed (the `locks`
   runner, the pre-push gate runner). Under nextest in this workspace it reads `NEXTEST_TEST_GROUP` and refuses, with the next step,
   when the test is not in `repo-graph`. A `./dev` that a test starts inherits the variables (ER-47), so the binary's reads are judged as
   the test's. Outside nextest, or under another workspace's nextest, it does nothing: production calls never see it. Open point 1 of the
   integrator (a test condition in the tool's code) is accepted for this reason: it is a pure read of nextest's variables, scoped to this
   workspace, with no effect elsewhere.
6. **The tree class is checked by its cost** (Bazel's verbose timeout warnings): whole-tree reads have too many entry points to guard each
   one (git ls-files, syn, node, `./dev check`), and cost is what defines the class. `./dev ci test-cost --run <id> | --log <file>` reads
   nextest's own status lines through the one reader of nextest output (`nextest_log::durations`, only whole runs), prints each class's
   longest test (the budgets are re-measured on the CI hosts from every landing run) and reports every unclassed test that used more than a
   quarter of its stop (15 s of 60 s in ci): no margin for a loaded host.
7. **Report-only first (ER-41, D3 as the user decided it), and visible.** The charge refuses nowhere yet (`test_cost::ENFORCED_PROFILES`
   is empty). Every judged charge, placed or not, is appended as one JSON line (test, binary, class, group, profile, call, placed) to a
   report the run owns: the file `PADDOCK_TEST_COST_REPORT` names (CI sets it on the paddock-dev Tests step, per run and attempt under
   `runner.temp`), else `<target>/nextest/test-cost/<NEXTEST_RUN_ID>.jsonl` in the build folder nextest ran from (found from
   `NEXTEST_BIN_EXE_paddock-dev`, never the checkout). A misplaced charge also goes to the
   test's stderr. The next step, `./dev ci test-cost --report <file>`, prints a `::warning::` per misplaced test and a table in the job
   summary, and exits 0; a report with no charge is "not measured", said, never "clean" (ER-33: nextest hides a passing test's output, so
   stderr alone would be a report nobody sees). The step that records the first green CI run on the real tree, by run id next to the
   constant, makes the charge refuse under `default`, `ci` and `member-audits` and passes `--fail-on-findings` in the same edit.
   CI wiring (two declared edits for the integrator, `.gitea/workflows/ci.yml`, job `paddock-dev`): the `PADDOCK_TEST_COST_REPORT` line
   on the Tests step, and the `Test cost report` step after it.
8. **Positive controls (ER-33).** A planted environment with the test outside the group is refused under an enforcing profile, and
   found but not refused in today's report-only mode; in the group, or outside this workspace, it passes. Live: a test named with each
   prefix observes its class's group under nextest, and an unclassed one observes `@global` and its charge is found. A planted misplaced
   charge appears as a line of the report file, in the `::warning::` text and in the job-summary table. A planted log with an unclassed 48 s test is a finding, the same
   time in a class or another workspace is not, and a partial log is an error, never a shorter list.
9. **Migration (ER-44).** Every existing site is renamed into its class in the same change, found by measurement, not by reading call
   chains (docs/benchmarks.md, 2026-11-24, "test cost classes"):
   - **graph, 44 tests.** 42 found by one full ci-profile run at 8 test threads with a `cargo` on PATH that logged each `metadata` call
     with its test and group (1028 calls): golden (3), tree (25), repo_graph_net (6), repo_model (4), locks_guard (4), gates_planner (2).
     2 more found by the charge itself in the first run with the class table: two `pre_push::tests` that run `cargo metadata` in a child
     whose environment the test replaces, so the shim never saw their test name (the charge, in process, did). The charge also showed that
     reading the graph is not the cost: a fixture with no workspace starts no `cargo metadata`, so the charge sits on each metadata run (an
     adapter around the `CargoMetadata` port), not on `read_graph`.
   - **tree, 29 tests.** The tests that read the whole real tree and took 6 to 59 s in that run: ci_run's four whole-tree tests (the
     conformance-line test of runs 8830 and 8843 among them), check_engine's working-tree test, nine os_scoped tests (each copies the
     tree's sources), ipc_endpoints' register test, two `check::live_` and three `check_c1xx` working-tree runs, golden's repository-copy
     test, and eight `pre_push::` tests of `tests/it` that run `./dev check --pre-push` over the real tree. Those eight were in the old
     `repo-graph` group by its module filter although they run no `cargo metadata`; taken out of it without a class they ran 2.5 to 4
     times longer in the first measured run (up to 52 s of a 60 s stop), which is how they were found.
   - Whole-tree reads that took under 4 s there (contracts, check_c129, check_c131 and similar) stay unclassed: their cost does not need
     the class's threads, and `ci test-cost` reports any of them that grows past a quarter of its stop. The member-audits filter keeps
     its clauses with an optional `tree_`; the ci-run golden's regenerate command names the renamed test (ER-40: regenerated, not
     edited). No baseline list.
10. **ER-48 on the shared Mac, stated.** build-jobs gives a macOS job 4 test threads, its share of the 12 usable cores (the former 8-thread
   exception was removed on 2026-11-24). The class tests are the ones that compute: with two threads each and two per group, at most
   two of them run in a job (4 threads), the job's 4 cores; three Mac jobs then keep their busy tests within the 12 usable cores. On Linux and Windows
   (5 threads, the core share) at most two run. There is no exception for waiting tests any more; nothing per machine is set by hand.

## Consequences

- Once the check is blocking, a new graph reader fails on its first run with the rename named; a new whole-tree test shows in `ci test-cost` of its first CI
  run. Neither needs a person to remember a list.
- Class tests run at most four at a time, so a run of paddock-dev can take longer in wall time on a quiet host (measured, docs/benchmarks.md
  2026-11-24, "test cost classes"); on a loaded shared host they keep inside their budget.
- Renaming tests changes their names in history (flake records, lessons). The old names stay readable in the prefix-less tail of the new
  ones.

## Decisions (accepted by the user, 2026-11-24)

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | The declaration | (a) a name prefix per class; (b) the current lists of names in nextest.toml, checked by the run-time charge only | **(a), accepted** |
| D2 | The tree check | (a) by measured cost from nextest's output; (b) a run-time charge at each whole-tree entry point | **(a), accepted** |
| D3 | Blocking | (a) report-only in CI first, blocking only in the step that records its first green CI run id (ER-41); (b) blocking in CI at once | **(a), accepted** |

## Cost

Common practice (nextest's groups, threads and timeouts on lists of names, as before plus the integrator's measured list): about 0.25
agent-day, already done on fix/graph-train-r5-repo-graph-group. This design: about 2 agent-days (the class module and charge 0.5, the
cost report 0.5, the class table and ER-48 layout 0.25, migration of the sites 0.5, measurements and this ADR 0.25), so about 1.75 days
beyond common practice. It is foundation work (ER-37: the complete option is recommended) and the user approved building it on
2026-11-24; the cheaper option is D1 (b).

## Amendment 2026-11-24: tests run no matter the load

The user's rule, 2026-11-24: **"Test should run no matter the load."** Scheduling the classes (above) lowers contention but does not
make a busy host harmless: a 60 s stop still fails a test that a host slows down 15 times. A wall-clock stop has to become what Bazel's
and Chromium's are: a detector of hangs, not a limit on performance.

### Common practice

- **Bazel**: a test's timeout comes from its `size` or `timeout` attribute in four tiers, `short` 60 s, `moderate` 300 s, `long` 900 s,
  `eternal` 3600 s; it is there to stop a test that hangs, and `--test_verbose_timeout_warnings` reports a test whose real time is far
  under its tier so the tier can shrink. A test's resources (`cpu:N`, `exclusive`) are declared separately from its time
  (bazel.build/reference/be/common-definitions#test.size; bazel.build/reference/test-encyclopedia, read 2026-11-24).
- **Chromium**: `TestTimeouts` gives a few named timeouts (`tiny_timeout` 100 ms, `action_timeout` 10 s, `action_max_timeout` 30 s,
  `test_launcher_timeout` 45 s) that are multiplied for slow configurations (debug, sanitizers, a timeout multiplier on the command line)
  instead of each test choosing a number; a test waits on conditions and uses these only as the bound of a hang
  (base/test/test_timeouts.h, read 2026-11-24).
- **Hang by progress**: neither runner judges CPU. nextest has a wall-clock `slow-timeout` per test and runs a test through a wrapper
  program when asked (`[scripts.wrapper]`, experimental, nextest 0.9.98 and later; the CI pin is 0.9.149). Paddock's push gates already
  judge a step by progress (rule 89's watchdog, src/pre_push/stall.rs), and `paddock_proc` reads a process tree's CPU time on all three OS.

### Decision

- **D4 (b), accepted by the user 2026-11-24: tiers plus a progress guard.**
  1. **Time tiers** (`.config/nextest.toml`): a stop is at least the test's own time times the worst measured contention, 15 (whole-tree
     tests ran 5 to 15 times their local time in jobs 47411 and 47644; `test_cost::CONTENTION`), rounded up to Bazel's tiers. Unclassed
     tests in CI: moderate, 300 s. `tree` and `graph`: long, 900 s. The train replay: eternal, 3600 s. The measurements are next to the
     numbers. The default profile keeps C114's 120 s for a run by hand; C114's override cap moves from 480 s to 3600 s (Bazel's
     `eternal`), the cap on an override and still never on the default.
  2. **`ci test-cost` judges each test against its own stop** (read from the configuration with the TOML parser): a test whose time
     times 15 passes its stop is a finding, in a class or not. This replaces "a quarter of the stop".
  3. **Asserts on elapsed wall time are listed** (`ci test-cost --wall-asserts`, report-only, in the CI step): an `assert` in a `#[test]`
     function that reads `.elapsed()`, `duration_since` or `Instant::now()`, or a local bound from them. The next step for each is an
     injected clock or a bound on CPU time. The list is a fix list, not a gate.
  4. **`./dev test-guard`** (nextest's wrapper for every test of paddock-dev): runs the test, forwards its output, and samples the CPU
     time of its process tree. A test that spends no CPU time and prints nothing for the guard's gap is a hang. Report-only first (ER-41):
     a hang is a line of the run's report (the same file as the charges) and a `::warning::`; nothing is stopped until the step that
     records the first green CI run with it. Each test's CPU time and longest idle stretch go into the report too, which measures the
     tiers' inputs on the CI hosts and sets the gap (ER-12). Once it enforces, the tiers can widen to backstops.
- **Cost** against the baseline (Bazel's tiers alone, which is item 1 and about 0.5 day): items 1 to 3 about 1 to 1.25 agent-days, item 4
  about 1.5 more; about 2.5 to 3 in all. The user chose the complete option on 2026-11-24.
