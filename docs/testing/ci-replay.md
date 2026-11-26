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

Rows reflect the reader of their commit. Run 9167 has a v3 scope line (job model WP-2, ADR 0123): a reader that knows only v2 must say
not measured; the reader that knows v3 reads it as a landing run. v2 lines are "not reused" once D2 lands (ADR 0123).

## Shapes

| Shape | Runs | Is |
|---|---|---|
| `green-train` | 9170 | a train's landing run: its diff's selection, no full run |
| `green-refused-by-old-reader` | 9167 | a green run with a v3 scope line |
| `red-one-flaky` | 9168 | red with one FLAKY test of untouched code |
| `push-reused` | 9073, 9074, 9170, 9171 | a push that reuses the train's run |
| `failed-only-round` | 9146, 9147 | a failed round and its green failed-only rerun |
| `nightly` | 9103 | a scheduled nightly, red |
| `member-trial` | 9163 | a member's trial run |
