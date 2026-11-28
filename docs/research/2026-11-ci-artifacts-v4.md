# CI artifacts: v3 to v4 (2026-11-01)

Gitea 28.2.0 (`routers/api/v1/repo/action.go`) lists only artifacts whose `content_encoding` contains `/`, which means
v4 artifacts. v3 uploads were invisible to the REST API, so tooling such as the VM harness could not fetch them.

**Chosen:** `ChristopherHX/gitea-upload-artifact` and `ChristopherHX/gitea-download-artifact`, branch `v4` (no tags exist),
pinned by digest (upload `81f940d0...`, download `75635f32...`, both "disable ghes no support check" on 2024-01-22).
The upstream `actions/*-artifact@v4` refuse a Gitea server with `GHESNotSupportedError`; the maintainer-suggested forks work
(go-gitea/gitea#36973, read 2026-11-01: the `@v4` fork worked, `@v6` failed with "unauthorized"). The forks are upstream
upload v4.2.0 and download v4.1.1+3 commits, each with one line changed (`isGhes` false), so `merge-multiple`, `pattern`, `overwrite` and `compression-level` exist.
The forks have no tags, so Renovate cannot track them: the digests are bumped by hand (renovate.json disables them).

**v3 to v4 differences that matter here**
- An artifact is immutable and its name must be unique per run (re-runs are fine on Gitea 28.2: uploads are create-or-get per attempt, and `overwrite` works). All names in our workflows are already unique per job (OS, shard, target).
- No merging of uploads into one name.
- `download-artifact` without `name` still extracts each artifact into `<path>/<name>/`, the v3 layout, so the release jobs'
  `find artifacts -type f` is unchanged.
- `retention-days` is honoured by the v4 fork. The server default applies wherever none is set; the plan job's `plan-record` sets 14 (escape::RETENTION_DAYS).

## Re-check on 2026-11-27 (Gitea 28.4.0, the escape monitor's plan record)

- Brought from the branch `ci/artifacts-v4` (commit e8be98c76, never landed) with the ci.yml steps; the claims above were written
  against 28.2.0.
- Still true on 28.4.0 (measured, trial run 9271 of `fix/plan-records-artifact`): a v3 upload (`actions/upload-artifact@ff15f030...`)
  finished "successfully uploaded", the web route `.../actions/runs/9271/artifacts/plan-record` returns its zip, and
  `GET /repos/decryp7/paddock/actions/runs/9271/artifacts` (also `?name=plan-record` and the repository-wide list) lists nothing; the
  repository-wide list holds only the 5 v4 artifacts of run 7318. So the REST API still lists only v4 artifacts.
- This change moves ci.yml's upload steps to the fork (`ChristopherHX/gitea-upload-artifact@81f940d0...`, the plan job's
  `plan-record` first). The release workflows, the demo releases and the other workflows still use the v3 pin and go in the release
  batch (rule 54).

## The release batch (2026-11-28, user decision 130c)

- Every remaining `actions/upload-artifact` and `actions/download-artifact` step moves to the same two pinned forks: the release
  workflows (`app-release`, `shell-demo-release`, `workbench-demo-release`, `throttle-release`, Debrief's release job in `debrief.yml`),
  the fuzz, bench and mutants workflows, the bench template and the rest of `debrief.yml`. No workflow uses the v3 action now.
- Names stay unique per run: the matrix legs name their artifact by OS or target, and Debrief's seven fuzz uploads by target.
  `download-artifact` without `name` keeps the v3 layout (`<path>/<name>/`), so the releases' `find artifacts -type f` is unchanged.
- `conformance/test/ci-actions.test.mjs` fails on any `*-artifact@` step in any workflow or template that is not one of the two forks
  at its digest, with a positive control. Renovate leaves the forks alone (no tags): `renovate.json` and `renovate/app.json` replace
  the old "<4" rule with it.
- Rule 54: the release path changed, so each touched app's `<app>-release.yml` is dispatched from the branch with the bundle built.
