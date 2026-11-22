# 0115. Every cache step reports what it restored and saved, from its own outputs

- Status: Proposed 2026-11-22 (the fix in the same change is user-approved 2026-11-22; ER-21 asks for this redesign with it).
- Date: 2026-11-22
- Related: ER-21 (a recurring class gets a redesign), ER-27 (logs explain failures), ER-33 (a check needs a positive control; "not
  measured" is not a pass), ER-37 (common practice), ER-41 (new checks start report-only), ER-44 (a complete fix); ADR 0111 (keyed
  caches instead of shared folders), ADR 0022 (the `./dev` launcher).
- Number: 0115. On 2026-11-22 the highest ADR on any `origin` branch was 0114 (`git ls-tree docs/adr/` over every remote branch; 0112 and
  0113 are on open branches).

## Context: the class

**A CI cache step says "hit" or "saved" from something other than what the cache said, and nobody notices that the cache is empty.**

| # | Where | Instance | Source |
|---|---|---|---|
| 1 | dev tool cache key | the key read the wrong lines of `./dev --wrapper-hash`, so it never matched | 67bcfcac6, run 8509 |
| 2 | dev tool cache verify | `verify` decided hit or miss by parsing `./dev`'s own log text | 75b3fd31b, run 8560 |
| 3 | dev tool cache order and verify | the first `./dev` call (setup-rust's build-jobs step, node-caches, build-jobs) ran before the restore, so the tool was built from source; `verify` then saw no build in its own log and printed `state=hit save=false` although the restore found nothing | run 8576 (job 44445, log lines about 226-232) |
| 4 | node-caches summary | called a partial restore (`cache-hit=false` with a restore-keys match) "miss" | `.gitea/actions/node-caches/action.yml`, the Cache summary step |

Instance 3 cost about 116 runner-minutes per round: 42 of 44 jobs of run 8576 rebuilt `./dev` (130 to 176 s each), every run green. It
starts with 5bdd263cc (ADR 0111's keyed caches): runs 8425 and 8426 on train 17, without it, built `./dev` 0 times; it is still there on
run 8601. Nothing failed, so nothing showed it for days. Three fixes went to the cache's key and its verify text; none asked whether the
cache was ever filled.

## Decision

1. **A cache step is reported by one reader, from the cache action's own outputs.** `conformance/ci/cache-report.sh` takes
   `cache-hit` and `cache-matched-key` of an `actions/cache` (or `cache/restore`) step and says `exact` (`cache-hit` is `true`), `partial`
   (a matched key without `true`, a restore-keys hit) or `miss`, and what the job knows about the save (`yes`: a lookup found the key
   after the save; `no`; `pending`: actions/cache's post step at the end of the job; `n/a`). It writes the job-summary line and one fixed
   `cache-report: name=… outcome=… saved=… key=…` log line. No step decides hit or miss from log text.
2. **The save decision is the restore's output.** The dev tool is saved when the restore found nothing under the exact key and the
   binary in the job passed its hash check, whatever built it (an earlier step of the job included). Log text only warns about a
   restored binary that `./dev` had to rebuild (state `rejected`, never saved: the key exists).
3. **Restore before first use.** The dev tool cache is one action, `dev-tool-cache`, used first by every action that calls `./dev`
   before a Rust job's own steps: `setup-rust` (before its build-jobs step), `node-caches`, `build-jobs`. The first use in a job does
   the work and sets `PADDOCK_DEV_TOOL_CACHE_DONE`; later uses do nothing. A use before a compiler exists finds no hash and leaves the
   work to the next use. A test reads the three actions through the workflow accessor (ER-42) and fails when a `./dev` call comes before
   the restore.
4. **A check for a cache that is never filled.** `./dev ci cache-audit` reads the `cache-report:` lines of the job logs of the last N
   runs through the CI API reader and names every key with at least three misses (a pending save counts once per run), no exact hit and
   no confirmed save. Report-only (ER-41: exit 0 with `::warning::`); it turns blocking in the tool step that records its first green run
   on the real tree, by run id here: *not yet recorded*. A window with no `cache-report:` line is "check broken" (exit 6), never clean
   (ER-33). Its positive control is a unit test: 42 misses and no save is a finding; the same window without the lines is broken.
5. **No unreported cache step grows.** `conformance/test/ci-actions.test.mjs` lists the `actions/cache` steps that do not report yet (11
   files, 28 steps: the scheduled and release workflows, `plan`, `tauri-bundle`, `nightly-mark`); the list only shrinks (ER-46). New
   cache steps report through `cache-report.sh`. Wiring the listed steps is the follow-up (WP-3 below).
6. **Not covered, on purpose.** sccache's local folder is not an `actions/cache` step; the `sccache-stats` action already reports its
   hit rate. The 3 MB Angular build cache stays: it is a partial-restore cache keyed by lockfile and sources, and the report now says
   when it is `partial`. Whether it buys anything is measured from the first runs that carry the report, not guessed (the brief of this
   change asks to keep it unless the report shows it buys nothing; the report cannot show that yet).

## Common practice (ER-37)

GitHub documents `actions/cache`'s outputs, `cache-hit` and `cache-matched-key`, as the way to decide what to do after a restore and
recommends the restore, build, save order with `if: steps.cache.outputs.cache-hit != 'true'` (docs.github.com, "Dependency caching
reference"). This design is that, with one reader of the outputs instead of a shell snippet per step. Nothing in common practice audits
a cache that is never saved; GitLab shows cache pull and push in the job log, and that audit is the project's own addition, one
subcommand over the run reader it already has. Cost: about one agent-day for 1 to 4, which is also the common-practice cost of the
fix; the audit (4) is about half a day beyond it. Cheaper option: skip 4 and rely on the summary lines; rejected because three fixes
in a row showed that a summary nobody reads does not find an empty cache.

## Work packages

- WP-1 (done in the change that adds this ADR): items 1 to 3 and the order test.
- WP-2 (done in the same change): `ci cache-audit` and its tests, report-only.
- WP-3: wire the listed unreported `actions/cache` steps through `cache-report.sh` and shrink the list; delete-by: the first release
  line after Dyno 1.2.0.
- WP-4: record the first green `ci cache-audit` run on the real tree by run id, then make it blocking (ER-41).

## Consequences

The first CI run after this lands proves the save: the first job to miss the key builds, saves and confirms it, and the others (they
start together on six lanes, so several may miss) report `exact` from the second wave on. `./dev ci cache-audit --runs 3` on that run
and the next one is the check; until the key is saved it prints the 42-job pattern of run 8576.
