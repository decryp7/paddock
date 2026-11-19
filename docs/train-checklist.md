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
7. **Land** only after the full green run on the final tree (rule 50 (b)); delete the `trial/*` refs of landed branches with `./dev ci trial-prune` (a trial that is sent back or not dispatched already deletes its own ref).
8. **Generated and append-only files after each merge** (`merge=union` in `.gitattributes`: both sides' lines survive and nothing conflicts).
   Run `./dev gen check --only adr-index,conformance-check-lists`; when one is stale, run its regenerate command
   (`node conformance/bin/paddock-adr-index.mjs`, `node conformance/bin/paddock-check-lists.mjs`) and commit the result with the
   merge. A check-list change still conflicts in `conformance/paddock.ci.json` (union is not set there: it breaks the JSON): take
   either side, then regenerate.
9. **A release cut on the train.** Union has no idea which section a line belongs to: a member that added its CHANGELOG entry under
   `Unreleased` while another change cut that section into a released version gets its entry filed under the released section
   with no conflict. After merging a member into a train that holds a release cut (a CHANGELOG heading moved or renamed), read
   `git diff <train-before-merge> HEAD -- CHANGELOG.md apps/*/CHANGELOG.md` and move each entry that the member added to the section
   it belongs to, in the merge commit. The order of entries within a section is not guaranteed either; that is accepted.
