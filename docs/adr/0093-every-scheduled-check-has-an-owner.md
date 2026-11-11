# 93. Every scheduled CI check has an owner, and a red one cannot be ignored

- Status: Accepted 2026-11-10 (user), all decisions as recommended (D14 (a)). **Cut 2026-11-11 (user): only the
  coordinator's morning digest of red night runs is kept; the sweep, the issues and the tag gate are dropped 2026-11-11
  (user), and so are the register section, waivers and the report-only period; the text below is history.** History: Proposed 2026-11-10, revision 4
  approved by the independent review at `6270bbbbe`. Drafted at the coordinator's request of 2026-11-10, asked for by the user under
  ER-21. Revision 1 (`34e7eeb00`) was reviewed independently ("changes", 5 P1 and a list of P2s); revision 2 answers it
  (see "Review answers (revision 2)"); its re-review of
  `fe28d4032` ("changes", P1-6 and seven P2s) is answered by revision 3 (see "Review answers (revision 3)"); the re-review of `3bf12da60` ("changes", P1-7
  and one P2) is answered by revision 4 (see "Review answers (revision 4)"). The acceptance commit adds the review's three P2s on
  revision 4 and aligns §4 and §5 with ADR 0097 (proposed, `origin/docs/adr-user-approvals` `d21eddcbf`; see "At
  acceptance"). Nothing here is built yet. ER-23 does not apply (no drawings).
- Amendment 2026-11-12 (user): a scheduled run ends early while a train's `workflow_dispatch` run on `train/*` is queued or in progress (`./dev ci schedule-skip`, the `schedule-skip` action; fails open when the Actions API cannot be read), says so in one `::notice::` line beginning `skipped: train`, writes no green marker, and counts as not measured, never green, for any reader of scheduled results (§2; the tag gate of §4 is cut); it never skips twice in a row nor when the workflow's last measured scheduled run is older than 36 h (a weekly one: 192 h).
- Date: 2026-11-10
- Related: ER-12, ER-21 / rule 40, ER-31, ER-33 / rule 71, ER-36; ADR 0022 (developer tools are Rust in `./dev`); ADR
  0041 (its tag-checklist row, line 208: "the last ... starved nightly of the app's tests has no open failure, or each is
  waived by the user", the hand-run precursor of §5); draft ADR 0047 on `origin/docs/redesign-adrs-7-9` (line 22:
  scheduled runs always run); ADR 0060 (`docs/gates.toml`: owner, `review_by` and `approved` per entry); ADR 0080 (the
  nightly is "the net" for D11, D12 and §3.3, its §3.1 table line 158; §3.3 input-driven skips, for example "Fuzz: a shard
  runs when its target crates changed since its last green night"; §4.4 lines 313-316: a slip "is found by the next
  nightly ... which then turns red"; its §3.1 row for `debrief-bench.yml`: "fix or quarantine with an owner: a check
  that always fails tells nothing"); ADR 0089 (accepted on `origin/docs/adr-not-measured`: one `Outcome`, exit codes 5
  findings, 6 not measured, 7 check broken; §4b the one runner-log reader `runlog`; its WP-2c); RT-8 `./dev ci flakes`
  on `origin/feat/rt8-ci-flakes` (`ci_runs/client.rs`: `TimedJob::is_caller`, `never_ran`); `docs/ci-schedules.md` and
  the decision of 2026-11-10 (scheduled work at night only, the Proxmox backup 00:00-04:00 local, Windows starts not
  before 05:00); ADR 0097 (being drafted: how the user's approval is recorded family-wide), which §5 defers to.
- Number: 0093 was free on every `origin` branch on 2026-11-10 when revision 1 was pushed; 0094 to 0096 have since
  appeared on other branches, none is 0093. Check id: C105 (ADR 0091) is the highest on any `origin` branch; ADR 0092 (on
  `origin/docs/adr-private-files`) takes C106, so this ADR names its check **C107**.
- Evidence: the Gitea Actions API of this instance (version 28.3.0), read-only, on 2026-11-10: `runs?event=schedule`
  (119 runs, 2026-10-25 to 2026-11-10, all pages), the job lists of runs 7172, 7329, 7565, 7684, 7685, 7686, 7802, 7857,
  7861 and 7870, the logs of the failed jobs of runs 7685 and 7870 and of job 34160 (run 7861), the issue list and labels
  of `decryp7/paddock`, and `/api/v1/user`. Run times from the API carry `+08:00` (local); job and log times are UTC
  (`Z`). Git facts from `git log` and `git merge-base --is-ancestor` after `git fetch`; lockfile facts from `grep` over
  every `Cargo.lock` on the base. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3` (`67a633ef3`; the train has since
  moved to `c151422a4`, which changes only `tools/paddock-dev/tests/it/golden.rs`, see the nightly row). No user
  data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/docs/adr-not-measured 0df6835a9
origin/docs/adr-evidence-heads b32655fef
origin/feat/rt8-ci-flakes 0d40eafed
origin/docs/redesign-adrs-7-9 4b0633e73
origin/feat/rt5-check-host ec8530a95
origin/docs/adr-private-files e2f7e946b
origin/docs/adr-user-approvals d21eddcbf
engineering-rules main ca4124bdc
```

## Context: the incidents

Scheduled runs per workflow, 2026-10-25 to 2026-11-10, local dates (one run a day; cancelled runs and two runs with an
unset start time left out of the streaks). The schedules on `main` still start these runs in the late morning and
midday; the night-only schedules of 2026-11-10 are on the train.

| Workflow | Runs | Red | Streak | What was red | Owner today |
| --- | --- | --- | --- | --- | --- |
| `debrief-bench.yml` | 14 | 13 (1 cancelled, never green) | 10-29 to 11-10, every run (6954 to 7870) | job `store benchmarks (10M events)`; on run 7870 the test `worker::imports_and_queries_within_the_budgets` logged "peak RSS importing 776 MB, indexing 527 MB" against `IMPORT_BUDGET_MB` 768 (`apps/debrief/crates/debrief-store-worker/tests/it/worker.rs:313`). Its cause is investigated separately | none named; ADR 0080 (2026-11-09) asked for one, two more red runs followed |
| `debrief-fuzz.yml` | 14 | 11 (2 cancelled, 1 green) | 10-28 to 11-08 | shards 5/6 and 6/6 (run 7684). The fix `a5aca536d` ("cut the schema error message on a char boundary") was committed 2026-11-05 13:29 and reached `main` with the merge `af929a1e3` (2026-11-08 11:26, "Merge debrief/1.2-limits-adopt"); the first green run was 7806 on 11-09; run 7863 on 11-10 was cancelled, so the check is unmeasured today | none |
| `service-smoke.yml` | 16 | 13 | 10-26 to 11-08 | job `update smoke in a macOS VM` (run 7685: "N+1's agent is not 0.1.1", "leftover VMs") | none |
| `nightly.yml` | 16 | 10 (3 cancelled, 3 green) | red on 10 of the 13 runs that finished; 9 to 537 min | run 7861: `crate (macos-latest)`, `crate (windows-latest)`, `paddock-dev (macos-latest)`, `paddock-dev (windows-latest)`, `fuzz (shard 1)`, and the callers `everything` and `fuzz (paddock-ipc)` (no runner, no steps); run 7802 also `app dyno (macos-latest)`, red only that day. The train's `c151422a4` merges `fix/golden-live-windows-timeout` (`tools/paddock-dev/tests/it/golden.rs`), which bears on the `paddock-dev (windows-latest)` red; it is not on `main`, so the next nightly cannot show it yet | the coordinator, implicitly |
| `debrief.yml` (schedule) | 14 | 8 | 11-09 and 11-10 now; 0 to 581 min | run 7857: `rust tests`, `rust tests (macOS)`, `real-app smoke (macos-latest)` | Debrief, implicitly |
| `debrief-mutants.yml` | 2 | 2 (never green) | 11-01 and 11-08 | job `mutants` | none; `docs/ci-schedules.md` says "not measured (never ran on schedule)", which the API contradicts |
| `canary.yml`, `debrief-corpus.yml` | 17, 14 | 1, 0 | none | | |

`dev-tool.yml` (05:33 local, Windows leg) is scheduled on the train only (it does not exist on `main`), so it has no
scheduled runs yet; it is in the register from the start. With it there are 9 scheduled workflows (`canary`,
`debrief-bench`, `debrief-corpus`, `debrief-fuzz`, `debrief-mutants`, `debrief`, `dev-tool`, `nightly`,
`service-smoke`); with this ADR's digest workflow, 10.

**The first `::error::` line is often not the error.** In job 34160 (`fuzz (shard 1)`, run 7861) the first lines
containing `::error::` are script text echoed by the runner (line 220: `command -v rustup >/dev/null || { echo
"::error::rustup is not installed ..."`; lines 245, 292, 317, 319, 810 the same shape). The real annotation is line 15084,
`<timestamp> ::error::Crashed: plugin_request (reproducers in the crashes-shard artifact)`.

**Known reds were paid for again.** 13 scheduled runs repeated a red on the same commit as the run before (`main`
unchanged), 407 wall-clock minutes in all (fuzz runs six shards, so more job-minutes). They ran when the lanes were
mostly free, so the cost is attention more than capacity: nothing read them.

**The tracker exists and went stale.** The repository has issues on, with labels `app:<app>`, `flaky`, `pattern` and
`release-blocker`. All 15 issues were opened on 2026-10-30; 13 have no comment and no update since that day, the
"Flaky-test ledger" (#3) was last touched on 10-31 and one Dyno flake (#2) on 11-08. An issue nobody is shown is the same
as no issue.

**Agents act as the user.** `/api/v1/user` with the `GITEA_TOKEN` agents use answers `decryp7`, an administrator, the
user's own account. Anything an agent writes in Gitea (an issue, a comment, a label, an issue's open or closed state) is
indistinguishable from what the user writes. Agents also run as the user's macOS account, so a different token alone does
not change that (§5).

**The net has a hole.** ADR 0080 trades per-run coverage for the nightly (D11, D12, §3.3), and §4.4 says a slip "is found
by the next nightly ... which then turns red". A nightly that is red on 10 of 13 runs does not turn red; it stays red. A
new failure in it looks like the old ones, so the slip ADR 0080 counts on catching is not seen.

## Pattern

**A check that runs on a schedule reports to nobody.** A branch run has an author waiting for it and a landing that
cannot happen while it is red; a scheduled run has neither. Its red therefore persists until someone happens to look
(bench: 13 runs; service smoke: 13; fuzz: 11, three of them with the fix written but not on `main`), a new failure hides
behind an old one, and a fix waits on a branch because nothing says it is the one thing standing between a check and
green. The same class at other levels: ADR 0080's own "fix or quarantine with an owner" line that changed nothing; the
15 issues of 2026-10-30; the hand-kept tag-checklist row of ADR 0041.

**Invariant.** Every scheduled job has a declared owner and a declared skip class. A failing test or step in a scheduled
run is shown to the coordinator within a day, and an app whose shipped code a scheduled failure covers cannot be tagged
once that failure has been red for more than 24 h, unless the user has waived it with a step that needs the user present.
The verdict is computed from the Actions record and git, never from state an agent can edit.

**Residual risk (not closed here).** The release workflow, and with it the gate step, runs from the tagged tree. An
agent can remove the step (or weaken the workflow) on a release branch and tag that commit; the gate then never runs.
This holds for every release gate, ADR 0089's `measured-on` and C103 (d) included, not only this one. Possible remedies:
Gitea protected tags so only the user can create `<app>-v*` and `v*` tags, or a post-tag check run from `main` that
compares the tag run's job list with `main`'s release workflow and fails (and withholds publication) when a gate job is
missing. ADR 0097 is asked to cover it as R5 (§5).

## Decision

### 1. The register: owner and skip class per scheduled job (C107)

A `[[scheduled]]` section in ADR 0060's register `docs/gates.toml` (D1 (d); share first, the register already holds
owners, reasons and review dates of checks), one entry per scheduled workflow and, inside it, per job, matrix legs by a
name pattern:

```toml
[[scheduled]]
workflow = "debrief-bench.yml"
owner = "debrief"                   # an app id, or "family" (the coordinator) for shared workflows
period_days = 1                     # from the cron; weekly = 7

[[scheduled.job]]
name = "store benchmarks (10M events)"
skip = "never"                      # it runs whenever the workflow runs

[[scheduled]]
workflow = "nightly.yml"
owner = "family"
period_days = 1
skip_workflow = "green-unchanged"   # nightly-mark: no run while main is unchanged since its last green run

[[scheduled.job]]
name = "fuzz (shard *)"
skip = "inputs"                     # ADR 0080 §3.3: skipped when its inputs are unchanged since its last green night

[[scheduled.job]]
name = "everything"
skip = "caller"                     # a reusable-workflow caller: judged by its children

[[scheduled.job]]
name = "installers (*)"
skip = "after-failure"              # skipped by design when an upstream job failed

[[scheduled.job]]
name = "paddock-dev (*)"
skip = "never"
covers = "release-path"             # only for failures that are not tests (§3); see D3

[[scheduled.job]]
name = "crate (*)"
skip = "never"
tests = "derived"                   # C107 reads the packages from the job's commands; their dependents are covered (§3)
```

**C107** (a `./dev check` in the RT-5 host) fails when a workflow with `on.schedule`, or a job it runs, has no entry;
when an entry names a workflow or job that no longer exists; when an owner is not an app id or `family`; when
`period_days` disagrees with the cron; and when a skip class disagrees with the job's text (`after-failure` without a
`needs` and no `always()`, `caller` on a job with `runs-on`). It expands reusable workflows (`uses: ./.gitea/workflows/...`)
into their jobs with the caller's name prefix, and matrices into their legs, as Gitea names them. A planted workflow
without an entry and a planted wrong skip class are its positive controls (ER-33). `docs/ci-schedules.md` gets its owner
column from the register, not by hand.

### 2. Reading a scheduled run: outcomes, skips and signatures

One pure function in a Rust module `tools/paddock-dev/src/ci_runs/scheduled/` (ADR 0022; one CLI variant), fed by RT-8's
`Client`, turns each register job of each scheduled run into one of:

- **pass**: the job's first attempt ended `success` and was full (not a dispatch with filter inputs);
- **fail(signatures)**: see below;
- **flaky(signatures)**: the first attempt failed and a rerun attempt of the same run, or a later run on the same commit,
  passed. It stays a failure for the gate (ER-12: a retry never turns red green) with a `flaky:` prefix;
- **skipped, carried forward**: `skip = "inputs"` and the job was skipped; its last first-attempt result on a commit with
  the same inputs is carried forward, and when there is none it is **not measured**. A workflow skipped under
  `skip_workflow = "green-unchanged"` carries every job's last result forward the same way;
- **skipped, charged**: `skip = "after-failure"`; the skip is charged to the failing upstream job's signatures and adds
  none of its own;
- **caller**: `skip = "caller"`, recognised by RT-8's `TimedJob::is_caller` (no runner, no steps, a real span); judged by
  its children and adds none of its own;
- **not measured**: cancelled, never started (`never_ran`, or no runner and no steps without being a caller), a log that
  cannot be accounted for, or a workflow with no completed run within `2 × period_days`.

**Signatures**, one per failing test (D10): the failing test ids read by ADR 0089 §4b's `runlog` (nextest, Playwright,
vitest; RT-8's `logfind` becomes `runlog` when §4b lands), as `test:<package>::<binary>::<path>`. A failure that is not a
test, or a test run `runlog` cannot read (the bench's `cargo test --nocapture`), gives one signature per failing step from
**annotation lines only**: a line that starts with the runner's timestamp followed by `::error::` (`^\S+Z ::error::`),
never a line that merely contains the text; digits, hex runs and temp paths are replaced by `N`, as
`step:<step>:<annotation>`. A failing step with no annotation gives `unclassified:<step>`, never "no failure" (ADR 0089).
Synthetic fixtures cover the echo shapes of job 34160 (an `echo "::error::..."` in a `run:` script, inside `{ ...; }`, in
a `case`, across a continued line) next to a real annotation, and must find only the latter.

### 3. Coverage is per failing test, through the planner graph (ER-31)

A `test:` signature covers the apps whose dependency set contains the test's package, resolved by the same planner graph
ER-31 (a) and ADR 0080 §3.3 use (the app's `Cargo.lock`, the npm workspace graph), at the commit being tagged. For
example `paddock-mcp` is in the lockfiles of `crates/`, Dyno, Pitwall, `shell-demo` and `workbench-demo` and no other
app, so a red `paddock-mcp` test blocks those and not Debrief, Pitcrew or Spotter. A `step:` or `unclassified:` signature
(rustup missing, a leftover VM) cannot be traced to a package. It covers, in this order: the job's `covers` if the register
gives one; otherwise the dependents of the packages the job tests (§1 `tests`, derived by C107 from the job's commands,
for example `cargo nextest -p` lists, `--workspace` in `crates/`, an app's `paddock.ci.json` leg); otherwise, for an app
owner, that app. A `family` job for which neither `covers` nor `tests` can be derived fails C107, so no `family` job
covers nothing by omission (D12). A `not measured` job covers the same way. Every `step:`, `unclassified:` and `not
measured` case is listed in the digest for triage. For tool jobs (`paddock-dev (*)`, `dev-tool.yml`) the derived
coverage is D3's.

### 4. The tag gate: stateless, from the Actions record

`./dev ci scheduled gate --app <app> --commit <sha>` computes, for each register job, from the Actions API and git only:
the latest **completed** first-attempt run on `main` (a run still in progress is reported as pending and the one before it
is used), its outcome per §2, and for each signature the start of its red streak: the completion time of the first run that showed
it after the job's last **three consecutive** full first-attempt greens (D13), so an intermittent failure that passes in
between does not restart its clock; a signature also listed as flaky by the flake ledger (RT-8 `ci flakes`, across
commits, ER-12) keeps its first appearance in the ledger's window as its start. It refuses (exit 5) when a signature that covers the app (§3) has a streak
older than 24 h at the time of the gate and no valid user waiver (§5); it returns 6 (not measured) when it cannot read the
API or the logs, when a covering job is not measured, or when the last sweep (§6) is older than 30 h, so a broken or
stopped pipeline never passes. Issues are not read for the verdict.

- **The 24 h clock** runs from the streak's start, not from the sweep or an issue, so it is the same 24 h whatever time
  the sweep runs.
- **Release-branch tags (ER-31).** Scheduled runs are on `main`. For a tag on a release branch, coverage (§3) is resolved
  in the dependency set at the tagged commit, and a covering red on `main` blocks the tag unless the same job was run green,
  full and first-attempt on the tagged commit (a dispatch of the scheduled workflow on that commit), or is waived. A fix
  that is only on the release branch is proven by that dispatch, not by being there.
- **Where it runs.** In each release workflow on a tag (`dyno-`, `pitcrew-`, `pitwall-`, `spotter-`, `shell-demo-`,
  `workbench-demo-release.yml`, `debrief.yml` on `debrief-v*`, `publish.yml` for `v*` as app `packages`), as a gates step
  before anything is built or published, beside ADR 0089's `measured-on`; so a tag pushed by hand is still refused. Also in
  the steward's tag checklist and in `./dev release tag` once that exists (ER-9). ADR 0041's hand-run row becomes a reading
  of this gate.
- **Not gated:** landings on `main` and trains (a fix must be able to land), branch runs, fix rounds.
- **The mode is a user decision, signed.** The gate's mode lives in `docs/gates.toml` (`[scheduled_gate]`: `mode =
  "report" | "enforce"`, `report_until = YYYY-MM-DD`, `reason`), never in a repository variable or an environment
  value, because agents can change those with the administrator token or the keychain credential. The gate reads it in
  two parts:
  - **The go-live anchor.** The **go-live commit** is the earliest commit on `origin/main`'s first-parent history that
    introduced a `[scheduled_gate]` section. It is found once from that history and never reset: deleting the record and
    adding it again, or adding one on another branch, does not move it. The **unsigned allowance** (report-only for the go-live length
    of D9) is read only from the record as it stands in that go-live commit, never from the tagged tree or a later commit.
    It starts at a **server-recorded time**: the `created_at` of the first Actions run on `origin/main` whose head commit
    contains the go-live commit, never the committer date, which an agent can set. A `report_until` in the record counts
    at most up to that time plus the go-live length. `origin/main` must be protected against force-push, so the anchor's
    first-parent history cannot be rewritten; ADR 0097's R5 and its D8 cover that protection.
  - **After the go-live commit**, the record must exist on `origin/main`; a missing or unparsable record there gives
    exit 6 (not measured). The tagged tree's copy is not read for the mode: like every user record (ADR 0097 §6), the mode
    is read only from `origin/main`'s first-parent history, so a record added, changed or deleted on a release branch has
    no effect. Before the unsigned allowance ends, the gate reports only. After it, the gate enforces; a mode other than
    `enforce`, or a later `report_until`, counts only from a record that ADR 0097's verifier accepts on `origin/main`
    (every line blamed to a commit with a valid user-presence signature from the trust root), and otherwise the gate exits
    6. There is no `off`.
  - **Positive controls (WP-5):** an unsigned commit that sets `mode = "report"` after the allowance still refuses; a
    signed one by a key outside the trust root still refuses; deleting the record and adding it back on `main` leaves the
    go-live date unchanged and refuses after the original allowance; a fresh unsigned record on a release branch (tagged
    tree only) gives no allowance and refuses; deleting the record on `main` gives exit 6; a go-live commit whose committer
    date is set earlier than its first Actions run on `main` gets its allowance from the run's `created_at`; a signature
    with the user-presence bit clear is refused.

### 5. Waivers need the user present (detail in ADR 0097)

No waiver path that an agent can complete on its own counts. Agents run as the user's macOS account, so they can reach
more than the `GITEA_TOKEN`: git's osxkeychain credential (`git credential fill`), the user's signed-in Chrome through
claude-in-chrome, and CI secrets reachable from a pushed workflow; a PreToolUse hook that refuses a `waive:` command is
bypassed by a script file or the browser. A Gitea comment, label or issue state therefore proves nothing about who wrote
it, and neither does an `approved = "user"` line.

The waiver this ADR recommends (D5 (a)), and the gate mode of §4, are records in `docs/gates.toml` (`[[scheduled_waiver]]`: workflow, job,
signature, reason, `until` at most 14 days ahead; `[scheduled_gate]`) added by a commit, or named by a tag, **signed with a touch-required
`sk-ssh-ed25519` key or a Secure Enclave key** listed in an `allowed_signers` file; the gate verifies through **ADR 0097's verifier**
(`./dev approvals verify`) that **every line** of each record, as read from `origin/main`'s first-parent history, blames
to a commit whose signature is valid for a key in the trust root **and has the user-presence bit set**, and ignores a
record any line of which fails that, so an agent cannot extend a signed waiver by editing its `until` line. `git log
--format=%G?`, `git verify-commit` and `ssh-keygen -Y verify` are not enough: they accept an `sk-*` signature whose
presence flag is clear (0097 §1), so the verifier parses the signature itself. A touch or Touch ID prompt is something an agent cannot answer. How the
user's approvals are recorded across the family (keys, the `allowed_signers` file, rotation, what else they cover) is ADR
0097's subject; this ADR consumes it and does not design it. It states what it needs from 0097:

- **R1.** Agents cannot change the set of trusted keys. An `allowed_signers` file in the repository is not enough, since
  any agent can commit a key to it; the trust root must be somewhere agents cannot write, or every change to it must
  itself be signed by a key already in it, checked from a pinned starting point.
- **R2.** Verification is per line: a record counts only if every line of it blames to a commit signed by a trusted key.
- **R3.** Scope includes switches (the gate mode of §4) as well as waivers, and any later user-only setting of this kind.
- **R4.** Each signature needs the user present (a touch or Touch ID), never a key an agent process can use unattended.
- **R5.** A release gate cannot be removed by the tree it gates: the release workflow and its gate steps run from the
  tagged tree, so 0097 is asked to cover tag creation itself (protected tags for the user, or a post-tag check from
  `main` that compares the tag run's jobs with `main`'s release workflow); see the residual risk under the Invariant.

Until 0097 is accepted, the gate has no waivers at all: only a fix clears a red, and the shadow period (§7) is the slack.

As defence in depth, agents and CI move to a bot account (`paddock-agent`: issue write, Actions read, no admin), so
issues and comments say who wrote them and agents stop holding an administrator token. It does not make a waiver
provable and is not used as proof.

### 6. Issues and the digest: display only

`./dev ci scheduled sweep` runs §2 over every scheduled run since its last sweep and keeps one Gitea issue per (workflow,
job, signature) (D10: per failing test, not per set), keyed by a first body line (`<!-- scheduled: debrief-bench.yml |
store benchmarks (10M events) | test:debrief-store-worker::it::worker::imports_and_queries_within_the_budgets -->`), with
labels `scheduled-red`, `app:<owner>` and one `covers:<app>` per covered app. It needs a small write client beside RT-8's
read client: create is idempotent (search by key first, create, then re-search and close a duplicate it lost a race to),
comments are added once per new commit or once a day. Issues hold the display and a link to the waiver record, never the
verdict: an agent who closes one changes nothing the gate reads, and the next sweep reopens it.

- **Closure rule** (D11): the sweep closes an issue only when the job's latest run on `main` is a full, unfiltered,
  first-attempt green that ran the test (or step) of the signature; a green on a rerun attempt of the same commit turns the
  issue's signature `flaky:` and leaves it open (ER-12); a skip carried forward closes nothing.
- **Fix link.** `./dev ci scheduled fix <issue> <commit>` notes a fix commit; the digest reports "fix `<sha>` not on `main`
  for N h" until `git merge-base --is-ancestor` says it is (the fuzz case, 3 days).
- **Digest.** `./dev ci scheduled digest` prints, per app and for `family`: open signatures with streak age, owner and
  covered apps; those past 24 h and the tags they block; pending runs; fixes not on `main`; waivers and expiries;
  `not measured` jobs; `step:` signatures awaiting coverage triage. A scheduled workflow `scheduled-digest.yml` (Linux,
  one slot, owner `family`, itself in the register) runs the sweep and the digest at **09:13 and 15:13 local**: the
  nightly starts 05:03 and has run up to 537 min, `debrief.yml` 06:03 and up to 581 min, so the morning digest lists them
  as pending and the afternoon one judges them. These are daytime starts against the user's night-only scheduling
  decision of 2026-11-10; the job builds nothing, takes one Linux slot for about a minute and reads the API, but it is
  an exception the user must grant (D14). Without it, the sweep runs at 04:13 local only (Linux, outside the backup
  window, before the Windows starts), and the nightly and `debrief.yml` are judged a day later in the digest; the gate,
  which reads the Actions record itself, is not delayed. It writes into one issue labelled `scheduled-digest` (body replaced);
  pinning it is optional, as whether pinning needs administrator rights on this Gitea is not verified (WP-0 checks). The
  coordinator's session start reads the digest, and every coordinator batch report carries its first line. A digest older
  than 30 h is reported stale, and the gate treats a sweep older than 30 h as not measured (§4).

It exits per ADR 0089: 0, 5 when it found reds, 6 when it could not read a run or log, 7 when the register is broken.

### 7. Go-live: a shadow period, then a triage, then enforcement

For N days after go-live (D9: 14 recommended) the gate runs everywhere in report-only mode: its verdict and the reasons
go into the release summary and the digest, and nothing is refused. The period ends with a triage of the backlog with the
user: every open signature is fixed, waived (§5) or has its coverage corrected, and the register's skip classes are
checked against what the period showed. The triage is planned before `report_until`; if it is not done by then,
enforcement starts anyway (fail closed) and only the user can extend the period, with a signed record (§4); the period's start is the go-live commit on
`origin/main` and cannot be reset. Reasons: bench has
never been green, mutants fails within minutes of starting, and the `step:` coverage needs a look before it blocks
anything.

`docs/ci-schedules.md` is corrected at go-live (mutants ran twice and failed both times).

## What it gates

After the shadow period and its triage: the `<app>-v*` tags of every app and the `v*` tags of the shared packages, each
only through failing tests whose package the app ships (§3) and through untraceable step failures of jobs it owns. Today's
reds as an illustration, not a forecast (the triage changes them): `debrief-bench.yml` (a `debrief-store-worker` test)
and `debrief-mutants.yml` would block a Debrief tag; the nightly's `crate (macos-latest)` and `crate (windows-latest)`
block exactly the apps whose lockfiles contain the failing tests' crates, which this ADR has not enumerated (that is the
gate's job); `paddock-dev (*)` blocks under D3 (today every app but `packages`); `app dyno (macos-latest)` was red only on 11-09 and green on 11-10, so it
blocks nothing; `debrief-fuzz.yml` is not measured today (run 7863 cancelled) and blocks Debrief only if the next run is
not green. Landings, branch runs and fix rounds are never gated.

## Parallelism and cost (rule 51)

The sweep reads one runs page per 50 runs (about one a day per workflow, 9 scheduled workflows plus the digest's own) and
the logs of failed first attempts only; on today's numbers about 10 failed jobs a day, so seconds of API time and one log
each. Comments are rate-limited by the "new commit or once a day" rule. The gate repeats the same reads for one app's jobs
at tag time, under a minute. Nothing to parallelise; RT-8's client retries and ADR 0089's `runlog` are reused, not
copied (rule 55).

## Consequences

- A red scheduled check now costs a release, so owners fix or the user waives with a touch; "red for two weeks" can no
  longer happen silently, and ADR 0080's net works again because a new failure is a new signature, not "still red".
- A shared crate's flaky test can hold several apps' tags; that is the intent for code they ship (ER-31 (b)) and the
  pressure that makes it get fixed (ER-12).
- The user gets a daily digest and, after 0097, occasional touch prompts for waivers; agents never produce waivers.
- The register adds one entry per new scheduled job; C107 keeps it complete.

## Rollback

Rolling back is the user's decision and needs the user present. The gate's mode is the signed `[scheduled_gate]` record
of §4: the user sets `mode = "report"` with a new `report_until` in a signed commit, and the gate returns to the shadow
mode. An unsigned change does nothing but turn the gate's result into exit 6 after `report_until`, so a rollback cannot
be made by an agent and a failed attempt does not pass a tag. The sweep and the digest are independent of the gate and can
be stopped by disabling `scheduled-digest.yml`; the gate then reports not measured after 30 h. Removing the register
section, C107 and the gate step is a normal revert, which lands like any change (reviewed, on `main`), and is itself the
user's decision.

## Alternatives

- **The gate reads the issues** (revision 1). Rejected: agents can open, close and edit issues as the user, and a stopped
  sweep would fail open.
- **A final job in every scheduled workflow that files its own issue.** It touches every workflow, needs a write token in
  each, and misses the runs that matter most (cancelled, never started, a workflow that stopped being scheduled).
- **Quarantine (`continue-on-error` until fixed).** Hides the red the same way; ADR 0080 §3.4 names `continue-on-error`
  on Debrief's memory budget as the opposite of a quiet check. A waiver with an expiry is the honest form of it.
- **Block landings on `main` while a scheduled check is red.** Stops the fix from landing.
- **Coverage by job** (revision 1). Contradicts ER-31 ("judge failing tests, not jobs") and over-blocks: a red
  `paddock-mcp` test in `crate (macos-latest)` would hold Debrief, which does not ship it.
- **Waiver by Gitea comment from the user's account**, after the bot move. Agents can still reach the user's session
  (keychain credential, Chrome); kept only as a display.

## Decisions for the user

- **D1. Where owners and skip classes are declared** (§1, C107): (a) a separate `conformance/ci/scheduled.toml`; (b)
  fields in each app's `paddock.ci.json` (shared workflows have no app file); (c) comments in each workflow; (d) a
  `[[scheduled]]` section in `docs/gates.toml` (recommended: share first, ADR 0060's register already holds owners and
  review dates).
- **D2. Who keeps the issues** (§6): (a) one sweep from outside (recommended); (b) a final job in each scheduled workflow.
- **D3. What a `step:` failure of a `family` tool job (`paddock-dev (*)`, `dev-tool.yml`) covers**: (a) every app,
  because `./dev` is on the release path (ER-31 (b)); (b) none, because no app ships it; (c) the apps whose release
  workflow reaches `./dev`, derived by C107 from the workflow graph, following composite actions and called workflows
  (recommended: exactly the apps whose release path it is on). Today that is every app and not `packages`:
  `dyno-release.yml` and `pitcrew-release.yml` run `./dev check` directly and call `ci.yml`; `pitwall-` and
  `spotter-release.yml` use `./.gitea/actions/plan` (`./dev plan --ci --app`) and call `ci.yml`; the two demo releases use
  `./.gitea/actions/job-isolation` (`./dev ci slot`); `debrief.yml` runs `./dev`; `publish.yml` reaches none. Its test
  failures are `test:` signatures and follow §3 like any other.
- **D4. The tag gate** (§4): (a) streak older than 24 h refuses unless waived, enforced in the release workflows on the
  tag and checked before tagging, stateless from the Actions record (recommended); (b) 48 h; (c) report only.
- **D5. How a user waiver is recorded** (§5): (a) a `[[scheduled_waiver]]` record added by a commit or tag signed with a
  touch-required `sk-ssh-ed25519` or Secure Enclave key in `allowed_signers`, as ADR 0097 defines (recommended; no waivers
  until 0097 is accepted); (b) a Gitea comment from the user's account after the bot move (not recommended: agents can act
  in the user's session); (c) `approved = "user"` text (no).
- **D6. Not measured gates** (§2, §4): a covering job that was cancelled, never started or has not run within two periods
  gates like a red (ER-33) (recommended); or it is reported only.
- **D7. Known reds on an unchanged `main`**: (a) keep running every scheduled run (recommended; the runs are at night, and
  ADR 0080 §4.4 relies on the nightly running on every change of `main` to catch slips, which a skip keyed on an open
  issue would weaken further); (b) skip while the signature is open and `main` is unchanged, unless dispatched.
- **D8. Waiver limit**: (a) at most 14 days and bound to the signature (recommended); (b) until the next tag of the app.
- **D9. Go-live mode** (§7): (a) report-only for 14 days, then a backlog triage with the user, then enforcement
  (recommended); (b) 7 days; (c) enforce at once with streaks counted from go-live.
- **D10. Issue and signature key**: (a) one per failing test (recommended: a test fixed among several closes its own
  issue, and coverage is per test anyway); (b) one per set of failing tests of a job.
- **D11. Closure rule** (§6): (a) only a full, unfiltered, first-attempt green that ran the signature's test or step; a
  rerun green makes it `flaky:` (recommended, ER-12); (b) any green of the job.

- **D12. Coverage of a `family` job's untraceable failures** (§3): (a) `covers` if given, else the dependents of the
  packages the job tests as derived by C107, and C107 fails a `family` job for which neither exists (recommended); (b)
  require an explicit `covers` on every `family` job.
- **D13. When an intermittent failure's clock starts** (§4): (a) at its first red after the job's last three
  consecutive full first-attempt greens, or its first appearance in the flake ledger (recommended); (b) N = 5; (c) only
  the unbroken streak (revision 2; an intermittent failure then never ages).
- **D14. The digest's daytime runs** (§6): (a) grant an exception to the night-only decision for `scheduled-digest.yml`
  at 09:13 and 15:13 local (one Linux slot, about a minute, no build) (recommended); (b) night only, at 04:13 local, with
  the nightly and `debrief.yml` shown a day later.
- **D15. The gate mode** (§4): (a) a signed `[scheduled_gate]` record in `docs/gates.toml`, enforce after `report_until`
  unless a signed record says otherwise, no `off` (recommended); (b) a repository variable (not recommended: agents can
  change it).

## Work packages (agent-days)

| WP | What | Days | Needs |
| --- | --- | --- | --- |
| WP-0 | Bot account `paddock-agent` (issue write, Actions read, no admin), tokens swapped in agents' environment and CI secrets; labels `scheduled-red`, `scheduled-digest`, `covers:<app>`; check whether pinning needs admin and whether a tag run's `github.token` can read the Actions API | 0.5 (plus the user's account step) | — |
| WP-1 | `[[scheduled]]` section for the 9 scheduled workflows and the digest, C107 in the RT-5 host (reusable workflows, composite actions and matrices expanded; skip classes checked against job text; `tests` derived from job commands; D3's `./dev` reach derived from the workflow graph; a `family` job with no `covers` and no derivable `tests` fails) with positive controls; owner column of `docs/ci-schedules.md` generated | 3 | D1, D3, D12, RT-5 |
| WP-2 | The pure reader of §2 (outcomes, skip classes, callers, flaky, not measured, streaks) and signatures on `runlog` with annotation-only `step:` lines; synthetic fixtures (job 34160's echo shapes, callers, charged skips, carried-forward skips, rerun greens); no real logs | 3 | RT-8 on `main`, ADR 0089 WP-2c (`runlog` for Playwright and vitest), WP-1 |
| WP-3 | Coverage per signature (§3) through the planner graph at a commit; tests with a planted crate in two of three lockfiles | 1 | WP-2 |
| WP-4 | Write client (idempotent create, duplicate close, comment rate rule) and `sweep`, `fix`, `digest`, `scheduled-digest.yml` (09:13 and 15:13 local) | 2.5 | WP-0, WP-2 |
| WP-5 | `gate --app --commit`: stateless verdict, 24 h streak rule, pending runs, release-branch tags, sweep freshness, ADR 0089 exit codes, the three-greens streak rule and the flake ledger, the `[scheduled_gate]` mode record; wired into the six `*-release.yml`, `debrief.yml` (tags) and `publish.yml`, and the tag checklist; positive controls (a planted red refuses, a stale sweep is not measured, a rerun green still refuses, an intermittent red with greens in between still ages, an unsigned mode flip after the allowance still refuses, a key outside the trust root refuses, delete-and-re-add on `main` keeps the go-live time, a fresh record on a release branch gives no allowance, a deleted record on `main` is exit 6, a back-dated go-live commit gets the run's `created_at`, a signature with the presence bit clear refuses) | 2 | WP-2, WP-3, D4 |
| WP-6 | Waiver and mode records, per-line blame and signature check against ADR 0097's trust root through ADR 0097's verifier (unsigned, wrong-key, presence-bit-clear and edited-line controls; the go-live anchor read from `origin/main`'s first-parent history, its time from the first Actions run's `created_at`) | 0.5 | ADR 0097 accepted, D5 (a) |
| WP-7 | Rule text (a Paddock rule and an ER-33 amendment, below), agent and reviewer briefs | 0.5 | WP-5 |
| WP-8 | Go-live: report-only switch on, backfill sweep, `docs/ci-schedules.md` corrected; at the end of the period the triage list for the user | 0.5 | WP-4, WP-5 |

**Total about 13.5 agent-days. A first cut that reaches the shadow period with the gate (WP-0, WP-1, WP-2, WP-3, WP-5,
WP-8) is about 10; the sweep, issues and digest (WP-4) add 2.5 (until then the gate's sweep-freshness check is off);
WP-6 waits for ADR 0097; WP-7 is 0.5.** WP-0 needs the user's hands (creating the account). Before WP-6 the gate cannot
leave enforcement once `report_until` has passed, because no signed record can be verified; the shadow period is set
long enough (D9) for 0097 to land first.

**Proposed rule** (a Paddock rule and an amendment to ER-33, wording for the user's approval): "Every scheduled CI job has
a declared owner and skip class. A failing or unmeasured scheduled job is in the coordinator's daily digest within a day;
an app is not tagged while a scheduled failure in code it ships has been red for more than 24 h, judged from the CI record
and not from issues, unless the user has waived that failure with a step that needs the user present."

## Risks

- **The planner graph decides coverage.** A wrong graph under-blocks; C107 checks the register, not the graph. The graph
  is already what ER-31 relies on, so a fix to it helps both.
- **`step:` signatures.** Infrastructure failures (runner setup, VMs) fall back to the owner or D3 and are listed for
  triage; until triaged they may block an app whose code is fine. The shadow period is where these are sorted.
- **Signature churn.** A failure whose text changes every run (numbers, as the bench's "776/527 MB") would start a new
  streak each run and never reach 24 h without the normalisation; normalisation is tested on the shapes above, and a
  `step:` whose normalised text still changes is flagged in the digest.
- **Job-token permissions.** Whether a tag run's `github.token` can read the Actions API and logs is not verified; WP-0
  checks it first and otherwise uses the bot's read token as a secret.
- **Waivers wait for ADR 0097.** Until then only fixes clear a red after enforcement and the mode cannot be changed; if
  0097 is late, enforcement still starts at `report_until` (fail closed) and only fixes clear reds until it lands. This is deliberate: a switch an agent can flip is the fail-open path P1-6 found.

## Review answers (revision 2)

- P1-1 (gate trusts editable issues, fails open): §4 is stateless from the Actions record and git; issues are display and
  waiver links only (§6); a sweep older than 30 h is not measured; closure only on a full first-attempt green, a rerun
  green is `flaky:` (D11).
- P1-2 (waiver proof): "provably" dropped; the agents' other routes listed; the user-presence option is a touch-required
  `sk-ssh-ed25519` or Secure Enclave signature in `allowed_signers`, defined by ADR 0097; the bot account kept as defence
  in depth only (§5, D5).
- P1-3 (skips): skip classes per job (§1), charged skips, carried-forward input skips, workflow-level green-unchanged
  skips, callers by `is_caller`, never-started jobs as not measured (§2).
- P1-4 (signatures): annotation lines only, on ADR 0089 §4b `runlog`; WP-2 needs 0089 WP-2c; echo-shape fixtures from job
  34160 (§2, Context).
- P1-5 (coverage by job; go-live): coverage per failing test through the planner graph (§3), "likely all" removed,
  `paddock-mcp` lockfiles stated; shadow period and triage before enforcement (§7, D9).
- P2s: D3 (c) and `dev-tool.yml` added; D1 (d), D9, D10, D11 added; facts corrected (`af929a1e3`, `+08:00` run times and
  `Z` job times, durations 0-581 and 9-537 min, `app dyno (macos-latest)` red only on 11-09, `fuzz (shard 1)` and the
  cancelled run 7863); the 24 h clock runs from the streak start and in-progress runs are pending (§4); release-branch
  tags defined (§4); estimates raised for the write client and C107's expansion (13 days, first cut 9.5);
  pinning checked in WP-0 and not relied on; demo release workflows in WP-5; the ADR 0080 §4.4
  conflict noted at D7; Consequences and Rollback added; the README blank line before 0080 removed.

## Review answers (revision 3)

- P1-6 (the repository-variable switch reopens fail-open): the switch is gone; the mode is a `[scheduled_gate]` record in
  `docs/gates.toml` under the same per-line signature check as waivers (§4, §5), a user decision; after `report_until`
  anything but `enforce` without a signed record is exit 6; no `off`; positive control: an unsigned flip still refuses
  (WP-5); Rollback rewritten; D15 added.
- P2-1 (D3 (c)): derived from the workflow graph through composite actions and called workflows, in C107; today every
  app and not `packages`, with the route per workflow stated.
- P2-2 (coverage gaps): `covers`, else the dependents of the job's tested packages derived by C107, else the owner app; a
  `family` job with neither fails C107 (§3, D12).
- P2-3 (intermittent flakes never age): the streak starts at the first red after the last three consecutive greens, or
  at the first flake-ledger appearance (§4, D13).
- P2-4 (requirements for ADR 0097): R1 to R4 in §5: agents cannot change the trusted keys, per-line blame to signed
  commits, switches in scope, user presence.
- P2-5 (daytime digest): D14 asks the user for the exception, with a 04:13 night-only fallback.
- P2-6 (counts): 9 scheduled workflows including `dev-tool.yml`, 10 with the digest.
- P2-7 (heads): block refreshed (train `c151422a4`, `origin/docs/adr-evidence-heads` `b32655fef`) and
  `origin/docs/adr-private-files` added; the golden fix's bearing on the `paddock-dev (windows-latest)` red noted.
- Estimate +0.5 day (WP-1): 13.5 in all, first cut 10.

## Review answers (revision 4)

- P1-7 (report mode reopened without a signature by deleting the record, deleting and re-adding it, or adding a fresh
  one on a release branch): the unsigned allowance is read only from the go-live commit, the earliest commit on
  `origin/main`'s first-parent history that introduced `[scheduled_gate]`, never reset; after it, a missing record on the
  tagged tree or on `origin/main` is exit 6; positive controls added for delete-and-re-add, a fresh record on a release
  branch and a deleted record (§4, WP-5, WP-6).
- P2 (the gate step runs from the tagged tree): stated as a residual risk under the Invariant, shared with every release
  gate (ADR 0089 `measured-on`, C103 (d)), with protected tags or a post-tag job-list check from `main` as remedies, and
  handed to ADR 0097 as R5 (§5). Not fixed here.

## At acceptance

- Accepted by the user on 2026-11-10: D1 to D13 and D15 as recommended, D14 (a) (the daytime digest at 09:13 and 15:13
  local as an exception to the night-only scheduling decision). The independent review approved revision 4 at
  `6270bbbbe`.
- The review's P2s on revision 4: the unsigned allowance starts from the server-recorded `created_at` of the first Actions
  run on `origin/main` containing the go-live commit, not the committer date (§4); the WP-5 row lists every positive
  control of §4; `origin/main` must be protected against force-push, covered by ADR 0097's R5 and D8 (§4).
- Aligned with ADR 0097 (`d21eddcbf`, proposed): the mode record is read only from `origin/main`'s first-parent history
  (0097 §6), so the tagged tree's copy is not required and has no effect, while a record missing on `main` stays exit 6
  (§4); verification goes through 0097's verifier, which requires the user-presence bit, not `%G?` (§5).
