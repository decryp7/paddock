# 89. A measurement that did not happen can never pass: one outcome type, one test outcome, one exit code, and a check over every copy

- Status: Accepted 2026-11-10 (user), all decisions as recommended, D14 (a). History: Proposed 2026-11-10, revision 6
  approved by the independent review at `d3acc649c`. Drafted at the coordinator's request of 2026-11-10 (ER-21: one class, seen
  six times in one day's Rust tooling reviews, after the Dyno 1.1.0 instances that created ER-33). Revision 1
  (`e5e4f300d`) was reviewed independently ("changes", 6 P1 and 12 P2); revision 2 (`90d28a0a6`) answered it and was
  re-reviewed ("changes": P1-7 and four P2s); revision 3 (`197d5234e`) answered that and was re-reviewed ("changes":
  P1-8, two P2s); revision 4 (`264468b3c`) answered it and was re-reviewed ("changes": P1-9, three P2s); revision 5
  (`7680e9d79`) answered it and was re-reviewed ("changes": P1-10, three P2s); revision 6 answers it.
  Nothing here is built yet.
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM: keep WP-0, WP-1 (`Outcome`, `Verdict`, exit codes 6 and 7), WP-2 (`Measured`, `require_tool`) with fail-closed on release runs, the WP-3 ban on silent skips as a grep check, WP-5 step 1 and its test-body sites, WP-6; dropped the per-tag-workflow wiring, the runner-log reader, the C103 syn pass and the tooling copies.
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-22 / rules 42 and 55 (share first; one implementation per mechanism,
  `docs/known-copies.toml`), ER-12 (no silent skips), ER-27, ER-31, ER-33 / rule 71; ADR 0022 and its 2026-10-31
  amendment (`paddock-cli`'s `Exit`, the shared runtime `paddock-tool`, G1), ADR 0038 (CI parity before push: its table
  of skipped steps, `ci-only` declarations and the G26 stamp), ADR 0088 (accepted 2026-11-10: "not measured, never stale
  and never fine" for list entries, run kinds in its §3, the RT-5 run record); the plan
  `docs/plans/rust-tooling-2026-11-10.md` on `origin/docs/rust-tooling-plan` (§3 positive controls, RT-5 check host,
  RT-12 push gate, RT-17, RT-22 `paddock-tool`); lesson `docs/lessons.md:1438` (Dyno 1.1.0).
- Number: 0089 was free on every `origin` branch on 2026-11-10 (highest file `0088`; no branch's `docs/` names "ADR 0089").
- Evidence: read on 2026-11-10 after `git fetch`, at these heads: `origin/train/2026-11-09-3` `67a633ef3` (every count
  without a ref), `origin/feat/rt2-release-isolation` `ad9d85a5b`, `origin/feat/rt3-dev-worktree` `2be92ec01`,
  `origin/feat/rt4-hook` `48b938bcb`, `origin/feat/rt5-check-host` `0a94bf347`, `origin/feat/rt8-ci-flakes` `da05b54e8` (incident #6 reviewed at `5da33f8cf`),
  `origin/fix/playwright-flaky-as-failure` `06adcb058`, `origin/docs/adr-windows-gate-ratchets` `db7782e01`,
  `origin/docs/adr-captured-tool-output` `9687d644b` (ADR 0090). Each count
  names its command (Appendix B). Measurements on synthetic input only (Appendix A): a throw-away crate under `cargo
  test` and `cargo nextest run` (cargo 1.99.1, nextest 0.9.151); the JS host's `applyWaivers` on a planted crash; the
  local `paddock-ci:v2` image (`sha256:fdc6b409ce5c…`, created 2026-10-24) probed with `docker run`.

## Context: the incidents

| # | Branch, commit | What did not measure | How it read as a pass |
|---|---|---|---|
| 1 | RT-2 release isolation; `93cdaf957`, fixed by `e59463f8f` | no `--tauri-pins` file: the Tauri tool folder was compared with nothing | the line went to `Verdict::notices` ("not measured and are not faults") and the JSON said `"passed": true` |
| 2 | RT-3 `./dev worktree`; `fdb4c3e44`, then `d055bcf31`, fixed by `2be92ec01` (a pinned copy of the oracle) | the `worktree.sh` oracle, `WORKTREE_SH` unset | first `#[ignore]` (no CI line runs ignored tests); then helper `oracle()` printed `oracle not measured` and each test did `if oracle().is_none() { return; }` (lines 77, 97, 137). The doc said "CI sets it"; no file under `.gitea/` names `WORKTREE_SH` |
| 3 | RT-4 hook; before `fb4c579fa` | the live shell-hook oracle, `bash` or `jq` missing | `None => return eprintln!(..)`, and in another test `let Some(denies) = old_denies(script, cmd) else { return };` with no print at all |
| 4 | RT-8 `./dev ci flakes`; `dff738952` | (a) a job log that could not be read; (b) Playwright's "flaky" marker where `failOnFlakyTests` is not set | (a) an incomplete report exited 5, the code of "measured, found something"; (b) Dyno, Pitcrew and Spotter set `retries: CI ? 1 : 0` and Pitwall and ui `retries: 0` on the train (`origin/main` still has Pitwall at `CI ? 2 : 0`), and only Debrief set `failOnFlakyTests`, so a test that passed on retry was invisible and the report said `complete` |
| 5 | RT-5 check host; `40b7faf8f` | a check that did not complete, or crashed | `mod.rs:295` added the unwaivable "not measured" error only `if !record.completed && found.is_empty()`, so a waiver for the one finding of a half run cleared it; the crash finding was waivable |
| 6 | RT-8 again, review of `5da33f8cf` (fixed in `da05b54e8`) | partial CI input: a job log cut before its runner's summary; a 404 on an attempt's job list; failed jobs that never ran a step | the cut log counts as a clean "non-test failure" and exits 0; the 404 reads as "no jobs"; the step-less failed jobs do not make the report incomplete. Cause: a second nextest reader, `ci_runs/logfind.rs` (`read(log) -> Findings`, no "could not read" state), a weaker copy of `plan_since::nextest_failures` (`-> Result<Vec<FailedTest>, String>`) |

And the Dyno 1.1.0 instances that created ER-33 (`docs/lessons.md:1438`): a VM with no menu bar "confirmed" a tray cause
in 12 of 12 runs, a tray count matched the app's own window buttons, a reading row passed on a dead helper, and
`renderResult` printed PASS on zero steps.

**Where they stand now.** #1 to #3 are fixed in their branches. #6 is fixed in `da05b54e8`: it adds a shared nextest
reader, `tools/paddock-dev/src/nextest_log.rs`, used by both `plan_since` and `ci_runs/logfind.rs`; a partial log is
refused, a 404 on a job list is not measured, and a failed job that never ran a step is a gap in the report. That is
the nextest part of §4b's reader; `logfind.rs` still exists (as a caller), and RT-8's exit 1 for an incomplete report
stays until WP-4 moves it to 6. #5 is fixed: `8550764e7` makes incomplete, crashed, broken and precondition-failed runs add
an unwaivable error, and at `0a94bf347` one function, `RunRecord::judge(&self, kept: bool) -> (Status, Option<String>)`
(`check/record.rs:112`), decides status and error together; a scoped run that reads nothing is `NotApplicable`, and a
`--files` list none of whose entries is a file is `NotMeasured`. One difference from this ADR remains: `judge` makes a
crash `NotMeasured`; this ADR makes it `CheckBroken` (§1), which WP-4 changes in RT-5 with a shadow-table line. #4: `5da33f8cf` (with `d95a47ceb`) exits **1 with an `::error::`** for an incomplete
report and records the Playwright precondition as a gap; the source fix for (b) is `06adcb058` on
`origin/fix/playwright-flaky-as-failure` (`failOnFlakyTests` for Dyno, Pitcrew and Spotter, and C71 fails a config with
retries and no `failOnFlakyTests`). **Still open:** the JS conformance host's crash (`conformance/src/runner.mjs:24`
makes it an ordinary `error`; measured: a `path = "*"` C53 waiver moves a planted `the check crashed: boom` from
`findings` to `waived`; no such waiver exists today, so at risk, not broken). RT-5's shadow run uses that host as its
oracle, so the oracle agreed with the bug.

**Why print-then-return is silent, measured** (Appendix A.1). A test that prints a marker with `eprintln!` and returns:
`cargo test` printed it 0 times and reported 1 passed; `cargo nextest run` printed it 0 times and summarised `1 passed,
0 skipped`. Both hide a passing test's output by default. The repository opts two tests out: profile `ci` sets
`success-output = "final"` for `test(/^golden::live_/)` (`tools/paddock-dev/.config/nextest.toml:25-27`) and for
paddock-service's cost probe (`crates/.config/nextest.toml:52-54`), and CI runs `--profile ci`. So golden.rs's
`::notice::` **does** reach the job log, as a notice in a green run, which no gate reads; every other skip line is hidden.
libtest has no runtime skip: proposals ([pre-RFC](https://internals.rust-lang.org/t/pre-rfc-skippable-tests/14611),
[later thread](https://internals.rust-lang.org/t/skippable-tests/21260); read 2026-11-10) were not accepted; whether a
newer RFC moved is **unverified**. What libtest does accept is any return type that implements
[`std::process::Termination`](https://doc.rust-lang.org/std/process/trait.Termination.html); Appendix A.2 shows it works
under both runners, which is what §4 builds on.

## Pattern

**A check, probe or test that could not look reports in a form some caller reads as a pass**: a notice, a warning, an
`eprintln!` and `return`, a bare early `return`, an ignored test, a waivable finding, a bool, or an exit code shared with
another meaning. Each tool decides on its own how to say "not measured", and the meaning is lost at the first conversion.

**Invariant.** "Did not measure" and "the probe is broken" are values of their own, distinct from "clean", "not
applicable" and "findings", from the probe to the process exit code; no step (waivers, baselines, report-only findings,
aggregation, JSON, exit codes, the test harness) can turn either into a pass. A run with one passes only where reviewed
data declares that measurement optional in that place, with a reason and an expiry; in a release run, only where another leg of the same run measured it (D14).

## Instances

**Ways to say "not measured" in tooling** (copies of one mechanism, rule 55):

| # | Where (ref) | Representation | State |
|---|---|---|---|
| 1 | `tools/paddock-dev/src/gates/` (train): `evaluate.rs` 12, `register.rs` 3, `gating.rs` 2, `edge.rs` 1 | text in error findings | fine (errors), stringly |
| 2 | `release_isolation/` (RT-2 `ad9d85a5b`, 40 lines) | `Leak`/`Probe` enums, `is_broken()`, `notices` | fixed; the local-trial flag still gives a notice, refused on a tag |
| 3 | `worktree/` (RT-3) | `"not_measured_only"` JSON flag, `pub const EXIT_NOT_MEASURED: i32 = 6` (`worktree/mod.rs:14`) | fine, private exit code |
| 4 | `check/record.rs` (RT-5 `0a94bf347`) | `Status { Clean, Findings, NotMeasured, NotApplicable, CheckBroken }` (`:71-75`), `measured() -> bool` (`:99`), `judge(kept)` (`:112`) | fine; the shape of §1, moved into it by WP-4 |
| 5 | `ci_runs/` (RT-8 `da05b54e8`) | `complete() -> bool` (`flakes.rs:154`), exit 1 | fine for now (1 is "failure"); moves to 6 in WP-4 |
| 6 | `hook/oracle_tests.rs` (RT-4) | `panic!("the live oracle is not measured: …")` | fine |
| 7 | `tests/it/golden.rs` (train) | `skip(reason)`: panic in CI on Linux, `::notice::` on CI host legs (shown through `success-output = "final"`), `eprintln!` locally (hidden) | at risk: the host-leg notice is a green run; an `[[optional]]` entry is its right form |
| 8 | `conformance/src/finding.mjs`, `util/waivers.mjs`, `runner.mjs` | `unwaivable` flag by convention; crash not unwaivable; CLI exits 0, 1, 2 and passes a sub-command's status through (`cli.mjs:95, 107, 136`) | **at risk** (measured) |
| 9 | `C99-no-keychain.mjs` | unwaivable errors; two "not measured" **warnings** (`--no-graph`, line 247; no clippy.toml, line 305) | fine by declaration (`--no-graph` never in CI; the demos' deviation in ADR 0057 §3.5); both become `[[optional]]` entries |
| 10 | `scripts/vm-smoke/verdict.mjs` | `not-measured`, `check-broken`; exit 3 = INCOMPLETE | fine, but 3 is `paddock-cli`'s `NotRunning` |
| 11 | `conformance/release/check-per-machine-installer.mjs` | text in problems | fine (fails) |
| 12 | runner-log readers: `plan_since::nextest_failures` (train) and `ci_runs/logfind.rs` (RT-8), Debrief's `e2e/retried-reporter.mjs` (Playwright) | two nextest readers that drifted; neither had a "partial input" result except by error text | fixed for nextest in `da05b54e8` (one `nextest_log.rs` under both); Playwright and vitest still per tool (WP-2c) |
| 13 | per-app strict switches: `PADDOCK_REQUIRE_TEST_BROWSER`, `DEBRIEF_`, `SPOTTER_`, `DEMO_REQUIRE_TEST_BROWSER` (e.g. `spotter-render/tests/real_browser.rs:27-35`) | lenient by default, strict when CI sets the variable | fine where CI sets it; four copies of one switch |

**Early exits in tests.** An approximation over every `#[test]`/`#[tokio::test]` body (Appendix B, a token scan that
skips closure and nested-fn bodies; the exact number comes from WP-3's syn pass): **126 `return` tokens in 71 files**:
37 `let … else { return }`, 25 `if … { return }`, 64 others (mostly `return;` after a print). The print-based subset
(`git grep` for a print with skip, missing and similar words) is 47 lines in 30 files; classified by message:

| Kind | Sites | Examples | Right form |
|---|---|---|---|
| a tool or capability is missing | 26 | `real_ffmpeg.rs:45`, `video/tests.rs:546` (ffmpeg: measured absent in `paddock-ci:v2`), `debrief-ipc/tests/it/launch.rs` (landlock, seccomp: 4), `storage/walk.rs` and `paths.rs` (links, undeletable file: 5; `walk.rs:308` skips when root, and the image runs as uid 0, measured), `dotnet_win_tests.rs` (csc.exe: 3), ETW tests that skip when elevated (3), `pitwall-server/tests/it/e2e.rs:510` (sparse file), golden.rs (3) | `require_*` (§4) |
| opt-in by a variable | 17 | `PADDOCK_VM` (5), measurements (5), `DEBRIEF_ETW_TESTS` (2), `CHROMIUM_TEST_PATH` (2), `DEBRIEF_EVTX_CORPUS`, `PITWALL_LLAMA_E2E`, `PADDOCK_REAL_WINDOWS` | `#[ignore = "<job that runs it>"]` plus `require_env` |
| wrong platform at run time | 1 | `eventlog_live.rs:12` | `#[cfg(...)]` |
| a race the test lost | 1 | `src-tauri/src/live.rs:772` | a fake clock (ER-12); its own bug |
| other | 2 | `pdb_symbols.rs:285`, `dumps.rs:913` | `require_*` |

Silent ones without any print are in the other 79: for example `spotter-render/tests/real_browser.rs:86, 109, 128, 144`
(`let Some(b) = browser() else { return };`, made strict in CI by `SPOTTER_REQUIRE_TEST_BROWSER`, copy 13 above).

JS tests skip through the runner (45 `skip:`/`.skip(` sites in `*.test.mjs`, `*.spec.ts`, `*.e2e.ts`), which is visible
but not failing: `c99-clippy-fixture.test.mjs:33` (`hasClippy`), `c99-graph.test.mjs:330` (`skipCargo`).

`#[ignore]`: 91 sites in 44 files, 33 naming a test VM; no CI line runs ignored tests. Out of scope (D10).

## Decision

### 1. One outcome type, in `paddock-cli`

`paddock_cli::outcome` (the crate already owns `Exit`; `paddock-tool` re-exports it when RT-22 exists):

```rust
#[must_use]
pub enum Outcome {
    Clean,                     // measured, nothing found
    NotApplicable(Reason),     // a scoped run (--files, an app scope) reached nothing this check reads
    Findings(Findings),        // measured; each finding failing or report-only
    NotMeasured(Reason),       // did not look: tool missing, precondition false, incomplete, zero probes in its scope
    CheckBroken(Reason),       // looked, but its positive control failed, or it crashed
}
```

- **No conversion to a pass**: no `bool` accessor, no `From<Outcome>` for `bool` or `Result`, no `Default`. The only
  exits are `Verdict::exit()` and `Verdict::json()`.
- **Computed once**, by `RunRecord::judge(&self, findings_after_waivers)`, RT-5's function under its own name (WP-4
  moves it into the shared module and changes its argument from `kept: bool` to the findings left after waivers);
  waivers and baselines take `Vec<Finding>` and never see an `Outcome`, so they cannot cancel `NotMeasured` or
  `CheckBroken`. A **crash is `CheckBroken`** (a panic is a bug in the check; an input it could not read is
  `NotMeasured`); RT-5's `judge` says `NotMeasured` today, and WP-4 changes it, listed in RT-5's shadow table.
- **`NotApplicable`** is RT-5's "not applicable": only a scoped run can produce it; a root run, a tag run or a run that
  names no scope reaching nothing is `NotMeasured`. Unknown `--files` entries are RT-5's counted warnings, and an error
  when all are unknown (§2 treats that as `NotMeasured`).

### 2. Aggregation and exit codes (a shared change)

`Exit` gains `NotMeasured = 6` and `CheckBroken = 7`; `Findings` stays 5. `Verdict::of(outcomes, &Optional, run_kind)`:

1. any `CheckBroken` → **7**;
2. else any **failing** finding → **5** (the JSON and table still list every not-measured item);
3. else any `NotMeasured` not covered by an `[[optional]]` entry → **6**, whatever report-only findings the run has
   (a report-only finding never masks a missing measurement: revision 1 folded "informational findings + not measured"
   to `Findings`, which exits 0 in a report-only run);
4. else **0**: `Clean`, `NotApplicable`, report-only findings, and not-measured items that are all declared optional.
   The JSON lists the optional entries used.

RT-3's private 6 becomes `Exit::NotMeasured`; RT-8 moves from 1 to 6 (WP-4); vm-smoke's 3 moves to 6 with its port.
The JS conformance CLI (0, 1, 2, and a passed-through sub-status) adds 6 and 7 for its own checks; the pass-through
stays; each change is an intended difference in RT-5's shadow table (as ADR 0088 §7.2).

### 3. Optional measurements are reviewed data: `docs/optional-measurements.toml`

```toml
[[optional]]
id = "test:debrief-video::real_ffmpeg::*"    # or "check:C99/graph", "gate:windows-tests", "probe:release-isolation/tauri-tools"
where = { leg = ["linux-container"], os = ["linux"] }
reason = "paddock-ci:v2 has no ffmpeg (measured 2026-11-10); the image adds it in WP-5"
expires = "2027-01-31"
```

- **Readers:** the test outcome (§4, Rust and JS), `./dev check`'s `Verdict`, the push gate (ADR 0038). Each records
  which entries it used, in its run record.
- **As an ADR 0088 register:** an entry is **covered** by a run whose leg and OS match its `where`; covered and used is
  live; covered by every reader's runs and never used is stale (0088 §3's severities); an entry whose `where` no run
  covered (a leg that did not run) is **not measured, not stale**. Expired is an error (as `W001`).
- **Never optional:** `CheckBroken`, anywhere. **In a release run** (ADR 0088 §3's set: tag refs, `release/**`
  branches, `<app>-release.yml` workflows) an entry counts only under D14's rule (§3a).
- **How a run knows its kind:** a tag or `release/**` run from `GITHUB_REF` (`refs/tags/…`, `refs/heads/release/…`); a
  release workflow from an explicit `PADDOCK_RUN_KIND=release` that each `<app>-release.yml` and the tag path of each
  `<app>.yml` set in their `env:` (`GITHUB_WORKFLOW` is the workflow's `name:`, not its file, and whether Gitea sets
  `GITHUB_WORKFLOW_REF` is **unverified**). A caller's workflow `env:` does not pass into a reusable workflow, so
  `ci.yml` sets it itself, at workflow level, from its `release` input or a tag ref, the same expression as
  `PADDOCK_DEV_DOWNLOAD` at `ci.yml:83`:
  `PADDOCK_RUN_KIND: ${{ (inputs.release == true || startsWith(github.ref, 'refs/tags/')) && 'release' || '' }}`.
  Anything else is an ordinary run.
- **Rule 54 dispatches** of an `<app>-release.yml` (a manual run with gates on) are release runs: the file is the
  tag-triggered one and sets `PADDOCK_RUN_KIND=release`. **Not release runs** (ADR 0088 §3's set): `debrief.yml`'s
  manual run with `bundles: true` (a branch ref, no `PADDOCK_RUN_KIND`), and `nightly.yml`'s calls of
  `app-release.yml` (`:216`, `:263`, `:295`, `:325`; nightly sets none).
- **Records and the 0088 engine:** `check:`, `gate:` and `probe:` entries are judged stale or live by 0088's engine
  from the run records of `./dev check` and the push gate. **`test:` entries are expiry-only**: their records live in
  CI job artifacts (`PADDOCK_NOT_MEASURED_LOG`, uploaded per job) that no run on main reads today, so the engine reports
  them `not measured`, never stale, and their expiry (at most 90 days) is what removes them. A later step may let the
  nightly read the artifacts.
- `paddock.ci.json`'s `ci-only` declarations stay the push gate's source for whole steps (ADR 0038); they count as
  optional entries for the push gate only.

### 3a. Optional entries in release runs (D14)

"Never optional in a release run" cannot hold for unit tests: `debrief-v*` tags run the full gates
(`debrief.yml:27`, the `full` output at `:75`), and the uid-0 Linux container can never measure `walk.rs:308`, nor can a
Docker container without landlock measure `debrief-ipc`'s sandbox tests. The rule (D14 (a), recommended): in a release
run an `[[optional]]` entry is honoured only if it names `measured_on = ["<leg>", …]` and at least one of those legs
**ran the same test green in the same run**. A final job of the tag's workflow, `if: always()` and needing every test
leg, merges the legs' `PADDOCK_NOT_MEASURED_LOG` artifacts with their JUnit reports and fails the release when an
entry's `measured_on` legs did not pass it (where it runs, per workflow: §7). So a measurement may move to another leg,
but never vanish from a release.
ER-33's release checks (vm-smoke, release isolation, the installer gate, the gates register) never take entries.

### 4. One test outcome: `paddock_testkit::Measured`

Revision 1's `-> Result<(), NotMeasured>` cannot work: libtest fails any `Err`. A test returns a testkit type that
implements `std::process::Termination` (prototype measured, Appendix A.2):

```rust
#[test]
fn transcodes_a_clip() -> Measured {
    measure(|| {
        let ff = require_tool("ffmpeg")?;          // Err(NotMeasured) on a missing tool; `?` inside the closure
        let jq = require_env("PADDOCK_VM", "1")?;  // inside an #[ignore = "..."] test
        assert_eq!(run(&ff, ..), ..);              // ordinary assertions panic as usual
        Ok(())
    })
}
```

`measure(f)` returns `Measured::Yes` or `Measured::NotMeasured(reason)`; there is no `unwrap_measured`. Its
`Termination::report()` decides, in this order:

1. **Declared optional** for this test on this leg (`PADDOCK_LEG` and the OS against `docs/optional-measurements.toml`,
   §3) → success, with one record appended to `PADDOCK_NOT_MEASURED_LOG` when it is set. **In a release run this fails
   closed** (revision 6): it succeeds only when the entry names `measured_on` legs, **and** `PADDOCK_LEG` and
   `PADDOCK_NOT_MEASURED_LOG` are both set, **and** the record was written (the file opened and the line flushed);
   any of the three missing is a strict failure, `not measured: release run on an unwired step (no PADDOCK_LEG or
   PADDOCK_NOT_MEASURED_LOG), so the cross-leg check could not see this skip`. The final job (§3a, §7) then checks the
   `measured_on` legs. This is the primary guarantee: an unwired step can never skip silently in a release, whatever
   C103 (d) found; C103 (d) is wiring coverage, so that wired steps are the normal case and failures are rare.
2. **Reporting mode** (`PADDOCK_NOT_MEASURED=report:<file>`, set only by `./dev test` and the push gate, which read the
   file afterwards and fold it into their `Verdict`, so they exit 6) → success after recording.
3. **Otherwise strict** (plain `cargo test`, `cargo nextest run`, CI without an entry) → failure with
   `not measured: ffmpeg is missing (spawn failed: …). Install it, run through ./dev test to list it as a skip, or
   declare it in docs/optional-measurements.toml for this leg.`

So a local "visible skip" is the wrapper's table, never a line a passing test printed, and CI needs no wrapper (D4).
The JS twin is `measure(t, async () => { const cargo = await requireTool('cargo'); … })` for node:test, Vitest and
Playwright: the same three rules, `t.skip(reason)` in cases 1 and 2. Each helper's behaviour is tested once (ER-22).
The four `*_REQUIRE_TEST_BROWSER` switches (copy 13) become `require_tool`-style probes of the pinned browser.

### 4b. One runner-log reader: `tools/paddock-dev/src/runlog/` (nextest, Playwright, vitest)

Every tool that reads a CI job log for test results (`./dev plan --since-run`, `./dev ci flakes`, later the push gate's
and `ci timings`' readers) goes through one module:

```rust
pub fn read(runner: Runner, log: &str) -> Read;     // Runner: Nextest | Playwright | Vitest
pub enum Read {
    Measured(TestResults),                           // the runner's own summary line was found and agrees with the rows
    NotMeasured(Reason),                             // cut before the summary, no runner start line, counts that disagree
}
```

- **Never an empty result for partial input.** "No failed tests" is `Measured` with an empty list only when the
  runner's final summary (nextest's `Summary [...] N tests run`, Playwright's `N passed`/`N failed`/`N flaky` block,
  vitest's `Test Files`/`Tests` lines) was read and its counts match the rows read.
- **The callers' other partial inputs are the same type:** a non-2xx reply for a run's or attempt's job list is
  `NotMeasured("job list of attempt N: HTTP 404")`, never "no jobs"; a failed job with no step that ran is
  `NotMeasured("job failed before its first step")`, so the report is incomplete.
- **One log, several invocations:** a job log can hold two nextest runs (`app-gates/action.yml:153-154`), so a log is
  `Measured` only when the count of runner start lines equals the count of final summaries and each summary's counts
  match its rows; a fixture with two invocations, cut inside the second, must be `NotMeasured`.
- **Truncation controls, per runner:** for each real captured log (RT-8's `tools/paddock-dev/tests/ci-runs/*.log`:
  `nextest-r0-fail.log`, `nextest-r1-pass.log`, `nextest-timeout-r0.log`, `playwright-project.log` and the others; a
  vitest log still to capture), the test cuts it at every line boundary before the summary and asserts `NotMeasured`;
  the whole log gives `Measured` with the known rows.
- RT-8 built the nextest part in `da05b54e8` (`nextest_log.rs`, used by `plan_since` and `logfind.rs`); when §4b lands, RT-8's `ci_runs/logfind.rs` becomes `runlog` (it is not kept
  beside it), `plan_since::nextest_failures` becomes a caller, and the
  mechanism `runner-log` is registered in `docs/known-copies.toml` with Debrief's `retried-reporter.mjs` as a copy
  to retire.
- **ADR 0090** (proposed, `origin/docs/adr-captured-tool-output` `9687d644b`: code that reads another tool's output is
  tested against captured output, with a register of readers and fixtures and its check C104) covers the fixtures:
  `runlog` is one of its readers and its captured logs are 0090 fixtures. This ADR adds only the rule that partial input
  is `NotMeasured`; the two are not merged.

### 5. A check over every instance: C103 (next free id: C102 was the highest in use, and ADR 0090 reserves C104)

In RT-5's `./dev check` host:

- **(a) Early exits in tests.** Inside a test function's body (`#[test]`, `#[tokio::test]`, any `#[…::test]`), not
  inside a closure or nested fn: every `return` token that is not the last statement, print or not: a bare `return`,
  `let … else { return }`, `if … { return }`, `match … => return`. The one allowed exit is through `measure`, which needs
  no `return`. A syn pass (as ADR 0076's C42). JS: `skip:` options and `test.skip(cond)`/`t.skip()` whose condition
  probes a tool or variable, outside `measure`. Fixtures include the `d055bcf31` form (print in a helper, bare
  `return` in the test) and the `fb4c579fa^` let-else without a print, verbatim.
- **(b) Local copies of the outcome, definitions only** (literals are too common: "not measured" is in 132 lines of 68
  files): in non-test tooling code outside `paddock_cli::outcome` and its JS mirror `conformance/src/outcome.mjs`, an
  enum variant named `NotMeasured`, `CheckBroken`, `Incomplete` or `NotApplicable`, a `const` whose name contains
  `NOT_MEASURED`, or a `fn complete|measured|is_measured(&self) -> bool`. JS shapes: an exported constant or an object
  key named `INCOMPLETE`, `NOT_MEASURED` or `CHECK_BROKEN` (vm-smoke's `EXIT = { …, INCOMPLETE: 3 }`), and the status
  literals `'not-measured'`, `'check-broken'`, `'not applicable'`. Seed at the heads above: 0 on the train's Rust;
  RT-3 `worktree/mod.rs:14`, RT-5 `record.rs:71, 73, 75, 99`, RT-8 `flakes.rs:154` (6 sites, 3 branches); the JS seed
  (vm-smoke's verdict and its rows) is counted by WP-3, which re-seeds every list from the heads it lands on.
- **(c) Exits.** `std::process::exit` in `tools/` only in `main.rs` (Clippy `disallowed-methods` in `tools/*/clippy.toml`).
- **(d) Tag workflows** follow §7's rule: the variables on every test step (the wide detector of §7), uploads from
  every leg, and a final `measured-on` job; `on.push.tags` read as YAML, actions followed recursively, with fixture
  workflows (§7). It is wiring coverage; the release guarantee is §4 rule 1's fail-closed check.

Today's (a), (b) and (d) sites seed a shrink-only ratchet (`docs/ratchets/c103.toml`, ADR 0088's engine); new sites fail.

### 6. Positive controls (ER-33), each a test

- `Verdict`: a property test over every combination of outcomes, failing and report-only findings, optional entries
  and run kinds: no combination with an undeclared `NotMeasured`, any `CheckBroken`, or an optional entry without green `measured_on` legs in a release
  run yields 0; **report-only findings plus `NotMeasured` yields 6**; failing findings plus `NotMeasured` yields 5 with
  both listed.
- Waivers: a planted crash and a planted half run with one finding, each under a matching `*` waiver, exit 7 and 6, in
  both hosts.
- **Fail closed on an unwired step:** in a release run (`PADDOCK_RUN_KIND=release`), a planted test with an
  `os`-only entry (no `measured_on`) and with `measured_on` but without `PADDOCK_NOT_MEASURED_LOG` (and again without
  `PADDOCK_LEG`) must each fail; with both variables and `measured_on` it passes and writes exactly one record. Run
  under nextest and `cargo test`, and the JS twin under node:test.
- `Measured` under nextest and `cargo test` in all three modes (A.2 is the prototype): a missing tool
  (`paddock-no-such-tool-0089`) fails strict, passes and records once in reporting mode, passes on a declared leg and
  fails on an undeclared one, and in a release run passes only with `measured_on` (a planted `measured_on` leg that did
  not run the test makes the final job fail; run on Debrief's tag path, §7); `require_tool("cargo")` returns a path (the probe sees a present tool).
- C103: every shape of §5 (a) and (b) in fixtures with a clean twin each; it must report every planted one.

### 7. CI and the strict default (rewritten in revision 5: a rule and a check, not a list)

CI does not use `./dev test`: it calls the runners directly. Under D4 (b) the strict outcome reads the register itself,
so each test step needs `PADDOCK_LEG=<id>` and `PADDOCK_NOT_MEASURED_LOG=<file>`, each leg uploads its reports, and one
final job reads them. Today nothing of that exists: `release-verdict` runs only in `dyno-`, `pitcrew-`, `pitwall-` and
`spotter-release.yml` (`:199`, `:157`, `:168`, `:155`) and in `nightly.yml` and reads no test results
(`conformance/ci/release-verdict.mjs` reports installers and failed gates); the only JUnit upload is Debrief's Linux
`rust-test` (`debrief.yml:225-230`).

Revisions 3 and 4 listed the tag workflows by hand and missed three (P1-8, P1-9), so the list is replaced by a rule
that a check enumerates:

**Rule.** Every workflow under `.gitea/workflows/` that has a tag trigger (`on.push.tags`) and whose steps, or the
steps of a workflow it calls (`uses: ./.gitea/workflows/…`), or of a local action those steps use
(`uses: ./.gitea/actions/…`, recursively: an action that uses another), run tests must. "Runs tests" is the detector
of C103 (d) below, wider than the three runner names:

1. set `PADDOCK_LEG` and `PADDOCK_NOT_MEASURED_LOG` on each of those steps (an action takes a `leg` input, default
   `${{ github.job }}-${{ runner.os }}`);
2. upload, `if: always()`, each leg's JUnit report and not-measured log under a name that carries the leg;
3. have a final job, `if: always()`, that needs every job running such a step, directly or through a called workflow,
   and runs `./dev ci measured-on` (Rust, ADR 0022; it applies §3a and is `NotMeasured`, never "nothing to check", when
   a leg's artifacts are missing). In the four app release workflows that job is the existing `verdict`, whose
   `release-verdict` step is extended to call it.

**Who is authoritative when a workflow is called.** `ci.yml` runs `measured-on` in its own final job only when it is
not an app's tag gate: `if: always() && inputs.app == ''` (its `workflow_call` input `app` is set by the
`<app>-release.yml` callers, `dyno-release.yml:73-77`). For a release caller, the caller's `verdict` job is the one
check; there is no second.

**Unverified:** whether Gitea's act_runner lets the caller's `verdict` job download artifacts uploaded by legs inside
the reusable `ci.yml` call (`actions/download-artifact` across a `workflow_call` boundary). WP-2b settles it first with
the Dyno control below; if it cannot, the called `ci.yml` runs `measured-on` itself for its legs (`if: always()`, its
`app` input set) and passes its outcome to the caller as a job output, which `verdict` reads.

**The check: C103 (d), wiring coverage.** The guarantee is §4 rule 1's fail-closed release rule; C103 (d) keeps the
wiring complete so that rule rarely fires. It parses each workflow as YAML and reads `on.push.tags` (not any `tags:`
key: `ci-image.yml:63` has a docker `tags:` input and no tag trigger, only `workflow_dispatch`), follows `uses:` into
called workflows and local actions **recursively** (composite actions inside composite actions), and reports every tag
workflow breaking the rule, by job and step.

**Detector: a step runs tests** when its `run:` (or a followed action's) invokes `cargo test` or `cargo nextest`,
`node --test`, `npm test`, `npm run <script>` (or `npm run <script> -w <ws>`) whose `package.json` script runs
`vitest`, `ng test`, `playwright` or `node --test` (resolved through the workspace's `package.json`, following
`npm run` chains), `ng test`, or `npx playwright test`/`npx vitest`. Today's steps the narrow detector missed:
`publish.yml:42` (`cargo test --doc`) and `:49-51` (`npm run test:tokens`, `npm test`, `npm run test:scripts`), the demo
releases' `:61-62` and `:75`, `debrief.yml:236` (`cargo test --doc`) and `:252` (`cargo test --release -p
debrief-store-duck --test analysis_memory`), `ci.yml:99` (`node --test`). **Doc tests** (`cargo test --doc`) may be
exempted per step with a reason in the check's register (they cannot return `Measured`), never silently.

Fixtures (each with a clean twin): a tag workflow with a nextest step and no `PADDOCK_LEG`; one with the variables but
no final job; one whose final job lacks `if: always()` or misses a test job in `needs:`; one calling a reusable
workflow that runs Playwright; one per detector form above (`cargo test`, `node --test`, `npm test`, an `npm run`
script that runs vitest, `ng test`, `npx playwright test`); a composite action nested inside another that runs
`cargo nextest`; a workflow with a docker `tags:` input and no tag trigger (must not be selected). The seed is
enumerated by the check, not typed here; at `67a633ef3` a scan finds 9 tag workflows that run tests (every one breaks
the rule today); `ci-image.yml` has no tag trigger:

| Workflow (tag) | Test steps found at `67a633ef3` | Final job |
|---|---|---|
| `debrief.yml` (`debrief-v*`) | `rust-test` `:223`, `rust-test-macos` `:330` (nextest), its e2e and frontend jobs (Playwright, vitest) | **new** `measured-on` job |
| `dyno-`, `pitcrew-`, `pitwall-`, `spotter-release.yml` | through `gates` → `ci.yml` (`app` job → `app-gates`, `action.yml:153-154`) | existing `verdict`, extended |
| `shell-demo-release.yml` (`shell-demo-v*`) | `gates`: Playwright `:67`, nextest `:74` | **new** final job |
| `workbench-demo-release.yml` (`workbench-demo-v*`) | `gates`: Playwright `:67`, nextest `:74` | **new** final job |
| `publish.yml` (`v*`, the shared packages every app ships) | `gates`: nextest `:41` | a `measured-on` step at the start of the `publish` job (it already `needs: [gates]`), before anything is published |
| `ci.yml` (`v*`) | `wiki` `:478`, `heavy` `:575`, `crate` `:835-857`, `paddock-dev` `:1186`, `app` (through `app-gates`) | **new** final job, skipped when `inputs.app` is set |

**Positive controls** (§6), on throw-away branches with the tag path's inputs: (1) `debrief.yml`, a planted test
`NotMeasured` on `rust-test` (Linux) with `measured_on = ["rust-test-macos"]`: green when macOS passes it, red when it is
skipped there too; (2) `dyno-release.yml` with gates on, a planted test in a Dyno crate `NotMeasured` on the Linux
`app` leg with `measured_on = ["<Dyno's macOS app leg>"]`, the same two outcomes, which also proves (or disproves) the
artifact download across the `ci.yml` call; (3) a rule 54 dispatch of `dyno-release.yml` (`workflow_dispatch`, gates
on, a branch ref, so only `inputs.release` marks it): the planted test inside the called `ci.yml` must see
`PADDOCK_RUN_KIND=release` (an `os`-only entry fails there, as in a tag run); (4) C103 (d)'s fixtures.

This touches shared actions and every app's release path (ER-31 (b), ADR 0022 G.7), so WP-2b lands outside a release
freeze, **before** WP-4 and WP-5: otherwise migrated tests turn legs red where a declared exception was intended.

### 8. Migration (after acceptance; one small commit per group, C103 green as the proof)

1. **Open defect now, failing test first:** the JS host's `runner.mjs:24` (crash unwaivable), listed in RT-5's shadow
   table. (RT-5's scope fix is in progress in its branch; RT-8's interim exit 1 stands.)
2. WP-1 (type, `Exit` 6 and 7), WP-2 (`Measured`, `require_*`, JS twin, `./dev test`), WP-2b (CI legs), WP-3 (C103).
3. **WP-5 step 1, measured before any migration:** run each Rust test leg once in reporting mode and list, per leg, what
   is not measured; today's evidence says at least ffmpeg (absent in the image), `walk.rs:308` (the image runs as uid
   0) and, unverified, landlock and seccomp in Docker, elevated ETW and csc.exe on the Windows runner. Each item gets the
   tool in the image or an `[[optional]]` entry in the same change; WP-5 is re-sized from that list.
4. WP-4: tooling copies onto the type (RT-5 record and `measured()`, RT-3's const, RT-8 to 6, RT-2 notices, C100 strings,
   golden.rs `skip()` to an optional entry for host legs, the four `*_REQUIRE_TEST_BROWSER` switches, the JS host's
   `unwaivable` to `outcome.mjs`); vm-smoke with its port.
5. WP-5: the test sites by kind, app by app.
6. Register `outcome` in `docs/known-copies.toml` (canonical `crates/paddock-cli/src/outcome.rs`; the JS mirror a
   declared copy until RT-17's last batch).

### 9. Relations

- **ER-33 / rule 71.** This is its mechanism; D11 proposes the text.
- **ADR 0088.** Same principle one level up (entries) and here (runs). Both read RT-5's `RunRecord`; 0088's coverage and
  this ADR's outcome are one function on it. `docs/optional-measurements.toml` and C103's ratchet are lists of its engine;
  release runs are its §3 set.
- **RT plan.** §3's positive controls become §6's rule; every brief from RT-9 on says: a check that cannot run returns
  `Outcome::NotMeasured`; tests return `Measured`; no early `return` in tests; no `notices` for things not measured.
  RT-22 re-exports; nothing waits for it.
- **ADR 0038 (push gate).** Its table is the `Verdict` table, with reporting mode for tests; `ci-only` declarations
  count as optional for it. The G26 stamp is written for exit 0 only, which includes a run whose only gaps are declared
  optional; the stamp records the optional entries used, so dispatch and review can see them.
- **ER-12.** Strict `Measured` is "never silently skip a test" made mechanical.

### 10. Release impact (ER-31)

`paddock-cli` is in Dyno's and Pitwall's shipped dependency sets through `paddock-service` (which uses `Exit::Ok`,
`Exit::NotRunning`, `Exit::Usage`, `CliError` and `Help`). WP-1 only adds a module and two variants; nothing shipped
matches on `Exit` exhaustively outside the crate (`git grep` for `Exit::ALL` and match arms). It is still shared code on
their release path, so it lands outside a release freeze, as WP-2b does. The rest is tooling and tests.

## Parallelism and cost (rule 51)

`Verdict` folds a few hundred items. `require_tool` spawns `<tool> --version` once per test, replacing probes the tests
already make. The register is parsed once per test process (a `OnceLock`). C103 (a) is one syn parse per test file in
the RT-5 host's existing walk. No timing is quoted as evidence (rule 48); WP-3 records C103's cost in
`docs/benchmarks.md`.

## Alternatives

- **Fix each instance and add a checklist line.** Rejected: five packages in one week repeated the class, and RT-3's
  fix round replaced an `#[ignore]` with an `eprintln!` and `return`, the same class in a quieter form.
- **Every CI test step through `./dev test --leg <id>` (D4 (a)).** Possible, and it gives one table per job; rejected
  as the first step because it changes every test command in `ci.yml`, the app workflows and the shared actions on the
  release path, where (b) changes one environment line and a summary step.
- **Lenient by default locally, strict when `CI` is set.** Rejected: a passing test's output is hidden (measured), so
  local runs would be silently green.
- **A custom harness with a real skip (`libtest-mimic`).** Rejected: one harness per test target in nine workspaces;
  `Termination` gives the same control with the standard harness.
- **`#[ignore]` for tests that need a tool.** Rejected: no CI line runs ignored tests (incident #2's first form).
- **One exit code for both states.** Rejected (D3 (b)): different owners and next steps (ER-27).
- **Literal scan for C103 (b).** Rejected: 132 lines in 68 files, most of them correct messages.

## Consequences

- A developer without `ffmpeg`, `jq` or a landlock kernel sees failing tests under plain `cargo test`; the message says
  what to install or how to list it as a skip.
- A test leg missing a tool turns red after migration unless the register declares it; WP-5's step 1 finds those first.
- `docs/optional-measurements.toml` becomes the one reviewed place that says what is not measured where.
- Reviews stop hunting this class by hand.

## Rollback

An `[[optional]]` line declares a measurement optional per place; C103 can be report-only in `docs/ratchets.toml`
(ADR 0088). Reverting WP-1 is safe once the tools stop returning 6 and 7.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Where the outcome type lives | (a) `paddock_cli::outcome` now, re-exported by `paddock-tool`; (b) `paddock-dev`'s core, moved by RT-22; (c) wait for RT-22 | **(a)** |
| D2 | The states | (a) five: `Clean`, `NotApplicable` (scoped runs only, as RT-5's fix), `Findings`, `NotMeasured`, `CheckBroken`; (b) four, "out of scope" as "not selected" | **(a)**: matches RT-5 |
| D3 | Exit codes | (a) `NotMeasured` 6, `CheckBroken` 7; precedence 7, then failing findings 5, then 6, then 0; (b) one code for both | **(a)** |
| D4 | How CI applies optional entries | (a) every CI test step through `./dev test --leg <id>`; (b) the strict `Measured` reads the register and `PADDOCK_LEG`, plus a summary step (WP-2b) | **(b)** now; (a) can follow with RT-12 |
| D5 | Report-only findings and not measured | (a) never masked: exit 6 (or 7), the property test covers it; (b) report-only runs exit 0 regardless | **(a)** |
| D6 | Where optional is declared | (a) `docs/optional-measurements.toml`, an ADR 0088 register (readers, coverage, uncovered = not measured); never for `CheckBroken`; in a release run only under D14; `ci-only` counts for the push gate; (b) at the call site | **(a)** |
| D7 | The check | (a) C103: every early exit in a test body, print or not; outcome definitions outside the canonical module; `process::exit` outside `main.rs`; tag workflows that run tests without §7's variables, uploads and final job; shrink-only ratchet; (b) Clippy only | **(a)** |
| D8 | The JS host's waivable crash | (a) fix now, failing test first, shadow-table entry; (b) with WP-4 | **(a)** |
| D9 | ffmpeg in CI (measured absent in `paddock-ci:v2`) | (a) add it to the image; (b) an `[[optional]]` entry for the Linux container leg; (c) both, (b) expiring at the image date | **(c)** |
| D10 | The 91 `#[ignore]` tests | (a) out of scope; a later check that each names the job that runs it; (b) now | **(a)** |
| D11 | Engineering rule | (a) extend ER-33: "… and a check, probe or test that could not look reports it as its own outcome and exit code, which no waiver, baseline, report-only finding, aggregation or test harness turns into a pass; a measurement may be skipped only where reviewed data declares it optional for that place, with a reason and an expiry, and in a release run only where another leg of the same run measured it"; (b) a new ER-37 | **(a)**, as ADR 0088 D11 |
| D12 | Tag gating | (a) no new gate on tags beyond D14's rule; WP-1 and WP-2b land outside a release freeze, and WP-5 migrates an app's tests only with its `measured_on` entries in the same change; (b) block tags until WP-5 | **(a)** |
| D14 | Optional entries in release runs (P1-7) | (a) honoured only with `measured_on = [legs]` and one of those legs ran the test green in the same run, checked across legs by a final job of each tag workflow (§7's rule, enforced by C103 (d)); (b) release strictness only for ER-33 release checks (vm-smoke, release isolation, installer, gates), `where` entries for unit tests honoured as on any run; (c) stay strict, and first make the image measure everything (a non-root test user, a landlock-capable runner) in a WP before WP-5 | **(a)**: a test may be measured on another leg but never vanishes from a release; (c)'s image work stays worth doing and removes entries as it lands; (b) is the fallback if `release-verdict` cannot read the legs' reports |
| D13 | Reading CI logs for test results | (a) one reader (`runlog`, §4b) for nextest, Playwright and vitest, `Measured` only with the runner's summary, truncation controls from real logs; (b) each tool keeps its own | **(a)**: two nextest readers already drifted (#6) |

## Work packages (raw agent-days, one agent), in order

| WP | Content | Days |
|---|---|---|
| WP-0 | The JS host's crash (§8.1) with the planted-waiver test first | 0.25 |
| WP-1 | `paddock_cli::outcome` (`Outcome`, `Verdict`, `Optional` reader with run kinds, JSON and table), `Exit` 6 and 7, contract and property tests (D5's case included), 99% coverage; `conformance/src/outcome.mjs` | 1.5 |
| WP-2 | `paddock_testkit::{Measured, measure, require_tool, require_tool_that, require_capability, require_env}` with the three modes and the record format; the JS `measure`/`requireTool`; `./dev test` (nextest plus records into a `Verdict`); §6's controls under nextest and `cargo test`; the fail-closed release rule (§4 rule 1) and its control | 2.75 |
| WP-2b | §7's rule on every tag workflow that runs tests (9 at `67a633ef3`): the variables on each test step, uploads from every leg, `PADDOCK_RUN_KIND` in the release workflows and in `ci.yml` from its `release` input, `./dev ci measured-on`, new final jobs in `debrief.yml`, both demo release workflows and `ci.yml` (skipped when `inputs.app` is set), a step in `publish.yml`'s `publish` job, `release-verdict` extended; settle the artifact download across the `ci.yml` call; the Debrief and Dyno controls | 2.5 |
| WP-2c | §4b's reader: the Playwright and vitest parts (nextest by RT-8, not counted here), the truncation controls over every captured log, a vitest fixture, `plan_since` and `ci flakes` as callers, the copies entry | 1.5 |
| WP-3 | C103 (a) syn pass, (b), (c), (d) (the tag-workflow pass of §7: YAML `on.push.tags`, recursive actions, the wide detector with `package.json` script resolution, doc-test exemptions, fixture workflows), fixtures for every shape, the ratchet seed; `docs/optional-measurements.toml` with schema and its 0088 list entry; benchmark line | 3.5 |
| WP-4 | Tooling copies onto the type (§8.4) with the shadow-table entries | 2 |
| WP-5 | Step 1: one reporting-mode run per Rust test leg, the per-leg list, the image line for ffmpeg (D9) and the entries; step 2: the 126 test-body sites and 2 JS sites by kind, per app. Re-sized after step 1 | 3 (1 + 2, provisional) |
| WP-6 | `docs/known-copies.toml` entry, ER-33 text (D11), rule 71 line, lessons entry, RT brief template line | 0.5 |
| | **Total** | **17.5** |

First cut: WP-0 now (independent of acceptance), then WP-1, WP-2, WP-2b, WP-2c and WP-3 (11.75 days) close the class for new
code; WP-4 and WP-5 retire today's copies.

## Review answers (revision 5, `7680e9d79`)

- P1-10: §4 rule 1 fails closed in a release run (both variables set and the record written, else a strict
  failure), stated as the primary guarantee, with a §6 control; C103 (d) is wiring coverage with a wide detector
  (`cargo test`/`nextest`, `node --test`, `npm test`, `npm run` scripts resolved through `package.json`, `ng test`,
  `npx playwright`), each with a fixture, and doc-test exemptions with reasons.
- P2: `ci-image.yml` has no tag trigger (its `tags:` is a docker input); the check reads `on.push.tags` as YAML;
  actions are followed recursively with a nested-composite fixture; rule 54 dispatches are release runs, Debrief's
  bundles dispatch and nightly's `app-release.yml` calls are not.

## Review answers (revision 4, `264468b3c`)

- P1-9: the hand-made workflow table (which missed `shell-demo-release.yml:74`, `workbench-demo-release.yml:74` and
  `publish.yml:41`, after revision 3 had missed Debrief's tag path) is replaced by §7's rule and the check C103 (d) with
  fixture workflows; the three are added (a final job per demo, a step at the start of `publish.yml`'s `publish` job);
  WP-2b 2.5 days, WP-3 3 days.
- P2: Debrief's count is the two lines `:223`, `:330`; the artifact download across the `ci.yml` call is marked
  unverified, with a fallback, and the Dyno control settles it; `ci.yml`'s own final job skips when `inputs.app` is set,
  so the release caller's `verdict` is the one check.

## Review answers (revision 3, `197d5234e`)

- P1-8: §7 rewritten per workflow that runs a tag's tests, naming the jobs (Debrief's `rust-test` and `rust-test-macos`
  and a new final job; the other apps' `ci.yml` `app` legs through `app-gates` and their existing `verdict` job with
  `release-verdict` extended to read test reports; `ci.yml`'s four direct nextest jobs and a new final job), JUnit and log
  uploads from every leg, `./dev ci measured-on`, the §6 control on Debrief's tag path; WP-2b 2 days.
- P2: the C103 (b) seed is 6 sites; `logfind.rs` becomes `runlog` when §4b lands.

## Review answers (revision 2, `90d28a0a6`)

- P1-7 release runs: §3a and D14 (recommended (a)), D12 restated, WP-2b +0.5 day.
- RT-5 drifts: `judge` named as RT-5 names it; crash stays `CheckBroken`, changed in RT-5 by WP-4 with a shadow-table
  line; the "being fixed" text replaced by `0a94bf347`'s state; the C103 (b) seed updated and re-seeded by WP-3.
- A: `test:` entries are expiry-only; their records are CI artifacts read by the tag workflow's final job (§7). B: run kind from
  `GITHUB_REF` and an explicit `PADDOCK_RUN_KIND`; `PADDOCK_LEG` set on every nextest step (revision 5: §7's rule).
  C: start lines must equal summaries; a two-invocation fixture. D: JS shapes in C103 (b). ADR 0090 cross-referenced.

## Review answers (revision 1, `e5e4f300d`)

- Added from the RT-8 review of `5da33f8cf`: incident #6, copy 12 and §4b (one runner-log reader with truncation controls).

- P1-1 golden's notice: corrected (it reaches CI's log through `success-output = "final"`, as a notice in a green run).
- P1-2 the test helper: `Measured` implements `Termination`; `unwrap_measured` dropped; prototype measured under both
  runners and modes (A.2); §6 control added.
- P1-3 C103 (a): now every early exit in a test body, print or not, with both commits' forms as fixtures; seed recounted.
- P1-4 CI does not use `./dev test`: D4 and WP-2b, sequenced before WP-4 and WP-5; total re-estimated.
- P1-5 stale defects: RT-5 `8550764e7` and RT-8 `5da33f8cf` described as they are; `NotApplicable` aligned with RT-5;
  RT-8's interim exit 1 and its move to 6; `06adcb058` cited; the JS crash kept open.
- P1-6 report-only masking: §2 rule 3, D5, and the property test.
- P2s: retries per config; ffmpeg and uid 0 measured in the image; WP-5 step 1 measured per leg; C103 (b) definitions
  only with its seed; the register's readers and coverage; release runs as 0088 §3; the JS CLI's exits; exact commands
  (Appendix B); the capture test inlined (A.1); the README blank line; G26; paddock-service's uses.

## Appendix A: measurements (synthetic, 2026-11-10)

**A.1 Print-then-return under both runners.** One crate, one test:

```rust
#[test]
fn needs_a_tool() {
    if std::process::Command::new("no-such-tool-xyz").arg("--version").output().is_err() {
        eprintln!("SKIPPED-MARKER: no-such-tool-xyz is missing");
        return;
    }
    panic!("would have measured");
}
```

`cargo test 2>&1 | grep -c SKIPPED-MARKER` → 0, result `ok`; `cargo nextest run 2>&1 | grep -c SKIPPED-MARKER` → 0,
`Summary … 1 test run: 1 passed, 0 skipped`.

**A.2 A `Termination` test outcome.** A prototype `enum Measured { Yes, NotMeasured(String) }` whose `report()` returns
`ExitCode::FAILURE` (printing `not measured: …`) unless `PADDOCK_NOT_MEASURED=report:<file>` is set, in which case it
appends one JSON line and returns `ExitCode::SUCCESS`; two tests, one needing `paddock-no-such-tool-0089`, one `cargo`.
Strict under nextest: `FAIL … needs_a_missing_tool` with `not measured: paddock-no-such-tool-0089 is missing (No such
file or directory (os error 2))`, `PASS … needs_a_present_tool`, `2 tests run: 1 passed, 1 failed`. Reporting mode
under nextest: `2 tests run: 2 passed`, one record written. Strict under `cargo test`: `1 passed; 1 failed`.

**A.3 JS host waiver.** `applyWaivers` with finding `{check: 'C53', file: '', message: 'the check crashed: boom'}` and
a waiver `check = "C53"`, `path = "*"`, expiry 2099 → `findings: []`, `waived: ['the check crashed: boom']`.

**A.4 CI image.** `docker run --rm --entrypoint sh dev.decryptology.net/decryp7/paddock-ci:v2 -c 'command -v ffmpeg ||
echo NO-FFMPEG; id -u; command -v jq'` → `NO-FFMPEG`, `0`, `/usr/bin/jq` (the local copy, created 2026-10-24, run under
amd64 emulation; that the runners use the same digest is **unverified**).

## Appendix B: the commands behind the counts (at `67a633ef3` unless a ref is named)

- Gates strings: `git grep -c -i -E 'not measured|check broken' -- tools/paddock-dev/src/gates` → 12, 3, 2, 1 (18).
- RT-2: `git grep -i -E 'not measured|check broken' origin/feat/rt2-release-isolation -- tools/paddock-dev/src/release_isolation | wc -l` → 40.
- All "not measured" lines: `git grep -i 'not measured' -- '*.rs' '*.mjs' '*.ts' '*.sh' '*.ps1' ':!**/node_modules/**'` → 132 lines, 68 files.
- Print skips: `git grep -i -E '(e?println!|eprint!).*(skip|not measured|not available|not found|missing|not installed|unavailable)' -- '*.rs' ':!apps/dyno/vendor/**'` → 47 lines, 30 files.
- Test-body early exits: a token scan over `git ls-files '*.rs'` (vendor excluded) that strips comments and strings,
  finds each `#[…test…]` fn body by brace matching, and counts `return` tokens whose enclosing braces are not a closure
  (`|…|`), nested `fn` or `async` block → 126 in 71 files (37 let-else, 25 if-guard, 64 other). An approximation; WP-3's
  syn pass gives the exact seed.
- JS skips: `git grep -E '\{ *skip *:|t\.skip\(|test\.skip\(|it\.skip\(|describe\.skip\(' -- '*.test.mjs' '*.spec.ts' '*.e2e.ts'` → 45.
- `#[ignore]`: `git grep -E '#\[ignore' -- '*.rs' ':!apps/dyno/vendor/**'` → 91 lines, 44 files; with `VM` in the
  attribute: 33.
- Ignored tests run in CI: `git grep -E 'run-ignored|--ignored|--include-ignored' -- .gitea apps/*/paddock.ci.json '*nextest.toml'` → none.
- Playwright: `grep -E 'retries|failOnFlakyTests'` over the 10 `playwright*.config.ts` files.
