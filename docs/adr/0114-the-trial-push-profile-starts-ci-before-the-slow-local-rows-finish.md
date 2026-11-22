# 0114. The trial push profile: a `wip/` or `trial/` push starts CI before the slow local rows finish

Status: accepted by the user on 2026-11-22 (top priority for CI speed; about 0.5 to 1 agent-day). Rules: ER-9 (overlap), ER-8, ER-15,
ER-33, ER-36, ER-51; builds on ADR 0080 (CI at the speed of the critical path) and ADR 0102 (a landing push takes the heavy slot).

## Problem

A tree could not reach CI until the whole local push gate passed, and on a busy Mac that gate queues for the heavy slot behind other
agents. On 2026-11-22 CI sat idle for over three hours behind it. ER-9 already says what to do: run the fast static gates, dispatch CI,
run the slow local checks beside it, and cancel the run at once if one fails. Nothing did that.

## Decision

1. **Covered pushes.** A push whose every ref is `refs/heads/wip/*` or `refs/heads/trial/*` (read from every line of git's pre-push
   input, so a mixed push or a second landing ref of the same commit is not covered). Every other push, `train/*`, `main`, tags and
   feature branches, runs the full gate exactly as before; the profile has no switch that reaches them.
2. **Before the push (fast rows)**: fmt, lockfile `--locked`, commit-lint, conformance, gates-register, ipc-endpoints, pitwall-deny,
   pitwall-scripts and the host Clippy of touched workspaces. **After the push (slow rows)**: doctest, Windows (xwin) and Linux Clippy,
   conformance-tests, dyno-wire-fixtures and the member audits. The fast set is an allow-list: a row added later is slow, never nowhere.
   Measured from the gate's own `step-history.jsonl` (2415 steps, 2026-11-22; passed warm steps, median / 90th percentile in seconds):
   fmt 3.1 / 9.7, lockfile 1.5 / 7.1, commit-lint 1.5, conformance 46.8 / 131, gates-register 7.3 / 17, ipc-endpoints 3.0 / 9.8,
   pitwall-deny 1.7 / 3.1, pitwall-scripts 1.5 / 4.0, host Clippy 6.8 / 42 (cold 143 / 449); slow: conformance-tests 92.7 / 237,
   doctest 38.6 / 202, Linux Clippy 18.8 / 66 (up to 710), xwin Clippy 14.8 / 99 (cold 346 / 859), dyno-wire-fixtures 82,
   member-audits 36 / 60. The fast rows run in parallel, so they take about the time of conformance (47 s median); the slow rows are
   about 250 s of work plus the queue for the heavy slot.
3. **The slow rows are one owned process.** The hook starts `./dev check --pre-push --split-phase slow --root <repo>` through
   `paddock_proc::spawn_detached`, with the repository root as its working folder (never the log folder, which is inside `.git`:
   there git finds no checkout and the child died at once, the first review's P1-1). The flag says `split` because "trial" already
   means `ci dispatch --trial` and the trial merge; it is honoured only for a push the profile covers. The child has no pipe of git's,
   which would hold the push open; its input and output are in the push's log folder (`slow-phase.stdin`, `slow-phase.log`, the rows'
   logs beside the fast rows'), whose owner file names the child so a prune never takes it. The slow phase always measures a
   throw-away worktree of the pushed commit, never the live tree. The starter keeps its identity (pid and start time; a start time
   that cannot be read is an error, and 0 never counts as alive) in `<state>/trial/<sha>.slow.json` (ER-51). One record per commit says running, passed, failed or
   not measured, the first row at fault and the next step, and a deadline derived from the gate's own limits (run budget, queue
   allowance, row cap, and 5 minutes).
4. **The verdict is a commit status.** The slow rows' record carries the Gitea server and repository (from the push's remote URL), and
   its state is posted on the commit as the status `paddock/slow-rows`: `pending` while they run, then `success`, `failure` (a row
   failed) or `error` (not measured, interrupted, superseded, dead, overdue). The commit has to be on the server first, so the child
   retries the pending post while it works and the final post for a minute. Any reader of CI for a sha refuses it when that status
   exists and is not `success`: the shared verdict reader (`ci verdict`, the pre-push known-commit shortcut, the landing checks that
   ask it) turns a green run into red, and a status it cannot read into not measured; the push gate also refuses from this checkout's
   own record without a server. So a green CI run that finished before the slow rows failed cannot let a train skip its full gate (the
   first review's P1-2). GitLab merge trains and GitHub's merge queue take required checks from the server in the same way.
   Without `GITEA_TOKEN` or a Gitea remote URL at the fast phase, the profile is not used: the full gate runs and the log says why.
5. **Dead or overdue children are settled by whoever comes next.** One shared sweep (`trial_guard::sweep`) runs at every push gate
   start, `ci dispatch` and `ci verdict`: a record still running whose process is gone (pid plus start time) or past its deadline
   gets its registered run cancelled and its status posted as `error`, and is rewritten as not measured only when both worked, so a
   failure leaves it running (it still refuses locally) for the next sweep. A new push to the same ref ends the previous push's
   process through its pid and start time (never a bare pid) and settles it as superseded. `<state>/trial/` has an owner note, a limit
   of 200 commits' records (about 100 KiB) and oldest-first eviction that never takes a live holder (ER-51).
6. **Cancel.** `./dev ci dispatch <workflow> --ref wip/<name> --guard --max <d>` is the second step. It reads the ref's head from the
   server and the record; a record that already says failed, not measured or never finished refuses the dispatch (nothing sent,
   nothing to cancel). Otherwise it dispatches, finds the new run by difference against the run list taken before (never "the newest
   run of the branch", ER-51), writes `<sha>.dispatch.json` and settles. The slow process settles when it ends. Each side writes its
   own file first and reads the other's after, so at least one sees both and no lock is needed. Settling cancels the run
   (`POST /api/v1/repos/<repo>/actions/runs/<id>/cancel`) when the rows failed, were not measured, were interrupted, the process is
   gone without a result, or the deadline passed, and prints one `::error::` with the row, cause and next step. ER-9 allows the
   cancel with no further approval. A run that cannot be found is reported as not guarded, loudly.
7. **Never green by skipping.** The fast phase says it is not a full pass. Neither phase writes a pass record (so `known::find` never
   skips a later train gate on the strength of a partial run). A trial whose slow rows never finished is not evidence: `judge` has no
   pass for a running record past its deadline or whose process is gone, and the commit status says so to every other reader. A train still runs its full gate on the train push and its
   full CI run before landing (ER-8, ER-15). The profile only starts CI sooner.
8. **`--trial <branch>` is unchanged.** It merges a branch onto the train in its own clone and pushes `trial/<branch>` with hooks off,
   so no local row runs for it at all (ADR 0098); `--guard` conflicts with it, because the merge commit is not a commit the slow rows
   measured. The documented flow for a hand push is two commands:
   `git push origin HEAD:refs/heads/wip/<name>` and `./dev ci dispatch <workflow> --ref wip/<name> --guard --max 30m`.

## Common practice (ER-37)

GitHub merge queue and GitLab merge trains start the combined run first and let required checks run beside it; a failing check removes
the entry and cancels its run. Gerrit with Zuul starts jobs speculatively and cancels those whose predecessor failed. The profile is
the smaller version of that for one developer: no queue service, two JSON files and one background process. The common-practice
option (leave the gate as it is) is what stalled CI for three hours.

## Not covered, on purpose

The cancel API call was written from the run objects' `cancel_url` (the recorded Gitea JSON in `tests/ci-triage`) and is tested only
against a fake dispatch; its first live use is the first guarded dispatch. The conflict with the stall fix on
`fix/push-gate-queue-stall` (5652345ae) and the result reuse on `feat/check-result-reuse` (f67b7f6a1) is in `exec.rs`'s `Exec` trait
(two methods added at its end) and in `command_in`; neither branch is merged here.
