# Amendment to ADR 0117 (2026-11-27): D13, the selection records are an Actions artifact, not refs

- Status: Proposed (2026-11-27), as the user decided on 2026-11-27. Amends ADR 0117 §8 and D13 (the escape monitor's store); branch
  `fix/plan-records-artifact`, based on main 7da64dedb.
- Why: §8 saved each plan's selection record as a blob under `refs/paddock/plan-records/*` and the plan job pushed it. ci.yml is
  read-only by design (`permissions: contents: read`: no job writes to the repository), so Gitea refused the push ("User permission
  denied for writing", train y; earlier run 8976 failed for the missing credentials). Every commit was therefore `not measured` at
  night and the monitor saw nothing. Giving the plan job `contents: write` would break the one property the workflow's header states.

1. **The store is an artifact of the plan job's own run.** `dev plan --ci` writes the record to the file named by `PLAN_RECORD_FILE`
   (`escape::artifact::FileSink`; ci.yml sets `${{ runner.temp }}/plan-record/record.json`), and the plan job's next step uploads it with
   `actions/upload-artifact`, the pin the other jobs use (v3; v4 aborts on Gitea: ER-26), as the artifact `plan-record` with
   `retention-days: 14`. A test reads ci.yml through the YAML model and ties the name and the retention to `escape::ARTIFACT_NAME` and
   `escape::RETENTION_DAYS`, and proves that ci.yml has no `contents: write` (or `write-all`), no job-level write permission and no step
   that pushes a ref (ER-42).
2. **The nightly reads it with its own read token** (`PLAN_TOKEN`, the nightly job's `BUILD_INPUTS_TOKEN`): for a commit, the API's run
   list filtered by `head_sha` (the shared `ci_runs::client::Client`, ER-22), the runs of `ci.yml` newest first (at most 5), each run's
   `GET .../runs/<id>/artifacts?name=plan-record` and `GET .../artifacts/<id>/zip`. The zip is read in process (`escape::artifact::unzip_one`,
   stored or deflated, bounded to 1 MiB; no archive crate, no `unzip` on the runner).
3. **Missing, expired or unreadable stays `not measured` (ER-33).** No run, no artifact, an `expired` artifact or a 404 is `Load::Missing`;
   a damaged zip, a record that does not parse or that is for another commit is `Load::Unreadable` with the cause. `judge` is unchanged
   and reports both as not measured, never clean and never a guess. A bad record of one run does not hide a good one of another run of the
   same commit.
4. **Deleted:** `GitRefs`, `REF_PREFIX`, the token header for git over http (`job_token`), the fetch and the push, `prune` and the store's
   `commits` and `remove` (the 14 days are the artifact's retention now), and the direct `base64` dependency of paddock-dev. `save` stays
   the one writer (it takes a `Sink`); `judge` and `chain` read a `Store`, which has `get` only.
5. **Common practice (ER-37).** An artifact of the run is how GitHub Actions and Gitea pass a run's result to a later reader; refs written
   by CI need a write token, which a read-only workflow must not hold. Cost: about 0.5 agent-day, less than the token route.
6. **Evidence.** The trial run of `fix/plan-records-artifact` (`ci dispatch --trial`) must show its plan job's artifact `plan-record`
   through the API; the run id is recorded in the commit that records it. Until the first nightly after landing reads a record, the
   monitor stays report-only (ER-41).
