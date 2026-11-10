# 96. Accepted decisions land, and their work is tracked to main

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10, after the user chose "a register plus a
  check" that day (ER-21: one class, six instances below). Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40 (looking for recurring classes is standing work), ER-30, ER-33 / rule 71, ER-36, rule 50 (d)
  (docs-only changes land after the local conformance checks, without CI); ADR 0021 (§4 `.gitattributes` policy), ADR
  0038 (`./dev check --pre-push`), ADR 0049 (§5 `merge=union` for the ADR indexes and C82), ADR 0088 (accepted: registers
  and one ratchet engine), ADR 0089 (accepted: `Outcome`, "not measured" is never a pass), ADR 0091 (proposed:
  `evidence-heads` and the public "is this commit in `origin/main`" query in `./dev evidence`); the plan
  `docs/plans/rust-tooling-2026-11-10.md` on `origin/docs/rust-tooling-plan` (§3: one module folder, one CLI variant).
- Number: 0096 was free on every `origin` branch on 2026-11-10 (highest file `0091`; 0092 to 0095 are being drafted, and
  no branch's `docs/` names "ADR 0096" except ADR 0091's forward reference). Check id: the next free one when this is
  accepted (C105 is ADR 0091's; 0092 to 0095 may claim more); written C1xx below.
- Evidence: read on 2026-11-10 with `git ls-tree`, `git show`, `git log -S`, `git merge-base --is-ancestor` and `git
  merge-tree` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3`. No user
  data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/docs/adr-logging-root-cause 021dc3c8d
origin/ci/per-crate-coverage 4539d48f1
origin/docs/adr-wiring-outcomes-ui fed6b5855
origin/docs/adr-0081-0082 9dc480ef6
origin/docs/adr-0083-0084 ee3f54485
origin/docs/adr-0085-release-cache-isolation 3a49d49a1
origin/docs/adr-redesigns-2026-11-10 405b119a0
origin/docs/adr-windows-gate-ratchets db7782e01
origin/docs/adr-not-measured 0df6835a9
origin/docs/adr-captured-tool-output 9c2cd4d8e
origin/docs/adr-evidence-heads f2bdba1df
origin/docs/rule-48-merge-check 3d45cdb57
origin/docs/rust-tooling-plan 2791c634c
origin/feat/rt5-check-host ec8530a95
```

## Context: the incidents

**1. Accepted texts that are on neither `origin/main` nor the train.** A scan of every `origin` branch for ADR files
whose `- Status:` line says Accepted, compared with `origin/main` and `origin/train/2026-11-09-3`, found 13:

| ADR | Accepted | Only on |
|---|---|---|
| 0036 logging for root cause | 2026-10-31 | `origin/docs/adr-logging-root-cause` (also `origin/ci/publish-tools`) |
| 0050 per-crate coverage gate | 2026-11-04 | `origin/ci/per-crate-coverage` |
| 0052 wired features, visible outcomes, real layout | 2026-11-04 | `origin/docs/adr-wiring-outcomes-ui` |
| 0081, 0082 | 2026-11-10 | `origin/docs/adr-0081-0082` |
| 0083, 0084 | 2026-11-10 | `origin/docs/adr-0083-0084` |
| 0085 | 2026-11-10 | `origin/docs/adr-0085-release-cache-isolation` |
| 0086 | 2026-11-10 | `origin/docs/adr-redesigns-2026-11-10` |
| 0087, 0088 | 2026-11-10 | `origin/docs/adr-windows-gate-ratchets` |
| 0089 | 2026-11-10 | `origin/docs/adr-not-measured` |
| 0090 | 2026-11-10 | `origin/docs/adr-captured-tool-output` |

A further 15 accepted ADRs (0051, 0058, 0060, 0071 to 0078, 0080) are on the train only; `origin/main` (`b51c72509`,
2026-11-08) has 45 ADR files, the last 0056. So an agent that reads `origin/main`, as briefs say to (memory
"lookups-read-origin-main"), sees none of the 28 newest accepted decisions; and one ADR, 0036, has waited ten days.

**2. Approved mechanisms that were never built, while their class recurred.**

| Decision | Approved | State on the train and `origin/main` | Since then |
|---|---|---|---|
| ADR 0021 §4: `**/golden/** text eol=lf`, `*.snap text eol=lf`, `**/samples/** -text`, `**/testdata/** -text` in the root `.gitattributes` (`0021-portable-tests.md:217-224`) | 2026-10-27 | none of the four lines; the root file has only the narrower `crates/*/tests/golden/** text eol=lf` and `crates/*/fixtures/** -text` | CRLF checkouts broke tests twice more, each fixed by a narrower line: `1589c46bb` (2026-11-04, packaging files, run 7494, per the `.gitattributes` comment) and `4d169393d` (2026-11-08, the paddock-limits TypeScript golden, run 7706) |
| ADR 0038: `./dev check --pre-push` runs CI's plan before a push | 2026-11-03 | not built: no `pre-push`/`push-gate` in `tools/paddock-dev/src`; `lefthook.yml`'s `pre-push` runs only `pitwall-scripts`, `pitwall-deny` and `ipc-endpoints` | the engineering-rules pre-review checklist (`checklists/pre-review.md`, its "Why", 2026-11-10) lists "a conformance failure that reached CI" among the day's review rounds; the plan schedules it as RT-12 |
| ADR 0049 §5: `merge=union` for the ADR indexes and lessons, and C82 over them | 2026-11-04 | no `merge=union` in `.gitattributes` (train or `origin/main`); no C82 under `conformance/src/checks` | see incident 3 |
| The merge check for stale reverts (memory "redesigns-2026-11-05") | 2026-11-05 | `scripts/merge-regression-check.mjs` only on `origin/docs/rule-48-merge-check` (`3d45cdb57`), an ancestor of neither main nor the train | the coordinator reports the class recurred; **unverified here** |
| Two redesigns of 2026-11-06 (memory "redesigns-2026-11-06"): doc facts generated and checked from code; rule 53 extended to remembered items (proven identity) | 2026-11-06 | no ADR on any branch; ADR 0060 (`:14`) records "doc facts from code" as "not started" | the coordinator reports recurrences; **unverified here** |

**3. The index conflicts on every docs branch.** The train's `docs/adr/README.md` lists 0080 last (no 0079 row: 0079 is
a proposal on `origin/docs/adr-scrutineer`), and every ADR branch appends its row at the same place. Measured with `git
merge-tree --write-tree`: `origin/docs/adr-not-measured` against `origin/docs/adr-captured-tool-output`, and the latter
against `origin/docs/adr-evidence-heads`, each conflict in `docs/adr/README.md`. The index also disagrees with the files:
the train's 0080 row says "proposed 2026-11-09, revision 2" while the file's status line says Accepted (2026-11-09).

**4. Status lines have no fixed shape.** Of 50 accepted ADR files on the train, the status lines come in more than eight
shapes ("Accepted (DATE), by the user, …", "Accepted DATE by the user: D1–D10 as …", "Accepted (DATE, after an
independent review)", "proposed (DATE)"), and 18 files have a `## Work packages` section in one of several table shapes.
No tool can read "which work of which accepted decision has landed" from them.

## Pattern

**A decision is accepted, and then nothing carries it to main or to done.** The text stays on a docs branch, the
approval stays in memory or a chat, the work packages have no owner state, and nothing reports the gap; the class the
decision was meant to stop recurs while the fix is "decided". This is ER-33's shape applied to decisions: an accepted
ADR reads as "handled" while nothing measured that it was.

**Invariant.** Every accepted decision is on `origin/main` within a short, fixed time; each of its work packages has a
recorded state that a command can check against git; a decision approved without an ADR has a register row too; and an
unknown state is "not measured", never "done".

## Decision

### 1. Accepted texts land on main the same day

An ADR's acceptance commit (status line, decisions) lands on `origin/main` as a docs-only change under rule 50 (d) on
the day of acceptance, by the integrator, on its own: it does not wait for the train its work rides on. A text that
lands with code (an amendment in a feature branch) is the exception and says so in its register row. The backlog of
incident 1 lands once (WP-0).

### 2. A register: one small file per decision, `docs/adr/register/<id>.toml`

One file per ADR (and per approved decision without an ADR, id `R-YYYY-MM-DD-<slug>`), so two branches never edit the
same file and the register itself cannot conflict:

```toml
id = "0091"
title = "Design documents pin their evidence"
status = "proposed"            # proposed | accepted | superseded | rejected | approved (no ADR yet)
accepted = "2026-11-10"        # when status is accepted or approved; from the clock
source = "memory decisions-2026-11-10"   # for approved rows: where the approval is recorded

[[wp]]
id = "WP-1"
days = 1.25
state = "not-started"          # not-started | in-progress | landed | dropped
branch = "feat/evidence-check" # in-progress
commit = ""                    # landed: the commit on origin/main that completes it
reason = ""                    # dropped: why, and who approved it
```

The register is the machine-readable source; the ADR's prose keeps its own status line, and C1xx checks they agree on
the one word (Proposed, Accepted, Superseded, Rejected).

### 3. The index is generated

`docs/adr/README.md`'s table is written by `./dev adr index --write` from the register files (number, link, title,
status word, date) plus a one-line summary field in each register file. C1xx fails when the index differs from what the
command would write. The index then never holds a status the file contradicts (incident 3's 0080 row), and a branch
that adds an ADR regenerates the index, which still conflicts textually with another branch's regeneration; the merge
resolution is to run the command again, which the integrator's merge step does. With `merge=union` on the index (ADR
0049 §5, accepted and unbuilt) both rows survive and C1xx catches order or duplicates; **this ADR builds that line of
0049 §5 for the ADR index** rather than inventing a second rule.

### 4. Checks

- **C1xx in the RT-5 host (tree only, deterministic):** every ADR file has a register file and the reverse; the status
  words agree; an accepted register row has at least one `[[wp]]` or `wp = "none"` with a reason; every WP has a valid
  state with its field (`branch`, `commit`, or `reason`); the index equals the generated one. It runs in the local
  conformance checks of rule 50 (d).
- **`./dev adr status` (git, Rust, `tools/paddock-dev/src/adr/`):** for each accepted row, the ADR file is in
  `origin/main` (else a failing finding "accepted, not on main", listing the branches that hold it, found with
  `git ls-tree` over `origin/docs/*`: 89 branches, 1.4 s here); each `landed` WP's commit is in `origin/main`, through
  ADR 0091's public query (not a second copy); each `in-progress` branch exists; anything it cannot read is
  `NotMeasured` (ADR 0089), never "landed". It also lists accepted ADRs found only on branches whose register file is
  not on main yet, so the backlog is visible before WP-0 lands it.
- **Staleness, a warning only:** `./dev adr status` warns when an accepted ADR is not on main more than 2 days after
  acceptance, or a WP is `not-started` or `in-progress` more than N days (default 14, a setting) after acceptance. It is
  not in conformance: a result that changes with the calendar would turn a green tree red with no change (the same
  argument as ADR 0091 §5). The coordinator runs it at the end of every batch, next to ER-21's "is this a class we have
  seen before?", and brings the overdue list to the user.

### What this does not cover

Whether the facts inside an accepted ADR are still current (ADR 0091); merges that revert others' work (the merge check
on `origin/docs/rule-48-merge-check`, which gets its own register row here); app ADRs under `apps/<app>/docs/adr/`
(numbered per app; the same files under `apps/<app>/docs/adr/register/` once the family register works, D6).

## Parallelism and cost (rule 51)

The full scan behind incident 1 (`git show` of every ADR file on 363 `origin` branches) took several minutes; the
register makes it unnecessary. `./dev adr status` reads one file per ADR, one `merge-base` per landed WP and one
`ls-tree` per docs branch (89 branches, 1.4 s measured). C1xx is a scan of about 90 small TOML files.

## Alternatives

- **Derive state from the ADR texts.** Status lines and WP tables come in many shapes (incident 4); a parser would be a
  guess, and a misread state reads as done. Rejected.
- **One `docs/adr/register.toml`.** Every branch that adds or moves a decision edits one file; with `merge=union` two
  edits of one row become a duplicate TOML key (a parse error, at least loud). One file per decision avoids it. Kept as
  D2 (b).
- **An issue tracker.** Gitea issues would hold state outside the repository, unreadable by the tree check and by agents
  without a token. Rejected for now.
- **Only a rule in the briefs.** That is today; incident 1 has 13 rows under it.

## Consequences

- The integrator lands accepted texts daily (minutes each, no CI under rule 50 (d)); WP-0 lands the backlog once.
- Every ADR change also touches its register file; `./dev adr new` (WP-1) writes the skeleton.
- The overdue list makes a decision that nobody builds visible within two weeks, and the user decides to build, defer
  or drop it.

## Rollback

Delete the register folder, the command and C1xx; restore a hand-written index from the last generated one.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | When an accepted text lands | (a) same day, docs-only under rule 50 (d), by the integrator, not waiting for a train; (b) with its first work package; (c) with the train | **(a)**: incident 1's backlog is (b) and (c) in practice |
| D2 | Register shape | (a) one file per decision, `docs/adr/register/<id>.toml`; (b) one `docs/adr/register.toml` with `merge=union` | **(a)**: no conflicts and no duplicate keys |
| D3 | The index | (a) generated from the register, checked by C1xx, `merge=union` per ADR 0049 §5; (b) hand-written with 0049's union and C82 only | **(a)**: also removes status disagreements |
| D4 | Approved decisions without an ADR | (a) a register row `R-…` with its source, the same staleness warning; (b) only ADRs are registered | **(a)**: three of incident 2's rows had no ADR |
| D5 | Staleness | (a) warnings in `./dev adr status` (2 days for the text, 14 for WPs, a setting), run by the coordinator at the end of each batch; (b) a conformance error; (c) none | **(a)**: (b) is time-dependent |
| D6 | App ADRs | (a) family ADRs first, apps after the command proves itself; (b) all at once | **(a)** |
| D7 | Rule text | (a) one Paddock rule and an ER rule (general rules become ER-n): "an accepted decision lands on main the same day and its work has a recorded state"; (b) a line in ER-21 | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-0 | Land the 13 accepted texts of incident 1 on `origin/main` (docs-only, rule 50 (d)), in number order, the index rows rebuilt once | 0.25 | the user's D1 |
| WP-1 | Register format and files for every family ADR (status word, WPs with their present state, read from the texts by hand once and reviewed), rows for incident 2's approved decisions; `./dev adr new` and `./dev adr index --write` | 1 | – |
| WP-2 | `./dev adr status`: the main and branch reads, ADR 0091's in-main query (or the same function moved to a shared module if 0091 is not built yet), `Outcome` per row, staleness warnings; tests on a throw-away repository with a planted unlanded text, a landed WP whose commit is not in main, and an unreadable ref (positive controls) | 1 | ADR 0089's `Outcome` (or a shim) |
| WP-3 | C1xx in the RT-5 host, with good and bad register fixtures; the `merge=union` line for the ADR index (ADR 0049 §5's index part) | 0.5 | RT-5 landed |
| WP-4 | Rule and ER text (D7), lesson, integrator and coordinator brief lines (land accepted texts; run `./dev adr status` at batch end) | 0.25 | – |
| | **Total** | **3** | |

WP-0 and WP-4 can start on the user's yes; WP-1 and WP-2 in parallel; WP-3 after RT-5.

**Tag gating.** None. Nothing here is on an app's release path. (The decisions it tracks may gate tags; this ADR only
makes that visible.)
