# 86. A bug fix names its failing test, and a replay shows it failed without the fix

- Status: Proposed (2026-11-10). Drafted at the user's request of 2026-11-10 (ER-21 redesigns) as Part B of the ADR 0038
  amendment of the same date; the independent review of that draft (`f0979961a`) asked for it to stand alone and for the
  changes listed in "Review answers". Needs the decisions below.
- Date: 2026-11-10
- Related: ER-11 / rule 5 (every bug fix starts with a failing regression test; the reviewer's mutation or revert check,
  2026-10-30), ER-12 (no retries that turn red green), ER-21 / rule 40, ER-27, ER-33 / rule 71, ER-36; ADR 0038 and its
  amendment A2 (every commit of a landing green; the per-commit gate), the ADR 0038 amendment of 2026-11-10 (the push
  gate, whose step 1 checks the trailer's form), ADR 0025's amendment of 2026-11-10 (build folders keyed by their source),
  ADR 0075 (trailers cannot be corrected), ADR 0077's amendment (the hermetic child helper), user decision 2026-11-05 ("a
  failing run shown for every new test", memory `redesigns-2026-11-05`).
- Number: 0086 was free on every `origin` branch on 2026-11-10 (`git ls-tree -r --name-only` over `git branch -r` after
  `git fetch`: family ADRs end at 0085) and in the local branch list.

## Context

- Rule 5 asks for a failing test first. Since 2026-10-30 a reviewer checks it by hand: `cargo mutants --in-diff`, or
  revert the fix and run the new test. On 2026-11-10 several implementers wrote the test together with the fix and nobody
  saw it fail (as the coordinator reports; the branches were not re-read for this draft).
- On `origin/main` since 2026-11-03, 316 commit subjects start with `fix`; nothing machine-readable says which test
  failed first or that it did. Lessons record some by hand ("Failing-first test: `...`", `docs/lessons.md:1405`).
- A red "test first" commit is not available: ADR 0038 A2 requires every commit of a landing to pass its gate.

## Pattern

**Whether a regression test ever failed without its fix is known only if a reviewer happened to revert and run it.**

**Invariant.** Every `fix:` commit that changes non-test code names its regression tests; a replay at the parent with
only the commit's test side applied shows them red (twice), and at the commit green; a fix where that cannot be shown is
named as such and has a recorded reviewer check.

## Decision

### 1. The record: a trailer

`Failing-first: <test id>[, <test id>]` with ids `cargo:<package>::<binary>::<test path>`, `vitest:<file>::<name>`,
`pw:<file>::<title>`; or `Failing-first-waived: <reason>` (CI configuration, docs, a build script, no test seam). The
push gate's commit-lint (step 1) checks the form on every `fix:` commit. A trailer is used, although ADR 0075 moved
copy waivers out of trailers, because this one is a claim the replay verifies before landing while the branch can still
be amended; it never needs correcting afterwards.

### 2. The replay: `./dev check failing-first <commit>`

1. **Where.** In one fixed replay worktree per agent worktree (`<worktree>-replay`, made once by `./dev worktree`,
   ADR 0083), detached, reused for every replay of that worktree, so its build folder's source key (ADR 0025 amendment)
   stays the same and the builds stay warm. Never in the author's worktree.
2. **The test side** of the commit is a **declared** list, `docs/test-paths.toml` (globs: `**/tests/**`, `*_tests.rs`,
   `**/testing.rs`, `*.spec.ts`, `*.test.{ts,mjs,js,cjs}`, `**/fixtures/**`, ...), plus the hunks inside `#[cfg(test)]`
   modules of other Rust files, found by a syn pass. A changed file that **looks like a test** (a name containing `test`,
   `spec` or `fixture`) but matches no glob is **refused** ("declare it in docs/test-paths.toml or rename it"), so the
   split cannot silently treat test code as fix code.
3. **Red.** At `<commit>^` with only the test side applied, each named test runs **twice** through the hermetic child
   helper (ADR 0077 amendment); it must fail both times on an assertion or a panic (`red 2/2`). One pass in two is
   `flaky-red`, reported as a failure of the claim (ER-12).
4. **Not measured.** The test cannot run without the fix: a Rust compile error, or in TypeScript an import that does
   not resolve, `X is not a function`, `X is not defined` or `Cannot read properties of undefined` raised from the test's
   own call of the code under test (the new API is missing, not wrong). These count as `not measured`.
5. **Green.** At `<commit>` the named tests pass (twice).
6. **Output.** The failing output's tail per test, kept with the push gate's stamp and printed in the landing summary:
   `Failing-first: <n> red 2/2 then green, <k> waived, <m> not measured (<m> reviewer revert checks recorded)`.

### 3. "Not measured" needs a reviewer's record, within a limit

A `not measured` or `waived` fix lands only with a recorded rule-5 reviewer check: the reviewer's report line
`Revert-check: <commit> <test id> red` (the reviewer reverted the non-test change by hand, or ran `cargo mutants
--in-diff`), which the integrator copies into the landing summary. The landing summary turns red when more than a
threshold of a landing's `fix:` commits are `not measured` (D4: 25 % to start, set after WP-5's replay of real ranges), so
the escape hatch cannot become the norm.

### 4. Where it runs

In the integrator's landing step beside `./dev check --commits` (ADR 0038 A3), for every `fix:` commit of the range,
through `heavy`. The push gate runs only the trailer form (cheap); the replay needs builds and runs at landing, or by the
author on demand before review.

## Check (positive controls, ER-33)

A fixture repository (`FixtureRepo`, ADR 0077 amendment) with: (i) a fix whose test also passes on the parent: refused;
(ii) a proper fix: red 2/2 then green; (iii) a test inside `#[cfg(test)]` in the fixed file: split and replayed; (iv) a
trailer naming a test that does not exist: `check broken`; (v) a `fix:` commit without a trailer: commit-lint refuses;
(vi) a test that fails 1 of 2 at the parent: `flaky-red`; (vii) a TS test whose import of a new function fails: `not
measured`; (viii) a changed `foo.test-helper.ts` outside the declared globs: refused. The replay prints the number of
tests it ran; 0 for a non-waived fix is `check broken`. Registered in `docs/gates.toml` (ADR 0060).

## Alternatives

- **Pasted failing output in the commit** (D1 (b)): a reviewer reads it; nothing proves it came from this test at this
  parent.
- **A separate red `test:` commit before the fix:** breaks ADR 0038 A2 (every commit green) and bisect.
- **Mutation testing only** (`cargo mutants --in-diff` in CI): stronger for the test's quality, Rust only, slower; it
  stays the reviewer's tool and a later CI job (rule 5's follow-up).
- **Reviewer judgement only:** today's state.

## Consequences

- Rule 5 becomes checkable; a test written with the fix that never fails is found at landing, not by a reviewer's luck.
- Every agent worktree gains one replay worktree (one more warm build folder per active worktree; disk bounded by ADR
  0083's worktree tool and the orphan sweep).
- Landings with `fix:` commits take the replay's build and two runs per named test.

## Rollback

Make the landing summary report-only (one setting); the trailer stays harmless. Remove the commit-lint form check to drop
the trailer requirement.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | The record | (a) trailer plus replay; (b) trailer plus pasted output read by the reviewer; (c) a red `test:` commit first | (a) |
| D2 | The test side | (a) a declared glob list plus syn for `#[cfg(test)]` hunks, refusing unknown test-looking files; (b) path heuristics only | (a) |
| D3 | Red means | (a) fails 2 of 2 on an assertion or panic; compile, import and missing-function failures are `not measured`; (b) any failure counts | (a) |
| D4 | `not measured` and waived fixes | (a) land only with a recorded reviewer revert check, and the landing summary is red above 25 % of the landing's `fix:` commits; (b) no limit | (a) |
| D5 | Where the replay runs | (a) one fixed replay worktree per agent worktree, at landing (and on demand); (b) in CI on every push | (a) |
| D6 | Release gating | (a) none for app tags; required at landing for `fix:` commits once WP-3 lands; (b) the next tag waits | (a) |

## Work packages (agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-1 | Trailer form in commit-lint (push gate step 1) and the waiver form | 0.5 |
| WP-2 | `docs/test-paths.toml`, the split (globs, syn for `#[cfg(test)]` hunks, refusal of unknown test-looking files) | 1.25 |
| WP-3 | `./dev check failing-first`: replay worktree, red 2/2, not-measured classification for Rust and TS, green, output | 2 |
| WP-4 | Landing summary line, `Revert-check:` records, the threshold; the `integrate` skill and the reviewer brief | 0.5 |
| WP-5 | Replay the `fix:` commits of three recent landings; record timings in `docs/benchmarks.md`; set the threshold | 0.5 |
| WP-6 | Rule 5 text (the record and the replay), lessons entry, index | 0.25 |
| | **Total** | **5** |

## Release impact (ER-31)

Tooling only; no shipped code. No app tag waits for it (D6 (a)).

## Review answers (revision 1 of the ADR 0038 amendment, Part B)

- "Not measured" needs a recorded reviewer revert check and a threshold: §3.
- TypeScript import and `is not a function` failures count as not measured: §2 item 4.
- The test side is a declared list plus syn, refusing unknown test-looking files: §2 item 2.
- Red on 2 of 2 runs: §2 item 3.
- One fixed replay worktree per agent worktree, so its build folder key stays stable: §2 item 1.
