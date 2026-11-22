# Scheduled CI runs

Scheduled workflows run at night, local time UTC+8 (cron is UTC), staggered so they do not overlap on the three Linux
runner slots. Matrix jobs are capped with `strategy.max-parallel` so one Linux slot stays free for release and branch runs
(rule 50; trigger: Debrief's fuzz run at 10:23 local held all three slots for about 30 minutes).

**A train in CI skips the night (user, 2026-11-12).** Every scheduled workflow first asks `./dev ci schedule-skip` (the
`schedule-skip` action, or the `plan` action's step): while a train's `workflow_dispatch` run on `train/*` is queued or in
progress, the scheduled run ends early with one `::notice::skipped: train <branch> in CI ...` line, runs nothing and
writes no green marker; it runs again on the next schedule after the train lands. Such a run is not measured, never green
(ADR 0093, amendment 2026-11-12), and the nightly's installers verdict warns so. A workflow is never skipped twice in a row,
nor when its last measured scheduled run is older than 36 h (`debrief-mutants`, weekly: 192 h), so a long train costs at
most one skipped night. When the Actions API cannot be read the run proceeds (fail open) and says so in its summary. A run still listed as running after 24 h is named but not counted, so a lost runner cannot stop every schedule.

**A push to main reuses a green train run (ER-49, user, 2026-11-21).** `ci.yml` and `debrief.yml` ask `./dev ci reuse` first for a push
to main: when `./dev ci verdict`'s reader finds a complete green run of the same workflow on that exact commit (the train's
dispatch, when main fast-forwards to its head) and no job log of it has a FLAKY test, the push run ends early, runs no job and
logs `plan verdict: complete=false`; its summary names the reused run. The asking run is left out of the runs it reads. A run
that was red, partial (a base or `since_run` dispatch), or green with a FLAKY test is not reused, so the post-landing run on main
is a real full run that `tag.yml`'s reader can use (a train that was red only with FLAKY tests needs it). Anything that cannot be
read (the list, a job, a log, the token) runs the commit (fail open to running, never to skipping).

**"Complete" is one scope: a push to main's (ER-15, 2026-11-21).** `plan verdict: complete=true` of `ci.yml` means the run had the scope
of a push to main, WebKit included: a push, or a dispatch with the `webkit` input and no `app`. A dispatch with `app` is that app only
and never complete. A train's full dispatch
without WebKit logs `complete=false`, so `./dev ci verdict` does not count it and `ci reuse` does not skip the main push run for it;
otherwise a tag could read a green verdict that never ran WebKit (the same class as run 8335 and the schedule-skipped and
rerun-only runs counted green on 2026-11-12). `./dev ci dispatch ci.yml --ref train/...` therefore adds `webkit=true` itself for a full dispatch (no `base`, no `since_run`); an explicit
`--input webkit=false` wins, and a member run or a rerun is sent as asked. That is what lets a landed train's push to main reuse the
train's run (ER-49). `./dev ci verdict`'s hint of
the last fully tested commit asks the same reader about each of main's newest commits, so a push that reused a train run
(partial itself) still names the commit the train tested.

**Member runs test only the reached packages (2026-11-21).** A dispatch with a `base` and no `since_run` (a member or a fix round)
passes `./dev plan --ci` the event, and the plan narrows nextest to the packages the change reaches: the app legs' `filter`, and
`crates_filter` and `paddock_dev_filter` for the host legs of `crate` and the tests of `paddock-dev`. The Linux crate tests run
whole under coverage in `crates`. A push to main, a tag, the nightly, a manual run with no base and an app's own gates are never
narrowed: the full run on the final head catches what the narrowed ones missed (ER-8). A project is narrowed only when every reason the plan
gives for it is a Cargo reach (`cargoOnly` in the plan): a UI, installer, lockfile or script reason runs its tests whole. A half of an app's
nextest that selects no test is skipped with a notice, never passed.

**Proxmox backup window.** Starts at 00:00 local (UTC+8; 16:00 UTC), stated by the user on 2026-11-10. The end is not
known; 04:00 local (20:00 UTC) is assumed until evidence says otherwise. The Windows CI VM crawled from 00:52 local on
2026-11-09/10 (memory note `windows-ci-proxmox-backup`). Rules:

- A workflow with a Windows leg neither starts in 00:00-04:00 local nor is likely to run into it. Because the end is
  only assumed, the Windows starts keep an hour of margin: nothing starts before 05:00. These times will be reset once the
  real end is read from the Proxmox task log.
- Linux-only and macOS-only workflows START between 22:00 and 23:59 local or after 04:00. debrief-fuzz runs until about
  02:30 local (estimate).
- Open point: whether the Linux runners are affected by the backup at all is unknown; the Linux-only runs that cross
  midnight (fuzz, perhaps mutants) assume they are not.

Typical durations are from the last five scheduled runs (Gitea API, 2026-11-06 to 11-10; the failed runs count, they
ran the same jobs).

The Linux runner runs 3 jobs at once (3 Linux slots, declared in `conformance/ci/runner-slots.json`).

| Workflow | Cron (UTC) | Local (UTC+8) | OS legs | Typical duration | Linux slots at most |
| --- | --- | --- | --- | --- | --- |
| debrief-mutants | `3 12 * * 6` | Saturday 20:03 | Linux | not measured (never ran on schedule) | 1 |
| debrief-bench | `3 14 * * *` | 22:03 | Linux | 5-13 min | 1 |
| canary | `23 14 * * *` | 22:23 | Linux | 2-5 min | 1 (`max-parallel: 1`) |
| debrief-corpus | `43 14 * * *` | 22:43 | Linux | 0-21 min | 1 |
| service-smoke | `17 15 * * *` | 23:17 | macOS VM | about 22 min | 0 |
| debrief-fuzz | `13 15 * * *` | 23:13 | Linux | 77-90 min with 6 shards at once; about 3 h at 2 at a time | 2 (`max-parallel: 2`), alone in its window |
| nightly | `3 21 * * *` | 05:03 | Linux, Windows, macOS | 25-123 min; its `generators` job (`./dev gen check --all`, every registry command, ER-40) measured once in the paddock-ci:v2 container on a Mac (amd64 emulated, 4 CPUs, 12 GB): 92 min, 32 checks current, exit 0, 2026-11-12; added to `mark` after the first green run (record run id: not yet) | uncapped for the rest (reusable ci.yml and many jobs); its five fuzz calls are chained in two chains, so at most 2 at once |
| debrief | `3 22 * * *` | 06:03 | Linux, Windows, macOS | 37-147 min | uncapped (parallel jobs, 4 e2e shards) |
| dev-tool | `33 21 * * *` | 05:33 | Linux, Windows, macOS | not measured | 1 (Linux leg) |
| tag-rehearsal | `3 1 * * *` | 09:03 | Linux, Windows, macOS | not measured (never ran; first green run id: not yet) | 1 (Linux), the four apps chained |

The Linux-only block (22:03-23:13) is serial so that fuzz, which takes two slots, runs after the small jobs have ended.
nightly and debrief cannot be capped by `max-parallel` (many unrelated jobs); only the stagger and the runner's capacity
limit them. Gitea counts runner slots per caller, so `max-parallel` inside the reusable `app-fuzz.yml` does not cap
five separate callers: nightly chains them with `needs:` instead.

## Tag rehearsal

`tag-rehearsal.yml` (user, 2026-11-20) runs a tag's release path every day on main's head, and by hand, with no tag, no release and
nothing published: the tag checks in dry-run mode (`request-tag.mjs --dry-run true`, the tag gates), then each app's installers with
`PADDOCK_TAG_REHEARSAL=1` (is-tag-run.mjs answers "tag": pre-sign, signing with a throw-away key made in the job, check-bundle's
release=1, fresh caches, C99) and `rehearse-release` (release notes and latest.json made and read back). The four apps with an
`app-release.yml` caller (Dyno, Pitcrew, Pitwall, Spotter) run one after the other. The named differences from a real tag are listed in
the workflow's header; Debrief has tag checks only (no rehearsal caller yet). The 09:03 slot follows nightly and debrief. Report-only:
nothing waits on it. Its schedule is **disabled in Gitea** like the other schedule-only workflows (scheduled-ci-paused) and is enabled
only after its first green run, whose id goes in the table row above and which must be a dispatched run that really ran (a night skipped for a train skips the verdict job: it is neutral, never green, and never the recorded run), its measured duration fits the slot (ER-24: the chain holds no runner reservation between apps; only the start yields to a train), and Dyno 1.2.0 has shipped.

## Dependency audits

External advisories turned main and every train red with no code change twice in 12 hours (RUSTSEC-2026-0543 on chrono, run 8214;
GHSA-fc5q-39xc-vjpf on picomatch, run 8225). As Dependabot and Renovate alerts and a scheduled `cargo deny` do, two questions are kept
apart, both answered by `./dev audit` (`tools/paddock-dev/src/audit`; `cargo deny check advisories` for every `Cargo.lock` with a
`Cargo.toml` and a `deny.toml` beside it, `npm audit --omit=dev` for every `package-lock.json` (severity `high` and above, as the apps' plain steps; `--npm-level` and `--npm-omit-dev` declare both)):

- **Does main have a finding?** The `audit` job of nightly.yml runs `./dev audit nightly` every night, outside the gate (an advisory
  appears while main is unchanged). One `::error::` per advisory names the advisory, the dependency path and the decision step: an
  exception with a reason and a review date (`deny.toml` `ignore` for cargo; `scripts/npm-audit-allow.json` for npm, the family's list that
  `scripts/npm-audit.mjs` also reads, which `./dev audit` reads in the same format: an approved entry warns, one on its review day counts as a finding
  again), which needs the user's approval (ER-46), or a fix. The failed job is the nightly's failure path.
- **Does this change add one?** The `audit` job of ci.yml runs `./dev audit compare --base <merge base ref>` on every change. It audits the
  merge base's lockfiles with the same tools, only when the change has a finding. An advisory main has too is a `::warning::` ("already
  present on main, decided there"); only an advisory the change introduces fails. No list of accepted findings is kept, so none can grow
  silently (ER-46).
- **Not measured.** A tool that cannot run, an offline advisory database or registry, a missing merge base or an unreadable answer is
  reported as NOT MEASURED (exit 6 when enforced), never as a pass (ER-33).

**The switch (ER-41).** Both jobs are report-only until their first green run on the real tree: `--enforce` is passed only when the
repository variable `PADDOCK_AUDIT_ENFORCE` is `1` (Gitea: repository settings, Actions, variables). Until then the findings print as
`::warning::report-only, would fail: ...` and the exit is 0. Record the first green run id of each job here when the variable is set
(nightly `audit`: not yet; ci `audit`: not yet), then add nightly's `audit` to `mark`. The plain audit steps (`cargo deny check` in the
crates, tauri and paddock-dev jobs, `npm audit` in debrief.yml) stay blocking until then, so nothing gets weaker; after it they move to
`./dev audit compare` (one job per question, no copied logic).

**The push hook (`./dev check --pre-push`).** Its `pitwall-deny` row runs `cargo deny` for licences, bans and sources on every push, and the
`advisories` check only when the push changes a `Cargo.lock`, a `Cargo.toml` or the `deny.toml` the workspace depends on (its own, or a
shared crate's manifest under `crates/`), measured from the same base as the rest of the push (`pre_push/deny.rs`, one value that
decides the arguments and the printed line). A push that skips them prints `advisories skipped` and names the nightly audit that covers
them. A run with a file list the caller gave (`--files`) is not measured from the base, so it runs them (not measured never passes, ADR 0089). Reason: an outside advisory (lettre 0.11.23,
2026-11-18) turned every push of every branch red, also those that touch no dependency.
