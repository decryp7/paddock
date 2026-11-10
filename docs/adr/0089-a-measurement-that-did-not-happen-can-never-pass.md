# 89. A measurement that did not happen can never pass: one outcome type, one test helper, one exit code, and a check over every copy

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10 (ER-21: one class, seen five times in
  one day's Rust tooling reviews, after the Dyno 1.1.0 instances that created ER-33). Nothing here is built yet. Needs an
  independent review before the user decides (ER-23 does not apply: no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42 and 55 (share first; one
  implementation per mechanism, `docs/known-copies.toml`), ER-12 (no silent skips, no retry that turns red green), ER-27
  (logs and exit codes explain a failure), ER-31, ER-33 / rule 71 (a check counts only if its precondition was verified
  and its probe has shown it can see the thing); ADR 0022 and its 2026-10-31 amendment (`paddock-cli`'s `Exit`, the shared
  runtime `paddock-tool`, G1), ADR 0038 (CI parity before push, its table of what was skipped and the G26 stamp), ADR 0088
  (accepted 2026-11-10 on `origin/docs/adr-windows-gate-ratchets`: "not measured, never stale and never fine" for list
  entries, the RT-5 run record); the plan `docs/plans/rust-tooling-2026-11-10.md` on `origin/docs/rust-tooling-plan`
  (§3 "positive controls instead", RT-5 check host, RT-12 push gate, RT-17 conformance remainder, RT-22 `paddock-tool`);
  lesson "A check that could not see the thing passed, and a wrong cause was called confirmed" (`docs/lessons.md:1438`).
- Number: 0089 was free on every `origin` branch on 2026-11-10 (highest file is `0088` on
  `origin/docs/adr-windows-gate-ratchets`; no branch's `docs/` mentions "ADR 0089").
- Evidence: read at `origin/train/2026-11-09-3` `67a633ef3` and the branch heads named in the table, on 2026-11-10. Two
  measurements on synthetic input in the researcher's scratch folder (no user data): a throw-away crate with one
  print-then-return test under `cargo test` and `cargo nextest run` (cargo 1.99.1, nextest 0.9.151), and the JS host's
  `applyWaivers` on a planted crash finding. Searches are `git grep` over the named refs; counts exclude
  `apps/dyno/vendor/`.

## Context: the incidents

Five review findings on 2026-11-10, each in a different Rust tooling package, and each the same shape:

| # | Branch, commit | What did not measure | How it read as a pass |
|---|---|---|---|
| 1 | RT-2 release isolation, `origin/feat/rt2-release-isolation`; before `e59463f8f` (`93cdaf957`) | no `--tauri-pins` file: the Tauri tool folder's content was compared with nothing | the line went to `Verdict::notices` ("Things that were not measured and are not faults") and the JSON said `"passed": true` |
| 2 | RT-3 `./dev worktree`, `origin/feat/rt3-dev-worktree`; `fdb4c3e44` then `d055bcf31`, fixed in `2be92ec01` | the `worktree.sh` oracle: `WORKTREE_SH` unset | first `#[ignore]` (never run: no CI line runs ignored tests), then `oracle()` printed `oracle not measured` with `eprintln!` and the test returned; the doc said "CI sets it", and no file under `.gitea/` names `WORKTREE_SH` |
| 3 | RT-4 hook, `origin/feat/rt4-hook`; before `fb4c579fa` | the live shell-hook oracle: `bash` or `jq` missing | `None => return eprintln!("{script}: bash or jq is missing, the live oracle did not run")` and `let Some(denies) = old_denies(..) else { return }` |
| 4 | RT-8 `./dev ci flakes`, `origin/feat/rt8-ci-flakes` `dff738952` (open) | (a) a job log that could not be read; (b) Playwright's "flaky" marker in apps that do not set `failOnFlakyTests` | (a) `ci_runs/mod.rs:90` maps an incomplete report to `Exit::Findings` (5), the code of "measured, and found something", so a script cannot tell them apart; (b) only Debrief's `playwright.config.ts` sets `retries` and `failOnFlakyTests` (9 of the 10 Playwright configs do not), so that probe cannot see a flaky test in the other apps, and the report still says `complete` |
| 5 | RT-5 check host, `origin/feat/rt5-check-host` `40b7faf8f` (open) | a check that did not complete, or crashed | `check/mod.rs:295` adds the unwaivable "not measured" finding only `if !record.completed && found.is_empty()`: a check that stopped half-way but had found one waivable finding gets none, so a waiver for that finding turns the run clean; the crash finding (`mod.rs:276-283`) is an ordinary `Finding::error`, so a waiver covers it |

And the older ones that created ER-33 (Dyno 1.1.0, `docs/lessons.md:1438`): a VM with no menu bar "confirmed" a tray cause in
12 of 12 runs, a tray count matched the app's own window buttons, a reading row passed on a dead helper, and
`renderResult` printed PASS on zero steps.

Two more found while counting instances (not in any review):

- **The JS conformance host does what RT-5 copied.** `conformance/src/runner.mjs:24` turns a crash into an ordinary
  `error`, not `unwaivable`. Measured: `applyWaivers` with a planted `the check crashed: boom` finding for C53 and a
  `path = "*"` C53 waiver returns it under `waived` and nothing under `findings`. No `path = "*"` waiver exists today, so
  this is at risk, not broken. RT-5's shadow run uses this host as its oracle (plan §3), so the oracle agreed with the bug.
- **RT-5 derives the status twice, and the two disagree.** `check/record.rs` `RunRecord::status` says `NotMeasured`
  whenever `probes == 0`; `mod.rs:297` raises the error only when `scope == Scope::Root`. A `--files` run that reaches no
  file of a check prints `"status": "not measured"` in the record and exits clean (read, not run).

**Why the print-then-return form is silent, measured.** A test that prints `SKIPPED-MARKER` with `eprintln!` and returns:
`cargo test` printed the marker 0 times and reported the test passed; `cargo nextest run` printed it 0 times and its
summary said `1 test run: 1 passed, 0 skipped`. Both capture the output of passing tests (no nextest profile in the repo
sets `success-output`). libtest has no runtime skip: the proposals for one were not accepted
([pre-RFC "Skippable tests"](https://internals.rust-lang.org/t/pre-rfc-skippable-tests/14611),
[a later thread](https://internals.rust-lang.org/t/skippable-tests/21260); read 2026-11-10, status of any later RFC
**unverified**). So in Rust, "skip and say so" from inside a test is a pass that nobody sees, and `tools/paddock-dev/tests/it/golden.rs:29`'s
`::notice::`, printed from a passing test, never reaches the job log either.

## Pattern

**A check, probe or test that could not look reports in a form some caller reads as a pass**: a notice, a warning, an
`eprintln!` and `return`, an ignored test, a waivable finding, a bool, or an exit code shared with another meaning. Each
tool decides on its own how to say "not measured", in a string, and the strings get lost at the first conversion (to a
bool, an exit code, a waiver match, a "complete" flag).

**Invariant.** "Did not measure" and "the probe is broken" are values of their own, distinct from "clean" and from
"findings", from the probe up to the process exit code; no step on the way (waivers, baselines, aggregation, JSON,
exit codes, test harness) can turn either into a pass. The only way a run with one passes is a declaration, in reviewed
data, that this measurement is optional in this place, with a reason and an expiry.

## Instances (2026-11-10)

**Ways to say "not measured" in tooling** (each a copy of the mechanism, rule 55):

| # | Where | Representation | State |
|---|---|---|---|
| 1 | `tools/paddock-dev/src/gates/` (C100, train) | "not measured: …" / "check broken: …" text in error findings (`evaluate.rs` 12 sites, `register.rs` 3, `gating.rs` 2, `edge.rs` 1) | fine (errors), stringly |
| 2 | `release_isolation/` (RT-2) | `Leak`/`Probe` enums, `Finding::is_broken`, `notices: Vec<String>` (40 lines name it) | fixed by `e59463f8f`; local trial still a notice, by flag, refused on a tag (fine) |
| 3 | `worktree/primary.rs` (RT-3) | `"not_measured_only"` JSON flag and a local `worktree::EXIT_NOT_MEASURED` = 6 (`cli.rs`) | fine, but a private exit code |
| 4 | `check/record.rs` (RT-5) | `Status { Clean, Findings, NotMeasured, CheckBroken }` plus a second derivation in `mod.rs:287-299` | **broken** (#5, and the scope mismatch) |
| 5 | `ci_runs/flakes.rs` (RT-8) | `complete() -> bool`, mapped to `Exit::Findings` | **broken** (#4) |
| 6 | `hook/oracle_tests.rs` (RT-4) | `panic!("the live oracle is not measured: …")` | fixed by `fb4c579fa` (fine) |
| 7 | `tests/it/golden.rs` (train) | `skip(reason)`: panic in CI on Linux, `::notice::` on CI host legs, `eprintln!` locally | at risk: the notice and the local line are captured (measured above) |
| 8 | `conformance/src/finding.mjs`, `util/waivers.mjs`, `runner.mjs` | `unwaivable` flag by convention; crash not unwaivable; CLI exit 0 clean, 1 findings | **at risk** (crash waivable, measured) |
| 9 | `conformance/src/checks/C99-no-keychain.mjs` | unwaivable errors, and two "not measured" **warnings** (`--no-graph`, line 247; no clippy.toml, line 305) | fine by declaration (`--no-graph` never in CI; the demos' deviation recorded in ADR 0057 §3.5) |
| 10 | `scripts/vm-smoke/verdict.mjs` | status strings `not-measured`, `check-broken`; exit 3 = INCOMPLETE | fine, but 3 is `paddock-cli`'s `NotRunning` |
| 11 | `conformance/release/check-per-machine-installer.mjs` | "not measured" text in problems | fine (fails) |

**Tests that skip by printing and returning** (`git grep` for a print with "skip", "not found", "missing" or "not available"
in `.rs`; 47 sites in 30 files, classified by their message, not each read):

| Kind | Sites | Examples | Right form |
|---|---|---|---|
| a tool or capability is missing | 26 | `debrief-video/tests/it/real_ffmpeg.rs:45` and `src-tauri/src/video/tests.rs:546` (ffmpeg: `ci-image/Dockerfile` installs none, so in the Linux CI container these always "pass"; macOS and Windows runners **not measured**), `debrief-ipc/tests/it/launch.rs` (landlock, seccomp: 4), `storage/walk.rs` and `paths.rs` (links, undeletable files: 5), `dotnet_win_tests.rs` (csc.exe: 3), `pitwall-server/tests/it/e2e.rs:510` (sparse file), ETW tests that need a non-elevated shell (3), `golden.rs` (node, a full clone: 3) | `require_tool` / `require_capability` |
| opt-in by a variable (VM, benchmark, corpus, real browser, elevated ETW) | 17 | `PADDOCK_VM` (5), measurements `DEBRIEF_BENCH_*`, `DEBRIEF_READY_*`, `DEBRIEF_EVTX_MEASURE_MB`, `DEBRIEF_PLUGIN_BASELINE` (5), `DEBRIEF_ETW_TESTS` (2), `CHROMIUM_TEST_PATH` (2), `DEBRIEF_EVTX_CORPUS`, `PITWALL_LLAMA_E2E`, `PADDOCK_REAL_WINDOWS` | `#[ignore = "<who runs it>"]` plus `require_env` inside, so a job that opts in and misspells the variable fails |
| wrong platform at run time | 1 | `eventlog_live.rs:12` ("exists only on Windows") | `#[cfg(...)]`, not a run-time skip |
| a race the test lost | 1 | `src-tauri/src/live.rs:772` ("passed between two refreshes") | a fake clock (ER-12); a bug of its own, out of this ADR's migration |
| other (no PDB, plugin absent) | 2 | `pdb_symbols.rs:285`, `dumps.rs:913` | `require_*` |

JS tests skip through the runner (`{ skip: … }`, `test.skip`: 45 sites), which **is** visible (node's test runner and
Playwright count skipped tests), but CI does not fail on a skip, so a tool missing from the image passes CI the same way:
`c99-clippy-fixture.test.mjs:33` (`hasClippy`), `c99-graph.test.mjs:330` (`skipCargo`).

There are also 91 `#[ignore]` tests in 44 files and no CI line runs ignored tests (`git grep` for `run-ignored`,
`--ignored` and `--include-ignored` under `.gitea/`, the `paddock.ci.json` files and the nextest configs finds none). They
are visible as "ignored" and most are declared opt-ins (27 name the Windows setup VM); this ADR leaves them alone (D9).

## Decision

### 1. One outcome type, in `paddock-cli`

`paddock_cli::outcome` (a module of the crate that already owns `Exit` and `Report`; `paddock-tool` re-exports it when
RT-22 builds the runtime):

```rust
#[must_use]
pub enum Outcome {
    Clean,                       // measured, nothing found
    Findings(usize),             // measured, found something (after waivers and baselines)
    NotMeasured(Reason),         // did not look: tool missing, precondition false, incomplete, zero probes
    CheckBroken(Reason),         // looked, but its positive control failed, or it crashed
}
pub struct Reason { pub what: String, pub why: String, pub next_step: String }
```

Rules the type enforces, not a reminder:

- **No conversion to a pass.** No `bool` accessor, no `From<Outcome> for bool`/`Result`, no `Default`, no `is_ok`. The
  only exits are `Verdict::exit() -> Exit` and `Verdict::json()`.
- **Waivers and baselines see findings only.** A waiver engine takes `Vec<Finding>` and returns fewer; it never receives
  an `Outcome`. The outcome of a check is computed **once**, from its run record and the findings *after* waivers:
  `RunRecord::outcome(&self, findings_after_waivers)`, replacing both of RT-5's derivations. So a waiver cannot cancel
  `NotMeasured` (incident #5), and a crash is `CheckBroken`, not a finding (#5, and the JS host).
- **Zero probes on a scoped run** is `NotMeasured` only when the scope included something the check covers; a `--files`
  run that lists no file a check reads does not select the check (the record says "not selected: out of scope"), so the
  two derivations cannot disagree again.

### 2. Aggregation: any not-measured makes the run not pass, unless declared optional

`Verdict::of(outcomes, &Optional)` folds the outcomes of a run. The result is the worst, in this order:
`CheckBroken` > `Findings` > `NotMeasured` > `Clean`, with every item kept in the JSON (`outcomes: [{id, outcome, reason}]`)
and in a text table. An item is downgraded to **"not measured, declared optional"** (shown, not failing) only when
`docs/optional-measurements.toml` has an entry for it:

```toml
[[optional]]
id = "test:debrief-video::real_ffmpeg"     # or "check:C99/graph", "gate:windows-tests", "probe:release-isolation/tauri-tools"
where = { os = ["windows"], leg = "host" } # the places it is optional; elsewhere it still fails
reason = "no ffmpeg on the Windows runner; the Linux leg runs it"
expires = "2027-03-31"
```

The file is a **register** in ADR 0088's sense: entries carry reason and expiry (an expired entry is an error, as `W001`),
and an entry no run used is stale. Never optional: a tag run (`GITHUB_REF` under `refs/tags/`) for anything on the
release path (ER-33's "the release does not pass"), and `CheckBroken` anywhere (a broken probe is fixed, not declared).

### 3. Exit codes: two new `paddock-cli` codes (a shared change)

`Exit::NotMeasured = 6` ("done, but something could not be measured; nothing failed") and `Exit::CheckBroken = 7` ("a
check's own probe or control failed; fix the check"). `Findings` stays 5. A run with findings and something not measured
exits 5 and lists both. `ALL`, `name()`, `describe()` and the contract test extend; the numbers stay a contract. RT-3's
private `EXIT_NOT_MEASURED` (6) becomes `Exit::NotMeasured`; RT-8's incomplete report exits 6; vm-smoke's INCOMPLETE (3)
moves to 6 when that runner is ported (3 is `NotRunning` in `paddock-cli`). The JS conformance CLI (today 0 clean, 1
findings) adds 6 and 7, listed as an intended difference in RT-5's shadow table (as ADR 0088 §7.2 does for its flips).

### 4. One test helper for an external tool: `paddock_testkit::require`

```rust
let jq = require_tool("jq")?;                                  // runs `jq --version`; a path on success
let ff = require_tool_that("ffmpeg", &["-hide_banner", "-f", "lavfi", "-i", "testsrc", "-frames:v", "1", "-f", "null", "-"])?;
require_capability("landlock", || landlock_supported())?;
let vm = require_env("PADDOCK_VM", "1")?;                      // inside an #[ignore = "..."] test
```

Each returns `Result<T, NotMeasured>`; the test's signature is `-> Result<(), NotMeasured>` (or it calls `.unwrap_measured()`).
Because libtest has no skip and hides a passing test's output (measured), "visible skip" cannot come from inside the
test. So:

- **Default (no variable set: `cargo test`, `cargo nextest run`, CI): strict.** `NotMeasured` fails the test with
  `not measured: jq is missing (spawn failed: …). Install it (brew install jq / apt-get install jq) or run through ./dev
  test, which lists it as a skip.` A missing tool can never be green by accident.
- **Reporting mode: `PADDOCK_NOT_MEASURED=report:<file>`.** Set only by a wrapper that reads the file afterwards: `./dev
  test` and the push gate (ADR 0038). The helper appends one JSON line (test id, tool, reason) and the test returns `Ok`;
  the wrapper prints the list as a table and folds it into its `Verdict` (§2), so the run exits 6 unless each is declared
  optional. That is the "visible skip locally": visible because the wrapper owns it, never because a test printed it.
- **CI never sets reporting mode** except through the same wrapper, which applies `docs/optional-measurements.toml` for
  that leg. A host leg that legitimately lacks a tool (golden.rs's case: node on the macOS leg) is an `[[optional]]`
  entry with `where.leg = "host"`, not code.

The JS twin is `requireTool(t, 'cargo')` in `@decryp7/paddock-e2e` (Playwright, Vitest) and in conformance's test utils
(node:test): strict by default (throws `not measured: …`), `t.skip(reason)` only in reporting mode, recorded to the same
file format. Shared behaviour is tested once, in each helper (ER-22).

### 5. A check over every instance: C103 (next free id; C102 was the highest on every branch read)

A Rust check in RT-5's `./dev check` host, enumerating, not sampling:

- **(a) Silent skips in tests.** In test code (`tests/`, `#[cfg(test)]` modules, `*_tests.rs`, `*.test.mjs`, `*.spec.ts`,
  `*.e2e.ts`): a `return`, `return Ok(())`, `let … else { return }` or `?`-less early exit in the same block as, or the
  block right after, a print (`println!`, `eprintln!`, `console.*`) whose text matches
  `/skip|not measured|not available|missing|not found|not installed/i`; and in JS, a `skip:` option or `test.skip(cond)`
  whose condition probes a tool (`which`, `spawnSync`, `existsSync` on a tool path, a `has<Tool>` variable). Fix:
  `require_*`/`requireTool`. A syn pass for Rust (as ADR 0076's C42), the masker of RT-5 for JS.
- **(b) Local copies of the outcome.** In tooling sources (`tools/`, `conformance/`, `scripts/`, the `paddock-cli` and
  `paddock-testkit` crates excluded as the canonical home): a string literal containing "not measured", "check broken"
  or "incomplete", or an enum variant named `NotMeasured`/`CheckBroken`/`Incomplete`, outside `paddock_cli::outcome`
  (Rust) or the one JS module that mirrors it (`conformance/src/outcome.mjs`). Product code is not in scope (Dyno's
  sensor readings say "not measured" about a sensor, a domain value).
- **(c) Exits.** `std::process::exit` in `tools/` only in `main.rs` (Clippy `disallowed-methods` with `allow-invalid` in
  `tools/*/clippy.toml`, the existing preset style), so every tool exits through `Verdict`/`Report`.

Today's sites enter through ADR 0088's engine as a **ratchet** (`docs/ratchets/c103.toml`): 47 Rust and 2 JS skip
sites for (a) and the copies table for (b); the ratchet only shrinks, and new sites fail at once.

### 6. Positive controls (ER-33), each a test

- `Verdict`: a property test over every combination of outcomes and optional entries: no combination containing
  `NotMeasured` without a matching entry, or containing `CheckBroken`, yields `Exit::Ok`; a tag context ignores optional.
- Waivers: a planted crash and a planted half-run with one finding, each under a matching `*` waiver, still exit 7 and 6
  (incident #5, in both hosts).
- `require_tool("paddock-no-such-tool-0089")` fails in strict mode with "not measured" and writes one record in
  reporting mode; `require_tool("cargo")` returns a path (the control that the probe can see a present tool).
- C103: fixtures with each silent-skip shape of the instance table and each copy shape, one clean twin each; the check
  must report every planted one (it fails on today's broken instances: the pre-`fb4c579fa` RT-4 oracle and the
  `d055bcf31` RT-3 oracle are fixture inputs, verbatim).
- RT-8: a replayed run list with one unreadable job log exits 6, not 5; an app config without `failOnFlakyTests`
  makes the Playwright probe `NotMeasured` for that app (its precondition), so the report is not "complete".

### 7. Migration (after acceptance, rule 40 step 8; one small commit per group, C103 green as the proof)

1. **The two open defects, now, on their own branches, with failing tests first** (they are review fixes, not migration,
   so they do not wait for this ADR): RT-5 `mod.rs:295` raises the unwaivable not-measured finding whenever the record is
   not measured (drop `&& found.is_empty()`), the crash finding becomes unwaivable, and the scope mismatch is fixed by
   using `record.status()` alone; RT-8 maps an incomplete report to the exit 6 RT-3 already uses, and treats an app
   without `failOnFlakyTests` as not measured for the Playwright marker. Same fix in the JS host's `runner.mjs:24`
   (crash unwaivable), listed in RT-5's shadow table.
2. `paddock_cli::outcome`, `Exit` 6 and 7 (WP-1); `paddock_testkit::require` and the JS twins (WP-2).
3. The tooling copies onto the type: RT-5's `Status` and record (the first, it is already the type's shape), RT-3, RT-8,
   RT-2's `notices` (they become `NotMeasured` items with an optional entry for the local-trial flag), C100's gates
   strings, golden.rs's `skip()`; the JS host's `unwaivable` becomes `outcome.mjs`; vm-smoke with its port.
4. The 47 test sites by kind (instance table), app by app, each app's branch owning its files.
5. Register `outcome` in `docs/known-copies.toml` (canonical `crates/paddock-cli/src/outcome.rs`; the JS mirror is a
   declared copy with a delete-by of RT-17's last batch), and the C103 ratchet.

### 8. Relations

- **ER-33 / rule 71.** This is ER-33's mechanism: its "reports `not measured` or `check broken`, and the release does not
  pass" becomes a type, an exit code and a check. D10 proposes the text.
- **ADR 0088.** Same principle one level down: 0088 says a list entry no covering run saw is "not measured, never
  stale"; this ADR says the run itself. Both read RT-5's `RunRecord`; 0088's coverage test (`measured()`) and this ADR's
  `outcome()` become one function on the record, so they cannot drift. `docs/optional-measurements.toml` and the C103
  ratchet are lists of 0088's engine.
- **The RT plan.** §3's "positive controls instead" becomes §6's rule for every package, and every RT brief from RT-9 on
  says: a check that cannot run returns `Outcome::NotMeasured`; tests use `require_*`; no `notices` for things not
  measured. RT-22 (`paddock-tool`) re-exports the module instead of owning it; nothing waits for RT-22.
- **ADR 0038 (push gate).** Its table's "what was skipped (with the declared reason)" is the `Verdict` table; the
  `ci-only` declarations in `paddock.ci.json` count as optional for the push gate (one source, read through the same
  `Optional`); the G26 stamp is written only for `Exit::Ok`, so a not-measured run never satisfies the dispatch check.
- **ER-12.** Strict-by-default `require_*` is "never silently skip a test" made mechanical; the race skip in `live.rs` is
  reported separately as an ER-12 bug.

### 9. Release impact (ER-31)

`paddock-cli` is in Dyno's and Pitwall's shipped dependency sets (through `paddock-service`, which uses
`Exit::{Ok, NotRunning}`). WP-1 only adds variants and a module; no shipped code calls them. It is still shared code on
their release path, so WP-1 lands outside a release freeze the coordinator names (as RT-22). Everything else is tooling
and tests, outside every app's dependency set. Migrating an app's tests (WP-5) may turn its CI red where a tool is missing
from a runner: that is the point, and each such case gets either the tool or an `[[optional]]` entry in the same change.

## Parallelism and cost (rule 51)

`Verdict` is a fold over at most a few hundred items. `require_tool` spawns `<tool> --version` once per test (a few ms; it
replaces the probe the tests already do). C103 (a) is one syn parse per test file in the RT-5 host, which already walks
the tree; (b) is a literal scan the masker already does. No timing is quoted as evidence (rule 48); WP-3 records C103's
cost on the root run in `docs/benchmarks.md`. Nothing to parallelise beyond the host's own file walk.

## Alternatives

- **Fix each instance and add a review checklist line.** Rejected: that is today; five packages written the same week
  by careful agents repeated it, and RT-3's fix round replaced an `#[ignore]` with an `eprintln!`, the same class in a quieter form.
- **Lenient by default locally (record and pass), strict only when `CI=true`.** Rejected: measured above, nothing a
  passing test prints is seen, so the local run would be silently green and a CI without `CI` set would be too. Kept as
  option (b) of D4.
- **A custom test harness with a real skip (`libtest-mimic`, `harness = false`).** Rejected for now: one harness per
  test target across nine Cargo workspaces, and nextest's integration to re-check; reporting mode gets the same
  visibility through the wrapper.
- **`#[ignore]` for every test that needs a tool.** Rejected: an ignored test never runs in CI (no CI line runs them),
  which is incident #2's first form.
- **One exit code for both new states.** Rejected (D3 (b)): "fix your environment" and "fix the check" have different
  owners and next steps (ER-27).
- **Put the type in `paddock-tool` and wait for RT-22.** Rejected: RT-5, RT-8 and RT-3 need it now, and `paddock-cli`
  already owns the exit-code contract it maps to.

## Consequences

- A developer without `ffmpeg`, `jq` or a landlock kernel sees failing tests under plain `cargo test` until they install
  the tool or use `./dev test`; the message says which.
- CI legs that silently lacked a tool turn red once their tests migrate (the ffmpeg tests in the Linux container are
  the first known case): each needs the tool in the image or a reasoned, expiring optional entry.
- Scripts that read exit 5 as "incomplete" (RT-8's first cut, nothing else found) read 6.
- Reviews stop looking for this class by hand: C103 and the type carry it.

## Rollback

`docs/optional-measurements.toml` can declare a measurement optional per place in one reviewed line; C103 can be set to
report-only in `docs/ratchets.toml` (ADR 0088). Reverting WP-1 is safe only after the tooling stops returning 6 and 7;
the codes themselves are additive.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Where the outcome type lives | (a) `paddock_cli::outcome` now, re-exported by `paddock-tool` (RT-22); (b) `paddock-dev`'s core now, moved by RT-22; (c) wait for RT-22 | **(a)**: `paddock-cli` owns `Exit`, which it maps to; no wait, no move |
| D2 | The states | (a) four: `Clean`, `Findings`, `NotMeasured(reason)`, `CheckBroken(reason)`; "out of scope" is "not selected", not a state; (b) five, with `NotApplicable` | **(a)** |
| D3 | Exit codes | (a) `NotMeasured` 6, `CheckBroken` 7, `Findings` 5 wins over 6 when both; (b) 6 for both, told apart in JSON | **(a)** |
| D4 | Test helper default | (a) strict everywhere; a visible skip only through a reporting wrapper (`./dev test`, push gate) that turns it into exit 6; (b) strict when `CI` is set, record-and-pass otherwise | **(a)**: (b) is silent locally, measured |
| D5 | Where "optional" is declared | (a) `docs/optional-measurements.toml` (id, where, reason, expiry), an ADR 0088 register; `paddock.ci.json`'s `ci-only` also counts for the push gate; never on a tag's release path, never for `CheckBroken`; (b) at the call site in code | **(a)** |
| D6 | The check | (a) C103 in the RT-5 host: silent-skip shapes in tests (Rust and JS), local copies of the outcome in tooling, `process::exit` outside `main.rs` (Clippy); today's sites as a shrink-only ratchet; (b) Clippy only (cannot see print-then-return) | **(a)** |
| D7 | The two open defects (RT-5 `mod.rs:295` and crash waivable; RT-8 exit 5) and the JS host's crash | (a) fix now in their branches with failing tests, independent of this ADR; (b) wait for WP-1 | **(a)** |
| D8 | ffmpeg in the CI image (the first known silent leg) | (a) add it to `paddock-ci` with the next image build; (b) an `[[optional]]` entry for the Linux leg until the image has it; (c) both, (b) expiring at the image date | **(c)** |
| D9 | The 91 `#[ignore]` tests | (a) out of scope here: visible, declared opt-ins; a later check that each names the job that runs it; (b) include now | **(a)** |
| D10 | Engineering rule | (a) extend ER-33: "… and a check, probe or test that could not look reports it as its own outcome and exit code, which no waiver, baseline, aggregation or test harness can turn into a pass; a measurement may be skipped only where reviewed data declares it optional, with a reason and an expiry"; (b) a new ER-37 | **(a)**, as ADR 0088 D11 did for lists |
| D11 | Tag gating | (a) none beyond the normal gates; WP-1 lands outside a release freeze (ER-31: shared code Dyno and Pitwall ship); (b) block tags until WP-4 | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | The open defects (§7.1): RT-5 `mod.rs` (single derivation, unwaivable crash, scope rule) and the JS host's `runner.mjs:24`, each with the planted-waiver test first; RT-8's exit 6 and the `failOnFlakyTests` precondition with a replay test first. In their own branches | 1 |
| WP-1 | `paddock_cli::outcome` (`Outcome`, `Reason`, `Verdict`, `Optional` reader, JSON and text), `Exit::NotMeasured`/`CheckBroken`, contract test, property test, 99% coverage; the JS mirror `conformance/src/outcome.mjs` | 1.5 |
| WP-2 | `paddock_testkit::require` (`require_tool`, `require_tool_that`, `require_capability`, `require_env`, strict and reporting modes, record format) and the JS `requireTool` for node:test, Vitest and Playwright; `./dev test`'s reporting wrapper (a thin command over nextest that reads the records into a `Verdict`) | 2 |
| WP-3 | C103 (a), (b), (c) in the RT-5 host with fixtures for every shape (§6), the ratchet seed of today's sites, `docs/optional-measurements.toml` with its schema and the D8 (b) entry; the benchmark line | 2 |
| WP-4 | Tooling copies onto the type (§7.3): RT-5 record, RT-3, RT-8, RT-2 notices, C100 strings, golden.rs, the JS host's exit codes 6/7 with the shadow-table entries | 1.5 |
| WP-5 | The 47 Rust and 2 JS skip sites by kind (§Instances), per app; the ffmpeg image line (D8 (a)) | 1.5 |
| WP-6 | `docs/known-copies.toml` entry `outcome`, the ER-33 text (D10), the Paddock rule 71 line, lessons entry, the RT brief template line | 0.5 |
| | **Total** | **10** |
| | Later, inside existing packages: vm-smoke's verdict onto 6 with its port; ADR 0038's push gate reads `Verdict` (RT-12); the 0088 engine's coverage reads `RunRecord::outcome` (its WP-2) | (+0.5) |

First cut: WP-0 (1 day, now, independent of acceptance) closes the two open instances; WP-1 to WP-3 (5.5 days) close the
class for new code; WP-4 and WP-5 retire today's copies.
