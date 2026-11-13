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
5. **One CI run per train round**, through `./dev ci dispatch` (it waits while another train holds a shared runner). Tests run
   once per train, not once per member (ER-8).
6. **A red train run.** `./dev ci triage <run> --members <train-base>` lists the members, each with the projects its own change
   reaches, and for every new failure the members that reach it. Take out every suspect, or fix the cause in its owner's
   branch, then fix round by failed tests only (ER-10). A failure that also fails on main is nobody's.
7. **Land** only after the full green run on the final tree (rule 50 (b)); delete the `trial/*` refs of landed branches.
