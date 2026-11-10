# 93. Every scheduled CI check has an owner, and a red one cannot be ignored

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10, asked for by the user under ER-21.
  Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-31, ER-33 / rule 71, ER-36; ADR 0022 (developer tools are Rust in `./dev`); ADR 0041
  (its tag-checklist row, line 208: "the last ... starved nightly of the app's tests has no open failure, or each is
  waived by the user", the hand-run precursor of §5); draft ADR 0047 on `origin/docs/redesign-adrs-7-9` (line 22:
  scheduled runs always run, never reuse a green run); ADR 0060 (`docs/gates.toml`: owner, `review_by` and `approved`
  per entry); ADR 0080 (the nightly is "the net" for D11, D12 and §3.3, its §3.1 table line 158; §4.4 lines 313-316: a
  slip "is found by the next nightly ... which then turns red"; its §3.1 row for `debrief-bench.yml`: "fix or quarantine
  with an owner: a check that always fails tells nothing"); ADR 0089 (accepted on `origin/docs/adr-not-measured`: one
  `Outcome`, exit codes 5 findings, 6 not measured, 7 check broken, `release-verdict` and `measured-on` jobs); RT-8
  `./dev ci flakes` on `origin/feat/rt8-ci-flakes` (`ci_runs/client.rs`, `ci_runs/logfind.rs`), the reader this ADR
  reuses; `docs/ci-schedules.md` and the decision of 2026-11-10 (scheduled work at night only, the Proxmox backup
  00:00-04:00 local, Windows starts not before 05:00).
- Number: 0093 was free on every `origin` branch on 2026-11-10 (highest file `0091`, on `origin/docs/adr-evidence-heads`;
  0092 is being drafted for private files in its own worktree and has no file yet). Check id: C105 (ADR 0091) is the
  highest on any `origin` branch; this ADR names its check **C107** and leaves C106 to ADR 0092 in case it needs one.
- Evidence: the Gitea Actions API of this instance (version 28.3.0), read-only, on 2026-11-10: `runs?event=schedule`
  (119 runs, 2026-10-25 to 2026-11-10, all pages), the job lists of runs 7172, 7329, 7565, 7684, 7685, 7686, 7802, 7857,
  7861 and 7870, the logs of the failed jobs of runs 7685 and 7870, the issue list and labels of `decryp7/paddock`, and
  `/api/v1/user` (to learn which account agents' token is). Times from the API are UTC; local is UTC+8. Git facts from
  `git log` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3`. No user data
  was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/docs/adr-not-measured 0df6835a9
origin/docs/adr-evidence-heads f2bdba1df
origin/feat/rt8-ci-flakes 0d40eafed
origin/docs/redesign-adrs-7-9 4b0633e73
origin/feat/rt5-check-host ec8530a95
engineering-rules main ca4124bdc
```

## Context: the incidents

Scheduled runs per workflow, 2026-10-25 to 2026-11-10 (one run a night; cancelled runs and two runs with an unset start
time left out of the streaks):

| Workflow | Runs | Red | Streak | What was red | Owner today |
| --- | --- | --- | --- | --- | --- |
| `debrief-bench.yml` | 14 | 13 (1 cancelled, never green) | 10-29 to 11-10, every night (runs 6954 to 7870) | job `store benchmarks (10M events)`; on run 7870 the test `worker::imports_and_queries_within_the_budgets` logged "peak RSS importing 776 MB, indexing 527 MB" against `IMPORT_BUDGET_MB` 768 (`apps/debrief/crates/debrief-store-worker/tests/it/worker.rs:313`). Its cause is investigated separately | none named; ADR 0080 (2026-11-09) asked for one, two more red nights followed |
| `debrief-fuzz.yml` | 14 | 11 | 10-28 to 11-08 | shards 5/6 and 6/6 (run 7684). The fix `a5aca536d` ("cut the schema error message on a char boundary") was committed 2026-11-05 13:29 +08:00 and reached `main` with `b51c72509` on 2026-11-08 23:30; the first green night was 11-09 (run 7806) | none |
| `service-smoke.yml` | 16 | 13 | 10-26 to 11-08 | job `update smoke in a macOS VM` (run 7685: "N+1's agent is not 0.1.1", "leftover VMs") | none |
| `nightly.yml` | 16 | 10 (3 cancelled, 3 green) | red on 10 of the 13 nights it finished | run 7861: `crate (macos-latest)`, `crate (windows-latest)`, `paddock-dev (macos-latest)`, `paddock-dev (windows-latest)`, `fuzz (paddock-ipc)`, `fuzz (shard 1)`; run 7802: `crate (macos-latest)`, `app dyno (macos-latest)`, `fuzz (paddock-ipc)` | the coordinator, implicitly |
| `debrief.yml` (schedule) | 14 | 8 | 11-09 and 11-10 now | run 7857: `rust tests`, `rust tests (macOS)`, `real-app smoke (macos-latest)` | Debrief, implicitly |
| `debrief-mutants.yml` | 2 | 2 (never green) | 11-01 and 11-08 | job `mutants` | none; `docs/ci-schedules.md` says "not measured (never ran on schedule)", which the API contradicts |
| `canary.yml`, `debrief-corpus.yml` | 17, 14 | 1, 0 | none | | |

**Known reds were paid for again.** 13 scheduled runs repeated a red on the same commit as the night before (`main`
unchanged), 407 wall-clock minutes in all (fuzz runs six shards, so more job-minutes). They ran at night, when the lanes
are free, so the cost is attention more than capacity: nothing read them.

**The tracker exists and went stale.** The repository has issues on, with labels `app:<app>`, `flaky`, `pattern` and
`release-blocker`. All 15 issues were opened on 2026-10-30; 13 have no comment and no update since that day, the
"Flaky-test ledger" (#3) was last touched on 10-31 and one Dyno flake (#2) on 11-08. An issue nobody is shown is the same
as no issue.

**Agents act as the user.** `/api/v1/user` with the `GITEA_TOKEN` agents use answers `decryp7`, an administrator, the
user's own account. Anything an agent writes in Gitea (an issue, a comment, a label) is indistinguishable from what the
user writes. A waiver recorded in Gitea today therefore proves nothing about who gave it.

**The net has a hole.** ADR 0080 trades per-run coverage for the nightly (D11, D12, §3.3), and §4.4 says a slip "is found
by the next nightly ... which then turns red". A nightly that is red on 10 of 13 nights does not turn red; it stays red.
A new failure in it looks like the old ones, so the slip ADR 0080 counts on catching is not seen.

## Pattern

**A check that runs on a schedule reports to nobody.** A branch run has an author waiting for it and a landing that
cannot happen while it is red; a scheduled run has neither. Its red therefore persists until someone happens to look
(bench: 13 nights; service smoke: 13; fuzz: 11, three of them with the fix written but not on `main`), a new failure hides
behind an old one, and a fix waits on a branch because nothing says it is the one thing standing between a check and
green. The same class at other levels: ADR 0080's own "fix or quarantine with an owner" line that changed nothing; the
15 issues of 2026-10-30; the hand-kept tag-checklist row of ADR 0041.

**Invariant.** Every scheduled job has a declared owner and a declared set of apps it covers. A red or unmeasured
scheduled job is an open issue within a day, on a list the coordinator sees every morning, and an app whose covering
check has been red for more than 24 h cannot be tagged unless the user, and only the user, has waived that failure.

## Decision

### 1. A register of scheduled jobs: owner and coverage (C107)

`conformance/ci/scheduled.toml`, one entry per scheduled workflow and, inside it, per job (matrix legs by their job
name pattern):

```toml
[[workflow]]
file = "debrief-bench.yml"
owner = "debrief"                 # an app id, or "family" (the coordinator) for shared workflows
period_days = 1                   # from the cron; weekly = 7

[[workflow.job]]
name = "store benchmarks (10M events)"
covers = ["debrief"]              # app ids, "packages" for the v* shared packages, or [] with `why`

[[workflow]]
file = "nightly.yml"
owner = "family"
period_days = 1

[[workflow.job]]
name = "crate (*)"
covers = "dependents"             # every app whose dependency set contains a crate the job tests (ER-31 (a))

[[workflow.job]]
name = "paddock-dev (*)"
covers = "release-path"           # the release path blocks every app (ER-31 (b))
```

`covers = "dependents"` is resolved by the same planner that ADR 0080 §3.3 and ER-31 (a) use (the lockfile and the
package graph), so a shared crate's red blocks only the apps that ship it. `covers = []` needs a `why` (a job that tests
something no app ships, such as a demo). The check **C107** (a `./dev check` in the RT-5 host) fails when a workflow with
`on.schedule` or a job in it has no entry, when an entry names a workflow or job that no longer exists, when an owner is
not an app id or `family`, and when `period_days` disagrees with the cron; a planted workflow without an entry is its
positive control (ER-33). `docs/ci-schedules.md` gets its owner column from the register, not by hand.

### 2. `./dev ci scheduled sweep`: one issue per red, kept current

A Rust module `tools/paddock-dev/src/ci_runs/scheduled/` (ADR 0022; one CLI variant), on RT-8's `Client` and log
readers. It reads every scheduled run since its last sweep (and at least the last `period_days` of each workflow), and
for each register job:

- **Signature.** The failing tests read from the log by RT-8's `logfind` (nextest, Playwright, vitest), sorted, as
  `test:<binary>::<path>`; otherwise the failing step's name and its first `::error::` or `panicked at` line with digits,
  hex runs and temp paths replaced by `N`, as `step:<name>:<line>`. A log it cannot account for gives
  `unclassified:<step>`, never "no failure" (ADR 0089). A job that was cancelled, never started, or whose workflow did not
  run within `2 × period_days` gives `not-measured:<why>` (ER-33).
- **Issue.** Keyed by (workflow, job, signature), held in a machine-readable first line of the issue body
  (`<!-- scheduled: debrief-bench.yml | store benchmarks (10M events) | test:it::worker::imports_and_queries_within_the_budgets -->`),
  labels `scheduled-red`, `app:<owner>` and one `covers:<app>` per covered app. The first red opens it (title: job and
  signature); each later red of the same key adds one comment (run id, commit, first line) only when the commit changed
  or once a day, so a nightly does not write fifty comments. A different signature for the same job opens its own issue
  and links the other.
- **Close.** A green run of the job, scheduled or dispatched on `main`, closes every open issue of that (workflow, job)
  with the run id. A red with a different signature does not close the old one: both stay until a green run, because a
  new failure can hide an old one.
- **Fix link.** `./dev ci scheduled fix <issue> <commit>` records a fix commit on the issue; the sweep and the digest then
  report "fix `<sha>` not on `main` for N h" until `git merge-base --is-ancestor` says it is (the fuzz case).

It is idempotent (re-running it adds nothing), read-mostly (writes only issues and comments), and exits per ADR 0089:
0, 5 when it found reds, 6 when it could not read a run or log, 7 when the register is broken.

### 3. A daily digest for the coordinator

`./dev ci scheduled digest` prints, per app and for `family`: open `scheduled-red` issues with age, owner and covered
apps; the ones past 24 h and which tags they block; fixes not yet on `main`; waivers and their expiry; and
`not-measured` jobs. A small scheduled workflow `scheduled-digest.yml` runs the sweep and then the digest at **09:13
local** (01:13 UTC), after the last night run (`debrief.yml` starts 06:03 and takes 37 to 147 min), Linux only, one
slot, and writes the digest into one pinned issue "Scheduled checks: digest" (body replaced, not a new comment). The
coordinator's session start reads it (`./dev ci scheduled digest` locally gives the same text), and every coordinator
batch report carries its first line. The digest workflow is itself in the register (`owner = "family"`), and a digest
older than 30 h is reported as stale by `./dev ci scheduled digest` (a digest that silently stopped is the same class).

### 4. The tag gate

`./dev ci scheduled gate --app <app> --commit <sha>` refuses (exit 5) when any open `scheduled-red` issue that covers the
app was opened more than 24 h ago and has no valid user waiver (§5), and returns 6 (not measured) when it cannot read the
issues, so a broken gate never passes. It runs:

- in each app's release workflow on a tag (`<app>-release.yml`, `debrief.yml` on `debrief-v*`, `publish.yml` for `v*` as
  app `packages`), as a step of the gates before anything is built or published, beside ADR 0089's `measured-on`; this is
  the authoritative place, so a tag pushed by hand is still refused;
- in the steward's tag checklist and in `./dev release tag` once that exists, so the refusal is seen before the tag is
  pushed (ER-9). ADR 0041's hand-run row becomes a reading of this gate.

The 24 h runs from the issue's first red, not from its last update. The gate does not block landings on `main` or trains
(a fix must be able to land) and does not block branch runs.

### 5. Waivers are the user's, and provably so

A waiver is a comment on the issue in the form `waive: <reason>; until: YYYY-MM-DD`, at most 14 days ahead, that counts
only when **its author is the user's account and that account is not the one agents and CI use**. To make that true,
agents and CI move to a bot account (`paddock-agent`, write access to issues, read access to Actions, no admin) and the
user's own token leaves agents' environments. A waiver binds to its issue's signature: a new signature needs a new
waiver; an expired waiver blocks again. The gate and the digest print who waived what until when.

Agents never write a `waive:` comment, an `approved = "user"` line or any other text that stands for the user's consent
(the brief rule already in force); a PreToolUse hook refuses an agent's command that posts `waive:` to the issue API, as a
second line, and the author check above is the first.

### 6. Backfill and go-live

At go-live the sweep opens issues for the reds of the day (on 2026-11-10: `debrief-bench.yml`, `debrief-mutants.yml`, the
three red jobs of `debrief.yml` and the red jobs of `nightly.yml`), and their 24 h clock starts at go-live, not at their
first red, so the gate does not land with every app already blocked. `docs/ci-schedules.md` is corrected (mutants ran
twice and failed both times).

## What it gates

The `<app>-vX.Y.Z` tags of every app and the `v*` tags of the shared packages, each only through the scheduled jobs
that cover it. With today's reds and `covers` as sketched: `debrief-bench.yml` and `debrief-mutants.yml` block a Debrief
tag; `nightly.yml`'s `crate (macos-latest)` and `crate (windows-latest)` block every app whose dependency set contains the
failing crates (likely all, since the shell crates are shared); `paddock-dev (*)` blocks every app if the user picks
D3 (a) and none under D3 (b); `app dyno (macos-latest)` blocks Dyno. Nothing else: landings, branch runs and fix rounds are
not gated.

## Parallelism and cost (rule 51)

The sweep reads one runs page per 50 runs (about one a day per workflow, 9 scheduled workflows) and the logs of failed
jobs only, which RT-8 already does for `ci flakes`; on today's numbers about 10 failed jobs a night, so seconds of API time
and one log each. Comments are rate-limited by the "commit changed or once a day" rule. The digest is one issue edit. The
gate is one issue query per app, well under a second. Nothing to parallelise; RT-8's client retries and its not-measured
handling are reused, not copied (rule 55).

## Alternatives

- **A final job in every scheduled workflow that files its own issue.** Rejected for the sweep: it touches nine
  workflows, needs a write token in each, and misses exactly the runs that matter most (cancelled, never started, a
  workflow that stopped being scheduled). The sweep sees them from outside.
- **Quarantine (mark the check `continue-on-error` until fixed).** Hides the red the same way; ADR 0080 §3.4 names
  `continue-on-error` on Debrief's memory budget as the opposite of a quiet check. A waiver with an expiry is the honest
  form of it.
- **Block landings on `main` while a scheduled check is red.** Stops the fix from landing; the release is the decision
  point that matters (ER-31's frame).
- **Skip a scheduled run while `main` is unchanged and its issue is open.** Would save the 407 wall-minutes above but
  loses the flake signal and the close signal for infrastructure fixes (a runner repaired, a VM image updated), and the
  runs are at night. Offered as D7, not recommended now.
- **Waivers in a repository file with `approved = "user"`** (as `docs/gates.toml` and ADR 0075). Any agent with commit
  access can write the line, and commits carry the user's identity too; it is an honour system. A user-signed commit
  checked against an `allowed_signers` file would be strong but adds a key step to every waiver; it is the fallback (D5).

## Decisions for the user

- **D1. Owner and coverage register** (§1, C107): (a) a register file `conformance/ci/scheduled.toml` with a check
  (recommended); (b) fields in each app's `paddock.ci.json` (shared workflows have no app file); (c) comments in each
  workflow (cannot be checked well).
- **D2. Who files issues** (§2): (a) one sweep from outside, after the night (recommended); (b) a final job in each
  scheduled workflow.
- **D3. What a red `paddock-dev` or other tool job covers**: (a) every app, because `./dev` is on the release path
  (ER-31 (b)) (recommended); (b) none, because no app ships it.
- **D4. The tag gate** (§4): (a) open for more than 24 h, refused unless waived, enforced in the release workflow and
  checked before tagging (recommended); (b) 48 h; (c) report only in the release summary, no refusal.
- **D5. How a user waiver is recorded** (§5): (a) a `waive:` comment whose author is the user's account, after agents and
  CI move to a bot account (recommended; needs the user to create the bot and swap the tokens); (b) a user-signed commit
  to a waiver file, verified against `allowed_signers` (fallback: strong, more friction); (c) `approved = "user"` in a file
  or a label, trusted (not recommended: agents act as the user today).
- **D6. Not measured blocks** (§2): a covering job that did not run within two periods, or was cancelled, is an issue and
  gates like a red (ER-33) (recommended); or it is reported only.
- **D7. Known reds on an unchanged `main`**: (a) keep running every night (recommended for now; the runs are at night);
  (b) skip while the issue is open and `main` is unchanged, unless dispatched.
- **D8. Waiver limit**: (a) at most 14 days, bound to the signature (recommended); (b) until the next tag of the app.

## Work packages (agent-days)

| WP | What | Days | Needs |
| --- | --- | --- | --- |
| WP-0 | Bot account `paddock-agent` (issue write, Actions read, no admin), tokens swapped in agents' environment and the CI secrets that write issues; labels `scheduled-red`, `covers:<app>` | 0.5 (plus the user's account step) | D5 (a) |
| WP-1 | `conformance/ci/scheduled.toml` for the 9 scheduled workflows and the digest, C107 in the RT-5 host with its positive control, owner column of `docs/ci-schedules.md` generated | 1.5 | D1, RT-5 |
| WP-2 | `./dev ci scheduled sweep` and `fix`: signatures on RT-8's `logfind`, issue key and body line, comment rate rule, close on green, not measured; tests on synthetic run and log JSON (no real logs) and a recorded-API fake | 3 | RT-8 on `main`, WP-0, WP-1 |
| WP-3 | `digest` and `scheduled-digest.yml` (09:13 local, one Linux slot), pinned issue, stale-digest report, coordinator session-start line | 1.5 | WP-2 |
| WP-4 | `gate --app --commit`: 24 h rule, waiver parser and author check, ADR 0089 exit codes; wired into `dyno-`, `pitcrew-`, `pitwall-`, `spotter-release.yml`, `debrief.yml` (tags), `publish.yml`, and the tag checklist; positive controls (a planted red issue refuses, an expired waiver refuses, an agent-authored waiver refuses) | 2 | WP-2, D4, D5 |
| WP-5 | Rule text (a Paddock rule and an ER amendment, below), agent and reviewer briefs, PreToolUse hook refusing an agent's `waive:` post, with its test | 1 | WP-4 |
| WP-6 | Go-live: sweep backfill, clocks started at go-live, `docs/ci-schedules.md` corrected | 0.5 | WP-2 to WP-4 |

**Total about 10 agent-days; a first cut that already gates tags (WP-0, WP-1, WP-2, WP-4, WP-6) about 7.5.** WP-0 is the
only step that needs the user's hands (creating the account). Without D5 (a), WP-4 falls back to D5 (b) and grows by
about one day.

**Proposed rule** (a Paddock rule and an amendment to ER-33, wording for the user's approval): "Every scheduled CI job has
a declared owner and the apps it covers. A red or unmeasured scheduled job is an open issue within a day, shown in the
coordinator's daily digest; an app is not tagged while a covering issue is older than 24 h, unless the user has waived
that failure in a way agents cannot write."

## Risks

- **A broad `covers` blocks every release on one shared red.** That is the intent for shared code an app ships (ER-31
  (b)); the narrowing is `covers = "dependents"`, which is only as good as the planner's graph, and C107 checks the
  entries, not the graph.
- **Signature churn.** A failure whose text changes every night (numbers in it, as the bench's "776/527 MB") would open an
  issue a night without the digit normalisation; the normalisation is tested on the cases above, and an issue still closes
  only on green, so churn adds issues, never hides one.
- **The bot account is a new credential.** It is scoped below the user's (no admin, no settings), and moving agents off
  the user's administrator token is itself a least-privilege gain.
- **Gitea's job-token permissions.** Whether `github.token` in a tag run can read issues on this instance is not
  verified; WP-4 checks it first and otherwise uses the bot's read token as a secret.
- **The gate is only as good as the sweep's last run.** The gate reads issues, not runs; if the sweep stopped, the digest
  says so (stale), and D6 turns missing runs into issues, but a stopped sweep with no digest read is the residual gap.
