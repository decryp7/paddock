# The integrator's train checklist (ADR 0098)

Built by hand today; the merge queue does the same through CI (docs/plans/merge-queue.md). Everything runs in the integrator's own
locked worktree, never in the main checkout (rule 2).

1. **Before approval, the trial.** `./dev ci dispatch ci.yml --trial <branch> --max <duration>` merges the branch onto the open
   train and dispatches the branch's one run on the combined tree. A conflict is reported with its files and nothing is
   dispatched: the branch goes back to its author.
2. **Merge one member at a time** with `git merge --no-ff`. A conflict that is not trivial sends the member back with the
   conflicting files.
3. **The member gate.** `./dev merge check --gates HEAD` on the merge: the detectors (dropped hunks, lost tests, workspace-hack,
   generated files), then the fast gates of `./dev check --pre-push` on the combined tree with `--base HEAD^1`. Load the machine
   lightly: `CARGO_BUILD_JOBS=2`, one member at a time.
4. **A member that fails is taken out, not fixed on the train.** Reset the train branch to `HEAD^1` in your own worktree
   before merging anything else, and send the member back to its author with the failing rows. While the gate is report-only
   (until ADR 0098 records its first green run on a real train) a failure is printed with `::warning::` and the exit code stays
   0: read the output anyway and send the member back for a real failure.
   A member that changes the test kit's process start (`crates/paddock-testkit/src/process.rs`, `ambient.rs`) or how CI makes build
   folders (`tools/paddock-dev/src/ci_isolation/`) also runs `testcommand-ab.yml` on its trial ref and reads the Windows runner's
   cells before it joins a train; a VM or Mac number is `not measured` (ADR 0107 D5, ER-9).
5. **One CI run per train round**, through `./dev ci dispatch` (the run is queued at once and its first job waits while an older run holds a shared runner). Tests run
   once per train, not once per member (ER-8).
6. **A red train run.** `./dev ci triage <run> --members <train-base>` lists the members, each with the projects its own change
   reaches, and for every new failure the members that reach it. Take out every suspect, or fix the cause in its owner's
   branch, then fix round by failed tests only (ER-10). A failure that also fails on main is nobody's.
7. **The release sequence, then land.** One command after the fix round's failed run: `./dev train release <train-branch> --since-run <failed run>
   --workflow ci.yml|debrief.yml [--accept-flaky apps/<app>/app/<spec>:<line>[:<col>]]... [--reuse <workflow>=<run>]... --max <duration>`, run in the worktree
   that holds the pushed train head. It runs only while it holds the train-CI reservation (`scripts/train-ci-hold.sh`): it proves the hold
   (`PADDOCK_VM_LOCK_HELD` confirmed by `scripts/vm-lock-run.sh --check`) or runs itself again under it, with a hard deadline, and exit 75 means
   another train's sequence holds it. Before every dispatch it makes the disk check `ci dispatch` makes. `--since-run` must be a finished red
   run of the same workflow (`passed:<run>` for a round that already ran: judged against the train head, not dispatched again). A red fix round
   is judged by `ci triage`'s verdict: PASS only when triage worked, `skipped_jobs` is 0, every count is a whole number and adds up to the rows,
   and every row is a FLAKY test (not a job) on the `--accept-flaky` list; anything else stops. An accepted test must be one the train did not
   touch, at the feature level of ER-12: refused when `git diff origin/main...head` changes the test file, a helper beside it, or the source it
   drives, which is `<package>/e2e/feature-sources.json` (an optional JSON object from a spec's file name to the source prefixes it drives,
   `[]` for none) or else the files named like the spec under `<package>/src`; a spec that resolves to neither is refused (fail closed): add its
   declaration. `--since-run` must be on this train's ref.
   **The fix ladder** (user, 2026-11-20): after a red run the command widens its evidence one rung at a time, each rung only when the one
   below is green. Rung 1, the failed tests only: the NEW, FAILED and FLAKY rows of `ci triage <run> --json` that a job's runner can take
   alone (nextest `-E`, vitest files, Playwright `file:line`) go to the workflow's `only_tests` input (a JSON object from a job's display name
   to its test ids, with `since_run`); jobs not named do not run. Its run is read with `ci triage <run> --ran`: a named job that ran fewer
   tests than it was given ids, or none, is **NOT MEASURED** and stops the ladder (a green run of an empty filter is not green). A red rung 1
   stops with the next step: fix, push through the gate, run again with the same `--since-run` and `--test-round n+1` (at most
   `--max-test-rounds`, default 3; past it, or with 0, the ladder starts at rung 2). Rung 2, their whole jobs: the `since_run` round above.
   Rung 3, stage 1 on every OS (the fast checks and unit tests): only when the workflow declares the stage input
   (`ladder::STAGE_INPUT`, the layout of `ci/staged-fast-then-long`), else a logged note and rung 4; the shared reader must not count a stage 1
   run as a full one, or the sequence stops. Rung 4, the full run on the exact head, below. **Only rung 4 lets a train land**: the report
   lists each rung (green with its run, or not run with the reason; a rung that had nothing to run is skipped, never a pass).
   Then the full `ci.yml` and `debrief.yml` (no `since_run`, ER-10) are dispatched unless `ci verdict` already has a green full run of the head,
   and each must be green by **`ci verdict`**, the one reader, or red only by what a train may land on (the sealed `Landable` of `ci_runs::landable`:
   the newest complete full run of the exact head, finished `failure` with clean earlier attempts, whose triage is the same run and head with
   `skipped_jobs` 0 and counts that add up, and whose every row is a named test that is an accepted, untouched FLAKY or ALREADY FAILING ON MAIN; a
   NEW, UNKNOWN or job row refuses; every accepted row is logged); then the release workflows, each ending `success` (a cancel or any other end
   stops). `--reuse` is accepted only for a green run on this train ref whose commit is an ancestor of the head and whose app the planner
   does not reach from that commit to the head (the diff is logged). Every wait ends at `--max` (one run at 6 h) and a stop is one `::error::`.
   Then `./dev train land <train-branch> [--accept-flaky ...]`: it refuses unless `ci verdict` is green for the exact head, or the full run is
   landable as above (`ci.yml`, and `debrief.yml` when the plan from main reaches Debrief), and unless every release workflow whose app the plan
   reaches has a green run on this train covering the head (rule 54: on the head, or on an earlier train commit with the app not reached
   since); it reports "landed on a full run with N accepted FLAKY, M already failing on main", fast-forwards `main` (no force, no rebase; refused when `origin/main` is not an ancestor), verifies with
   `ls-remote`, then runs the bounded, never-blocking disk clean `./dev train landed` (also callable alone). These commands replace the scratch
   scripts of train 14 (`seq-lib.sh`, `seq-release.sh`, `seq-full.sh`, `triage-verdict.py`, `land-*.sh`). Land only after the full green run on the final tree (rule 50 (b)); delete the `trial/*` refs of landed branches with `./dev ci trial-prune` (a trial that is sent back or not dispatched already deletes its own ref).
