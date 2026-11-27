# CI reader replay suite

Real runs of the Gitea server, replayed through the readers that decide a landing (user decision 96, 2026-11-26).

**The class.** Defects in the CI verdict, reuse and fix-items readers showed up only at a train's landing: the expected map of run
9167, the 200-commit walk of run 9150, the first-parent line of run 9056. A narrowed trial never exercises those readers, and a unit
test feeds them invented runs. A replay feeds them what the server really answered.

## What is there

- `./dev ci record-fixture <run>... --name <shape>` records finished runs into `tools/paddock-dev/tests/fixtures/runs/<shape>/`: each
  run (`run-<id>.json`), its jobs (`jobs-<id>.json`, first page), the trimmed log of every job that ran (`log-<job>.txt`: the plan job's
  `plan scope:` and `plan verdict:` lines, the nextest and Playwright result lines), each earlier attempt of a rerun
  (`attempt-<run>-<n>.json`), the parents of a trial's merge commit (`commit-<sha>.json`) and the list (`runs-1.json`). Names, e-mail
  addresses, avatars, runner names and home folders are replaced on capture (conformance C110). It is read-only on the server and needs
  `GITEA_TOKEN`. `--out <dir>` records elsewhere.
- `tools/paddock-dev/src/ci_runs/replay.rs` serves a shape as the server (`testutil::Fake`) and asks each reader: the verdict on the
  declared scope and on the old `plan verdict:` line, the landing verdict, the reuse decision of a push, the trial verdict of a member,
  and the fix items (`fix_items::lines`, `add_to_lessons`). The `SHAPES` table holds the answer of each, with a one-line reason.
- The recorder also keeps a run's artifacts when it has any (2026-11-27, the escape monitor's `plan-record`): `artifacts-<run>.json` (the
  list as the API answers it), `artifact-<id>.redirect.txt` (where the download redirects, signature dropped) and `artifact-<id>.zip.hex`
  (the zip, as hex so a recording stays text). `ARTIFACT_SHAPES` in `replay.rs` replays them through `escape::artifact::ArtifactStore`.
- What a replay cannot hold is git. A shape declares what the change touched (`Touch`) and the first-parent line the reuse reader walks;
  the table says which.

## Add or update a shape

A reader change adds or updates a replay shape (AGENTS.md, "Testing and quality gates").

1. Pick the real run whose shape the change is about (a green, a red, a reused, a rerun, a nightly, a trial). Include the runs it
   depends on: a push that reuses needs the landing run it reuses and main's base run, and the run that base itself reused.
2. Record it: `./dev ci record-fixture 9170 9171 --name push-reused`. A name is lower-case words joined by `-`.
3. Add a `Shape` row to `SHAPES` in `replay.rs`: the workflow, the run asked, the asking run (a push), the `Touch`, the answer of each
   reader and one line of why. Run it once, read what the reader answers, and write down the answer you expect for that reason, not
   the one it printed. A wrong row is a finding, not something to loosen.
4. When a reader changes an answer, update the rows it flips in the same change and say why in the commit body. A shape the change
   does not flip stays as it is: that is what the suite is for.
5. `every_recorded_folder_has_a_shape_and_every_shape_a_folder` fails for a recording with no row.

Rows reflect the reader of their commit. Since job-model WP-2 (ADR 0123, main 696f472f4) the readers know scope line v3 and a v2 line is
never reused (D2): the v2 shapes say "not measured", expected and not a fault. Run 9167 has a v3 line from before the planner fix (its
expected map lists the e2e legs a train skips), so the reader refuses it as "declared scope not run"; run 9180 is the fixed shape.

## Shapes

| Shape | Runs | Is | Answers |
|---|---|---|---|
| `green-train` | 9170 | a train's landing run, v2 | not measured (D2) |
| `green-refused-by-old-reader` | 9167 | a green run, v3 line before the planner fix | red: declared e2e legs skipped |
| `red-one-flaky` | 9168 | red with one FLAKY test, v2 | not measured (D2); its log still grades as a fix item |
| `push-reused` | 9073, 9074, 9170, 9171 | a push over a v2 run | not reused (D2) |
| `failed-only-round` | 9146, 9147 | a failed round and its green rerun, v2 | not measured (D2) |
| `nightly` | 9103 | a scheduled nightly, red, v2; it declares ci.yml's scope | not measured (D2) |
| `member-trial` | 9163 | a member's trial, v2 | not measured (D2) |
| `green-full-v3` | 9180 | a full green train run, v3, clean shadow | green |
| `push-reused-v3` | 9180, 9181 | the push over it | skip, reusing 9180 |
| `member-trial-v3` | 9179 | a member's trial, v3, narrowed | green: its own scope line ran |
| `plan-record-artifact` | 9272 | a trial's plan job whose v4 upload `plan-record` the REST API lists; the zip download answers 302 to `/zip/raw` | read back to the run's record (`ARTIFACT_SHAPES`) |
| `flaky-noncargo-reach` | 9329 | train aa before its fix: a FLAKY paddock-throttle test while ci.yml's plan job and upload steps made the plan reach crates | landing green with one fix item (`FixItems`, nothing touched); the FLAKY grade on the planner's `packageGraded` reach is the test `a_flaky_test_on_train_aas_real_plan...`; the recorded plan is `plan-9329.json` |
