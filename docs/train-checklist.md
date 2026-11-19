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
   --workflow ci.yml|debrief.yml [--accept-flaky <file:line[:col]>]... [--reuse <workflow>=<run>]... --max <duration>`, run in the worktree
   that holds the pushed train head. It takes the train-CI hold (`scripts/train-ci-hold.sh`), checks the disk (`./dev disk check`) before
   every dispatch, dispatches the fix round (`since_run`, the failed jobs only; `passed:<run>` for a round that already ran: it is judged,
   not dispatched again), judges a red round with `ci triage`'s verdict (PASS only when triage worked, no job was skipped, the counts add up
   and every row is a FLAKY test, not a job, on the `--accept-flaky` list; anything else stops), then `debrief.yml` with bundles after a
   `ci.yml` round, then the release workflows, and stops at the first run that is not green with one `::error::` (cause and next step).
   `--reuse` names a green run of a release workflow whose app's tree is unchanged; it is checked (exists, that workflow, success) and logged.
   Every wait ends at `--max` (one run at 6 h). Then `./dev train land <train-branch>` fast-forwards `main` to the head (no force, no
   rebase; it refuses when `origin/main` is not an ancestor) and verifies it with `ls-remote`. These two commands replace the scratch
   scripts of train 14 (`seq-lib.sh`, `seq-release.sh`, `seq-full.sh`, `triage-verdict.py`, `land-*.sh`). Land only after the full green run on the final tree (rule 50 (b)); delete the `trial/*` refs of landed branches with `./dev ci trial-prune` (a trial that is sent back or not dispatched already deletes its own ref).
