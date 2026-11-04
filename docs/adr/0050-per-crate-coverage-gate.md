# 50. A per-crate coverage gate with a register

- Status: Accepted (2026-11-04, decided by the user)
- Date: 2026-11-04
- Related: ER-9 / rule 3 (fix locally, CI confirms), ER-27 (CI steps print one error annotation with the cause and a job
  summary), AGENTS.md "Shared code" (crates >= 99 %, components 100 %), ADR 0022 (new tooling in Rust; see "Where it lives"),
  ADR 0049 (small merges). Rule 66 (proposed on `origin/docs/rules-2026-11-04`) treats an aggregate coverage gate as shared
  code that an app ships unless per-crate output clears the app's crates; this gate gives that per-crate output.
- Number: 0050. `origin/main` has up to 0049; 0045 to 0048 are reserved on `origin/docs/redesign-adrs-7-9`; no remote branch
  carries 0050 (`git ls-tree` over all remote branches, 2026-11-04).

## Context

The `crates` job ran `cargo llvm-cov` once over the whole workspace and `scripts/coverage-gate.mjs` held the merged figure
to 99 %. Run 7489 measured 98.67 %: the gate failed, and because it was a plain step the next step, `cargo deny check`,
was skipped, so one failure hid the other. The one number also hides which crate is short: a large well-covered crate pays
for a small bare one, and nobody could say from the log which crate to fix. The files that cannot run in unit tests were
left out by one long `--ignore-filename-regex` in `ci.yml`, with the reasons only in a comment.

## Decision

1. **Every crate in `crates/` must reach 99 % of its lines**, measured from the same lcov file, per crate.
2. **A crate may sit lower only through the register `crates/coverage.toml`**: a `reason` and a `review` date. Its `minimum`
   is the measured figure (cut to two decimals) and the register is a ratchet, enforced by the gate and not by trust:
   - a figure under the minimum fails, exactly, even when it is still above 90 %;
   - a figure above the minimum by more than max(0.05 %, 2 lines) fails too ("raise the minimum to X"), so minimums equal
     the measured figures and only go up. A crate whose figure differs between two CI runs of the same tree is
     nondeterministic: fix the test or document why in its reason.
   - `ratchet --base <rev>` runs in the plan job (which has the history) and reads the register at the base. **A lowered
     minimum of an existing entry always fails; no trailer allows it.** A new `[[crate]]` entry and a new `[[exclude]]`
     entry fail unless a commit of the change (`<rev>..HEAD`) has the trailer `Coverage-lowered: <crate|path> <reason>
     approved: user` in its trailer block (`git interpret-trailers --parse`, key matched case-sensitively). The trailer is an
     honour-system marker that review must check, until the user decides on server-side approval.
   - The base is the initial seed only when `git ls-tree --name-only <base> -- crates/coverage.toml` succeeds with empty
     output; any other git error fails. No base on a pull request or a push to main is a warning.
   - `suggest` prints the whole register the measured figures give (header and `[[exclude]]` entries as they are, `[[crate]]`
     blocks regenerated; a new crate gets a `TODO` reason that validation refuses).
   - **The initial seed comes from CI.** The landed register is byte-for-byte the `suggest` output of two CI runs of that exact
     tree, with the run ids in the landing commit message; platform arms differ (`paddock-menu`'s Tauri tests run on Linux
     and Windows only), so no developer machine seeds it. After the seed only raises are possible.
3. **The hard floor is 90 % per crate.** No reason allows less; a crate under it fails even with an entry.
4. **Whole files that cannot run in unit tests are excluded one by one in the same register**, each with a `reason` and, if
   one runs it, the `job` that covers it. The `--ignore-filename-regex` of `ci.yml` is generated from these entries
   (`node scripts/coverage-gate.mjs regex crates/coverage.toml`); there is no hand-written regex. An entry for a missing
   file or crate, an entry without a reason, and a passed review date (warning only) are reported.
5. **Shared UI components stay at 100 %** (AGENTS.md); nothing changes there.
6. **A coverage failure never stops the other checks.** `cargo deny check` is its own step that runs unless the job was
   cancelled, and a last step lists each step's result in the job summary (ER-27).
7. **The report is per crate**: crate, figure, minimum, the reason when below 99 %, and pass or FAIL, as a table in the job
   summary, with one `::error::` per failing crate naming the crate, its figure, its minimum and the next step.

### Where it lives

A Node script (`scripts/coverage-gate.mjs`, with the register reader `scripts/coverage-register.mjs`), not a `./dev`
subcommand. The `crates` job already runs this gate with Node in the `paddock-ci` image; a `./dev` command would first
need the binary built or fetched in that job and a TOML dependency in `tools/paddock-dev`'s own lockfile, for a parser of
about 60 lines. ADR 0022's direction stays: when the `crates` job moves to `./dev`, the check moves into it with
the same register and tests.

## Consequences

- A short crate is named in the log and in the summary; a failure in one crate no longer needs the aggregate to be read.
- The register is the one place that says why a crate or file is below the line, and when to look again.
- The register is seeded from CI (two runs, identical `suggest` output); after that a lowered minimum is impossible and a
  new entry needs the trailer.
- A crate that improves must raise its minimum in the same change (the gate fails otherwise), so it can never drop back unnoticed.

## Rejected

- **One aggregate number** (the old gate): hides the short crate, and a large crate's surplus pays for a small crate's gap.
- **An 80 % floor, or a lower default**: it lets any crate lose a fifth of its lines untested; 90 % is already the lowest
  figure the family accepts, and only with a reason and a review date.
- **A static minimum below 99 % without a ratchet**: lets a registered crate lose coverage down to its minimum unnoticed.
