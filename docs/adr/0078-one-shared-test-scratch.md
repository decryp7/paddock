# 78. One shared test scratch: one folder type per language, a per-run root, every failed removal reported, a leak check in every test job

- Status: Proposed (2026-11-09). The user approved drafting on 2026-11-09 (ER-21 redesigns). Needs the decisions below
  and an independent review before any build.
- Date: 2026-11-09
- Related: ER-14 / rule 7 (tests leave nothing behind), ER-12 / rule 5 (no retry that turns a red run green), ER-21 /
  rule 40, ER-22 / rules 42, 43 and 55 (share first; one implementation per mechanism, `docs/known-copies.toml`, ADR
  0034, C58), ER-27, ER-33 / rule 71.
- Composes with: **ADR 0076** (owned background work: a folder that cannot be removed is usually a thread or process
  still writing into it; 0076's leak guard finds the writer, this ADR finds the folder), **ADR 0077** (the same run marker
  names the run root), **C14** (`conformance/src/hermetic.mjs`), `conformance/ci/job-tmpdir.sh` (the one writer of
  `TMPDIR` in CI), ADR 0074 Part C (host size; `temp_dir()` left out of its static check on purpose).
- Number: free on every `origin` branch on 2026-11-09 (see ADR 0077).
- Evidence read at `origin/main` `b51c72509` unless a row names a branch. The survey script and its output are in
  `/Volumes/d7sst7/dev-targets/scratch/adr-0077-0078/` (`count.py`, `counts.txt`, `drops.txt`).

## Pattern

**Each test suite makes and removes its temp folders its own way, and a removal that fails is silent.** Tests use
`tempfile::TempDir` (whose drop ignores a failed removal), build paths from `std::env::temp_dir()` and remove them with
`let _ = remove_dir_all(..)`, or write a private folder type whose `Drop` does the same. When a background writer still
holds the folder, or the test process is stopped, the folder stays, nothing says so, and the next check that looks (or
the disk) finds it. Each fix copies the remedy into the next suite.

**The invariant that should hold.** Every temp folder a test makes comes from one shared type per language, under one
root per test run; a removal that fails is reported with the folder, the error and the test, never ignored; every test
job checks at its end that the run root is empty and that nothing else appeared in its temp folder, and names what is
left.

## Context: the incidents

| # | Where | What happened | State |
| --- | --- | --- | --- |
| 1 | `debrief.yml` run 7813 (train 1, Mac runner) | The temp-leak check failed on `debrief-duck-diff-*` and `debrief-maintain-reorder-skew-*`: both fixtures ignored a failed removal. | Fixed on `origin/fix/debrief-test-temp-leaks` `411c7af0f` by **copying** `debrief-core`'s `tests/it/scratch.rs` `remove_or_defer` (200 attempts, then stderr and an `atexit` removal) into `debrief-store-duck/tests/common/scratch.rs`; `maintain_tests.rs` includes it by path. Two copies now. |
| 2 | CI 7108 (`apps/debrief/docs/testing/temp-leaks.md`) | `debrief-prop-*` folders whose removal had failed silently. | The first `remove_or_defer`, in `debrief-core` tests only. |
| 3 | CI 7096 (same doc) | A test stopped by nextest's timeout left `debrief-app-test-ai-*` and `debrief-test-stores-*`. | `debrief-app`'s `test_support` arms a SIGTERM handler and an `atexit` removal: a third local mechanism. |
| 4 | The dev Mac (`docs/lessons.md` on `origin/train/2026-11-08-1`, line 202) | 98k `debrief-*` folders (55 GB) in `$TMPDIR`. | Guard types and Debrief's leak check. |
| 5 | Host runners (`docs/lessons.md:825`) | The runner's temp folder is the user's; a local run killed during a CI job made the job's check fail. | `job-tmpdir.sh`. |
| 6 | `conformance/test/kept-clone.test.mjs` on `origin/ci/kept-clone-sparse-checkout` | `fixture()` makes `mkdtempSync(join(tmpdir(), 'paddock-kept-'))` and nothing removes it: every run leaves a `paddock-kept-*` folder with a git origin, a work repo and clones. | Open. No job outside Debrief checks for leaks, so nothing failed. |
| 7 | Production scratch, `private_tempdir` (`debrief-ipc/src/scratch.rs` on `origin/train/2026-11-09-2`, 4 callers) | A 0700 private folder for shipped code; its removal is `tempfile`'s, silent. | Follow-up named in the brief; §6. |

A shared type exists and is little used: `paddock_testkit::Scratch` (`crates/paddock-testkit/src/scratch.rs`: `work`,
`home`, `tmp`, a leftover check on `tmp`, `PADDOCK_TESTKIT_KEEP`) has 35 call sites, 34 in `crates/` and 1 in shell-demo.
Its own removal is `tempfile`'s, so it is silent too. Debrief's leak check
(`apps/debrief/scripts/ci/check-temp-leaks.mjs`, snapshot and check) runs in 7 steps of `debrief.yml` and in no other of
the 24 workflows.

## Instances (survey, `origin/main` `b51c72509`)

**Method.** A Python pass over every tracked `.rs` file (not `vendor/`): a site is test code when its path is under
`tests/`, `benches/`, `examples/`, a `*_test(s).rs`, `tests.rs`, `test_*.rs`, `testing.rs`, `fake*.rs`, testkit or
`build.rs`, or when it comes after the file's first `#[cfg(test)]`/`#[cfg(all(test`/`#[cfg(any(test`. The last rule is
a heuristic: a non-test item after a test module counts as test, and a "shipped" count may hold test helpers in `src/`.
WP-0 reads every shipped-classified site. TypeScript and `.mjs`: `git grep` for `mkdtemp(Sync)?(` and, per file, any
`rmSync`/`rm(`/`rmdir`.

| Pattern (test code) | Debrief | Dyno | Pitcrew | Pitwall | Spotter | demos | `crates/` | `tools/` | Total |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `tempfile` constructor (`TempDir::new`, `tempdir()`, `Builder::new().prefix`, ...) | 661 | 343 | 0 | 73 | 142 | 19 | 754 | 38 | **2,030** |
| `std::env::temp_dir()` | 145 | 11 | 30 | 0 | 0 | 2 | 9 | 0 | **197** |
| `let _ = remove_dir_all(..)` | 174 | 16 | 50 | 0 | 0 | 0 | 16 | 0 | **256** |
| A private type whose `Drop` removes a folder | 38 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | **39** |
| `paddock_testkit::Scratch` | 0 | 0 | 0 | 0 | 0 | 1 | 34 | 0 | 35 |

Of the 39 private `Drop` types (list in `drops.txt`), **37 ignore a failed removal** and 2 (`debrief-core`
`tests/it/properties.rs:853`, `properties_files.rs:82`) defer it; the branch of incident 1 adds two more users and the
second copy. Shipped-classified sites to read in WP-0: 46 `let _ = remove_dir_all`, 29 `tempfile` constructors, 24
`temp_dir()`, and 7 shipped `Drop` types that remove a folder (Debrief `video/mod.rs:105`, `debrief-cli/src/run.rs:78`,
`debrief-ipc/src/elevation.rs:104`, `endpoint.rs:192`; Dyno `dyno-agent/src/bench_scratch.rs:118`; `paddock-archive`
`stage.rs:43`; `paddock-ipc` `endpoint/unix.rs:147`, the only one that checks its result).

TypeScript and scripts: 70 files hold the 97 calls of `mkdtemp(`/`mkdtempSync(` (`git grep -E 'mkdtemp(Sync)?\('`; 170
lines in 71 files mention `mkdtemp`, imports included); **25 of them have no removal call at all** (Debrief 4, Dyno 3,
conformance 4, scripts 4, docs 3, one each in Pacenotes, Pitcrew, Pitwall, Sighter, Spotter, tools and ui; a file may still remove through a helper of its own, which WP-0 reads).

## Decision

### 1. One Rust type: `paddock_testkit::scratch`

```text
pub struct ScratchDir            // one folder; the TempDir-shaped replacement (path(), join(rel), keep())
pub fn dir(prefix: &str) -> ScratchDir     // made under the run root, named <prefix><random>
pub struct Scratch               // today's three-folder type (work, home, tmp), made under the run root
pub fn remove(path: &Path) -> Removal      // the one removal: Removed | Deferred { error } (reported)
pub fn run_root() -> &'static Path         // <base>/paddock-run-<id>, made once per process, 0700 on Unix,
                                           // the process holding a shared lock on <root>/.lock until it exits
```

- **The run root.** `<id>` is the ADR 0077 marker (`PADDOCK_TEST_RUN`, else `NEXTEST_RUN_ID`), so every test process of
  one run shares one root and a job checks one folder. Without a marker (a plain `cargo test`), `<id>` is the process's
  `ProcessId` (pid plus start time, rule 53). `<base>` is `PADDOCK_JOB_TEMP` when set (CI, `job-tmpdir.sh`), else the
  temp folder read **once, before** any `EnvLock::hermetic` can move `TMPDIR` (the kit captures it at its first call
  and `hermetic` keeps the run root, so a test's own `tmp` folder is never the base).
- **The root's lock (rule 53).** Nextest runs one process per test, and other worktrees' runs share the same temp
  folder, so a root may have many live users at once. Every process that uses a root takes a **shared** OS file lock on
  `<root>/.lock` (`flock(LOCK_SH)`; `LockFileEx` without `LOCKFILE_EXCLUSIVE_LOCK` on Windows) and holds it until it
  exits; the OS releases it when the process dies. A root is removed or swept only by a process that gets the
  **exclusive** lock without waiting; anything that fails to get it leaves the root alone. The identity in `.owner` is
  only a hint for the log, never the reason to delete.
- **Removal.** `Drop` calls `remove`. On Windows, only the error kinds known to be transient there (a sharing violation, a
  file pending delete) are tried again, within 500 ms of the **real monotonic clock** (`Instant`; a test's fake clock
  never governs a real file system), and every retry is logged on stderr with its error and elapsed time, so a retry
  that turns green is still visible; any other failure, on any OS, is **deferred at once**: one stderr line (`scratch: could not remove <path>: <error>; test <name>; deferred`), the path recorded in
  `<run root>/.deferred`, and a removal at process exit. No yield loop: a folder that needs one is almost always a writer
  that outlived its owner (ADR 0076), and retrying hides it (ER-12).
- **Deferrals are failures that name themselves.** The job's leak check (§3) lists `.deferred` with the test names.
  Until ADR 0076's leak guard lands (its M1), a deferral fails the job, not the test (D2); afterwards the test.
- **Stopped processes.** A process stopped by nextest's timeout leaves its folders in the run root; the job's check
  reports them and removes the root once it holds the exclusive lock (every test process has ended by then). Locally,
  the next run sweeps only the roots whose exclusive lock it gets without waiting, so a live run in another worktree is
  never touched (rule 53). This replaces Debrief's SIGTERM handler and `atexit` copies.
- **Keep for inspection.** `PADDOCK_TESTKIT_KEEP` keeps a failed test's folder, as today.

### 2. One TypeScript helper: `@decryp7/paddock-e2e/scratch`

`scratchDir(prefix)` returns `{ path, [Symbol.dispose]() }` (TypeScript `using`, node `using` where available) and, with a
`node:test` or Playwright test context, registers its removal with `t.after`. Same run root (`PADDOCK_TEST_RUN`), same
report of a failed removal. A subpath export with no Playwright import, so `conformance/`, `scripts/`, `tools/` and the
Vitest suites can use it.

### 3. A leak check in every test job

Debrief's `check-temp-leaks.mjs` becomes the family's: `./dev check temp-leaks --snapshot|--check` (the Rust port, ADR
0022), run by job-isolation's acquire and release and by the `job-tmpdir` action (one owner; no workflow calls it
itself). It reports, with one `::error::` each: anything left in the run root, `.deferred` entries with their tests, and any
new entry in `$PADDOCK_JOB_TEMP` and the socket folder (the app patterns become one list in the script). **Positive
control:** its test plants one leftover of each kind and must report each; when it finds no run root while the job ran
tests, it reports `check broken` (rule 71).

### 4. The lint (number on acceptance)

A conformance check over test code (the survey's classification, made exact with a syn pass for inline test modules):
`tempfile` constructors, `std::env::temp_dir()`, `remove_dir_all` inside `impl Drop` or behind `let _ =`, and, in
TypeScript tests and scripts, `mkdtemp`/`os.tmpdir()` outside the helpers, are errors for new sites and a shrink-only
ratchet per file for today's (counts above). `paddock-testkit` and `paddock-e2e/scratch` are the allowed homes.
**Positive controls:** a fixture per pattern must fail; `git show 411c7af0f^:apps/debrief/crates/debrief-store-duck/tests/common/mod.rs`
must be flagged; the check prints how many test files it scanned and fails as broken at zero.

### 5. The copies register covers test-support mechanisms

`docs/known-copies.toml` counts only non-test source files today (its header), so the two `remove_or_defer` copies cannot
be registered. The register gains `scope = "test"` on a mechanism: C58 then counts test files for that mechanism only.
Entry `test-scratch-removal`, canonical `paddock-testkit`, copies: `debrief-core/tests/it/scratch.rs`,
`debrief-store-duck/tests/common/scratch.rs` (once incident 1's branch lands), `debrief-app` `test_support` (`TempDir`,
`scratch_dir`, `process_scratch_dir`), `debrief-core` `test_dirs::scratch_dir`; owner `debrief`, delete by
`debrief-v1.2.0`.

### 6. Production scratch folders (D6)

The 7 shipped `Drop` types above and `private_tempdir` are the same mechanism in shipped code: a private folder removed
on drop. Proposed: `paddock_env::scratch::private_dir(prefix)` (0700, atomic create, removal result logged with ER-27
fields, never the user's path), canonical for them, registered as copies until each app's next release.

### 7. Rule text (proposed; number on acceptance)

"A test makes temp folders only through the shared scratch type (Rust `paddock_testkit::scratch`, TypeScript
`@decryp7/paddock-e2e/scratch`), under the run's root; a removal that fails is reported, never ignored; every test job
checks that its run root is empty. Checked by the scratch lint and the job leak check."

## Alternatives rejected

- **Keep copying `remove_or_defer`** (incident 1): the fourth mechanism in one app; other apps have none.
- **Retry removal until it works** (200 attempts today): turns a writer that outlived its owner into a green run (ER-12).
- **Fail the test at once on any failed removal:** right after ADR 0076's guard exists; before it, a Windows indexer can
  fail a correct test (D2 keeps the strict mode as the end state).
- **A clippy `disallowed-methods` entry:** also hits shipped code (`private_tempdir`, the stores), and cannot see
  `let _ =` or a `Drop` body; the temp-leaks doc's own TODO found 159 `temp_dir` uses blocking it.
- **Only the leak check:** finds the folder, not the site that made it, and only in jobs that run it.

## Consequences

- One place decides how test folders are made, removed and reported; a leak names its test.
- Every app's test jobs get the leak check; some will go red on day one (the 25 TypeScript files without removal, the
  kept-clone test). WP-3 fixes or ratchets each before the check is an error.
- `tempfile` stays a dependency of the testkit and of shipped code; tests stop naming it.

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Read the shipped-classified sites; exact counts with a syn pass; the ratchet baseline | 1 |
| WP-1 | `paddock_testkit::scratch`: `ScratchDir`, `dir`, run root and its base, the shared and exclusive `.lock`, `remove` with logged Windows retries, deferral record, exit removal, the lock-guarded sweep; tests on Linux, macOS, Windows (two processes and a sweeper) | 2.5 |
| WP-2 | `@decryp7/paddock-e2e/scratch` and its tests | 1 |
| WP-3 | `./dev check temp-leaks` (port of Debrief's script), wired into job-isolation and `job-tmpdir`; positive controls; fix or ratchet the day-one leaks (kept-clone included) | 2 |
| WP-4 | The lint, fixtures and ratchet | 1.5 |
| WP-5 | `scope = "test"` in the register and C58; the `test-scratch-removal` entry | 0.5 |
| WP-6 | Migration (mostly mechanical, an implementer with the counts as the brief): Debrief 3 (39 types, the four local mechanisms), Dyno 1, Pitcrew 1, Pitwall 0.5, Spotter 0.5, `crates/` 2, tools and demos 0.5, TypeScript files 1 | 9.5 |
| WP-7 | Rule text, lessons, the temp-leaks doc moved to `docs/testing/`, ADR index | 0.5 |
| | **Total** | **18.5** |
| WP-8 (D6) | `paddock_env::scratch::private_dir`, the 8 shipped sites | 1.5 |

First cut (the mechanism, the check in every job, new sites refused): WP-0 to WP-5 and WP-7 = **9 days**; each app's
part of WP-6 lands in its next release phase.

## Release impact (ER-31)

Test code and CI only, except D6. No app ships `paddock-testkit`. The new leak check runs in every app's test jobs; a red
leak check in another app's tests does not block a release under ER-31 (a), as long as it is not in shared code the app
ships. Under D7 (b) the ratchet blocks no tag.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Where the Rust type lives | (a) `paddock_testkit::scratch` (exists, `Scratch` already there). (b) A new crate. | (a). |
| D2 | A failed removal | (a) Defer at once and fail the job naming the test; Windows transient kinds tried for 500 ms; fail the test once ADR 0076's leak guard is in. (b) Today's 200-attempt retry, then defer. (c) Fail the test at once. | (a). |
| D3 | The run root | (a) `<PADDOCK_JOB_TEMP or the temp folder captured before hermetic>/paddock-run-<marker>`, shared with ADR 0077's marker, each user holding a shared lock and a sweep only with the exclusive one. (b) Per-process roots. | (a): one folder per job to check; the lock makes the sweep safe across processes and worktrees. |
| D4 | The leak check | (a) The family's `./dev check temp-leaks` in every test job, through job-isolation. (b) Debrief only, as today. | (a). |
| D5 | The lint | (a) A conformance check on test code with a ratchet. (b) clippy `disallowed-methods`. | (a). |
| D6 | Production scratch | (a) `paddock_env::scratch::private_dir` now, copies registered. (b) Out of scope. | (a), as a separate WP in Debrief 1.2's phase (it owns 5 of the 8 sites). |
| D7 | Release gating | (a) Ratchet entries block each app's next tag. (b) New sites fail from landing; old ones migrate in each app's phase; no tag blocked. | (b): test-only code, nothing a user runs. |
| D8 | Register scope | Add `scope = "test"` to ADR 0034's register (amendment) so C58 sees test-support copies. | Yes. |
| D9 | Scope now | (a) All (18.5 days). (b) First cut (9 days). | (b). |
