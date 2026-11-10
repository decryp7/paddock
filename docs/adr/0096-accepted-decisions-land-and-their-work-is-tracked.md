# 96. Accepted decisions land, and their work is tracked to main

- Status: Proposed 2026-11-10, revision 2. Revision 1 (`919381e26`) was reviewed independently ("changes": one P1 and
  nine P2s), answered here (see "Review answers"). Drafted at the coordinator's request of 2026-11-10, after the user chose "a register plus a
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
  merge-tree` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3` `67a633ef3`; the train moved to `c151422a4` before this revision, changing one test file under `tools/paddock-dev` that nothing here names. No user
  data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
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

A further 12 accepted ADRs (0051, 0058, 0060, 0071 to 0078, 0080) are on the train only; `origin/main` (`b51c72509`,
2026-11-08) has 45 ADR files, the last 0056. So an agent that reads `origin/main`, as briefs say to (memory
"lookups-read-origin-main"), sees none of the 25 newest accepted decisions; and one ADR, 0036, has waited ten days.

**2. Approved mechanisms that were never built, while their class recurred.**

| Decision | Approved | State on the train and `origin/main` | Since then |
|---|---|---|---|
| ADR 0021 §4: `**/golden/** text eol=lf`, `*.snap text eol=lf`, `**/samples/** -text`, `**/testdata/** -text` in the root `.gitattributes` (`0021-portable-tests.md:217-224`) | 2026-10-27 | none of the four lines; the root file has only the narrower `crates/*/tests/golden/** text eol=lf` and `crates/*/fixtures/** -text` | a CRLF checkout broke a golden again: `4d169393d` (2026-11-08, the paddock-limits TypeScript golden, run 7706), fixed by a narrower line than §4 asked for, a real recurrence. `1589c46bb` (2026-11-04, Linux packaging files, run 7494) is the same class but not covered by §4 |
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

### 1. Accepted texts land on main the same day, as copied files, never as a merge

**Why not merge the source branches.** Measured with `git diff --name-only origin/main...<branch>` and `git rev-list
--count <branch>..origin/main`: `origin/docs/adr-0085-release-cache-isolation` and `origin/docs/adr-redesigns-2026-11-10`
each carry 2,367 files outside `docs/`, and `origin/docs/adr-windows-gate-ratchets`, `origin/docs/adr-not-measured` and
`origin/docs/adr-captured-tool-output` 2,384 each (they are based on the train); `origin/ci/per-crate-coverage` carries 12
files outside `docs/` and is 843 commits behind `origin/main`; `origin/docs/adr-logging-root-cause` is 1,175 behind and
`origin/docs/adr-wiring-outcomes-ui` 871. Merging any of them would land code, or old trees, with a docs change.

**The extraction.** The integrator builds one docs-only commit on top of `origin/main` that writes each accepted ADR
file from its branch with `git show <branch>:<path> > <path>` (blobs only; no source branch is merged, cherry-picked or
rebased), adds the ADR's register file (§2), regenerates the index (§3), and lands under rule 50 (d). Each copied ADR gets
one status note line: "Text copied to main on DATE from `<branch>` `<sha>`; its paths and line numbers refer to its
stated base, not to main." The same method serves every later acceptance, on the day of acceptance, without waiting for
the train the work rides on.

**The train's accepted ADRs.** The 12 accepted ADRs that are on the train only (0051, 0058, 0060, 0071 to 0078, 0080)
are copied in the same first commit (WP-0), with the same note naming `origin/train/2026-11-09-3`; when the train lands,
its versions replace them (the train is their source, so the merge takes the train's text). No exemption is needed.

### 2. A register: one small file per decision, `docs/adr/register/<id>.toml`

One file per ADR, and per decision approved without an ADR (id `R-YYYY-MM-DD-<slug>`), so two branches never edit the
same file and the register itself cannot conflict:

```toml
id = "0091"
title = "Design documents pin their evidence"
summary = "..."                # the index line
status = "accepted"            # proposed | accepted | superseded | rejected | approved (R- rows only)
accepted = "2026-11-10"        # from the clock

[[wp]]
id = "WP-1"
days = 1.25
state = "in-progress"          # stored states: not-started | in-progress | dropped
branch = "feat/evidence-check" # in-progress
commit = "1a2b3c4d5"           # optional: the commit that completes the WP, once there is one
reason = ""                    # dropped: why, and who approved it
```

**Landed is derived, never stored.** A WP counts as landed when its `commit` is an ancestor of `origin/main`; until then
it is in progress, whatever anyone wrote. An accepted decision with no work says `no_work = "<reason>"` instead of `[[wp]]`
rows.

**Rows for decisions without an ADR** carry the approval inline (`approval = "<the user's words>"`, `approved =
"YYYY-MM-DD"`) or link a file in the repository that holds it (`approval_file = "docs/..."`), and an `adr_by =
"YYYY-MM-DD"` date by which they become an ADR. Memory files are private to the coordinator's machine and no tool or
other agent can read them, so a row never points at one.

**The status line gets one shape.** The ADR's `- Status:` line starts with one of the five words (Proposed, Accepted,
Superseded, Rejected, Approved) followed by the date, then free text: `- Status: Accepted 2026-11-10 (user), ...`. WP-1
rewrites the 50 accepted status lines of incident 4 into this shape once (their meaning unchanged), and C1xx checks the
first word and date against the register.

### 3. The index is generated

`docs/adr/README.md`'s table is written by `./dev adr index --write` from the register files (number, link, title, status
word, date, summary). C1xx fails when the index differs from what the command would write, so the index never again holds
a status the file contradicts (incident 3's 0080 row). Two branches that each regenerate still touch the same lines; the
merge resolution is to run the command again, which the integrator's merge step does. Until WP-1 lands, the second of two
conflicting docs branches to land (today ADR 0091 and this ADR) fixes the row by hand.

**C1xx is C82's ADR-index part.** ADR 0049 §5 gave C82 the ADR indexes, `docs/lessons.md`, the `performance.md` files and
`docs/benchmarks.md`. The index part moves to C1xx (row order and uniqueness follow from generation); C82 keeps lessons,
`performance.md` and `docs/benchmarks.md`, and the `merge=union` line for the index is added with WP-3. ADR 0049's
register row records the split.

### 4. Checks

- **C1xx in the RT-5 host (tree only, deterministic):** every ADR file has a register file and the reverse; the status
  line's first word and date agree with the register; an accepted row has `[[wp]]` rows or `no_work`; each WP's stored
  state has its field (`branch` for in-progress, `reason` for dropped); R- rows have `approval` or `approval_file` and
  `adr_by`; the index equals the generated one. It runs in the local conformance checks of rule 50 (d).
- **`./dev adr status` (git, Rust, `tools/paddock-dev/src/adr/`):** for each accepted row, the ADR file is in
  `origin/main` (else a failing finding "accepted, not on main", naming the branches that hold it, found with `git
  ls-tree` over `origin/docs/*`: 89 branches, 1.4 s here); each WP's `commit` decides landed through the in-main query;
  each `in-progress` branch exists; anything it cannot read is `NotMeasured` (ADR 0089), never "landed". It also lists
  accepted ADRs found only on branches, so the backlog is visible before it is copied.
- **One home for the in-main query:** `tools/paddock-dev/src/git_refs.rs` (`is_in(commit, "origin/main") -> Result<bool,
  GitReadError>`), written by whichever of ADR 0091 WP-1 and this ADR's WP-2 lands first and used by both.
- **Staleness, report-only:** an accepted ADR not on main 2 days after acceptance, a WP `not-started` or `in-progress`
  more than N days (default 14, a setting) after acceptance, and an R- row past its `adr_by` date are ADR 0089
  report-only `Findings` (exit 0, listed). A result that changes with the calendar must not fail a gate (the argument of
  ADR 0091 §5). They are printed in the nightly job summary (`nightly.yml`), at the integrator's landing step, and in the
  coordinator's end-of-batch review next to ER-21's question; the coordinator brings the overdue list to the user.

### What this does not cover

Whether the facts inside an accepted ADR are still current (ADR 0091); merges that revert others' work (the merge check
on `origin/docs/rule-48-merge-check`, which gets an R- row here); app ADRs under `apps/<app>/docs/adr/` (numbered per app;
D6).

## Parallelism and cost (rule 51)

The full scan behind incident 1 (`git show` of every ADR file on 363 `origin` branches) took several minutes; the
register makes it unnecessary. `./dev adr status` reads one file per ADR, one `merge-base` per WP commit and one `ls-tree`
per docs branch (89 branches, 1.4 s measured). C1xx is a scan of about 90 small TOML files.

## Alternatives

- **Derive state from the ADR texts.** Status lines and WP tables come in many shapes (incident 4); a misread state reads
  as done. Rejected; the status line is normalised once instead (§2).
- **One `docs/adr/register.toml`.** Every branch that adds or moves a decision edits one file; with `merge=union` two
  edits of one row become a duplicate TOML key. Kept as D2 (b).
- **Store "landed".** A stored state can claim a commit main does not have. Rejected (§2).
- **Merge the docs branches.** Measured in §1: they carry thousands of non-docs files or old trees. Rejected.
- **An issue tracker.** State outside the repository, unreadable by the tree check and by agents without a token.
  Rejected for now.
- **Only a rule in the briefs.** That is today; incident 1 has 13 rows under it.

## Consequences

- The integrator copies accepted texts daily (minutes each, no CI under rule 50 (d)); WP-0 copies the backlog once.
- Every ADR change also touches its register file; `./dev adr new` (WP-1) writes the skeleton.
- A decision nobody builds becomes visible within two weeks, and the user decides to build, defer or drop it.
- Copied texts on main can name paths that only exist on their base; the status note says so.

## Rollback

Delete the register folder, the command and C1xx; restore a hand-written index from the last generated one; give the
index part back to C82.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | When and how an accepted text lands | (a) same day, a docs-only commit on `origin/main` that copies the file blobs with `git show` (never a merge), with a status note, by the integrator; (b) with its first work package; (c) with the train | **(a)**: incident 1's backlog is (b) and (c) in practice, and §1 measures why merging is impossible |
| D2 | Register shape | (a) one file per decision, `docs/adr/register/<id>.toml`; (b) one `docs/adr/register.toml` with `merge=union` | **(a)**: no conflicts and no duplicate keys |
| D3 | The index | (a) generated from the register, checked by C1xx, which takes over C82's ADR-index part; (b) hand-written with 0049's union and C82 | **(a)**: also removes status disagreements |
| D4 | Approved decisions without an ADR | (a) R- rows with the approval inline or in a repository file and an `adr_by` date; (b) only ADRs are registered | **(a)**: three of incident 2's rows had no ADR |
| D5 | Staleness | (a) report-only findings in `./dev adr status` (2 days, 14 days, `adr_by`), printed nightly, at landing and at batch end; (b) a conformance error; (c) none | **(a)**: (b) is time-dependent |
| D6 | App ADRs | (a) family ADRs first, apps after the command proves itself; (b) all at once | **(a)** |
| D7 | Rule text | (a) one Paddock rule and an ER rule (general rules become ER-n): "an accepted decision lands on main the same day and its work has a recorded state"; (b) a line in ER-21 | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-0 | One docs-only commit on `origin/main`: the 13 accepted texts of incident 1 and the 12 train-only ones copied with `git show` (§1), each with its status note, the index rebuilt once by hand | 0.5 | the user's D1 |
| WP-1 | Register format and files for every family ADR (states read from the texts once, reviewed), R- rows for incident 2 with their approval text; the 50 status lines normalised; `./dev adr new` and `./dev adr index --write` | 1.25 | – |
| WP-2 | `./dev adr status`: main and branch reads, `git_refs.rs` (if ADR 0091 has not built it), `Outcome` per row, report-only staleness; tests on a throw-away repository with a planted unlanded text, a WP commit not in main, and an unreadable ref (positive controls); the nightly summary and landing-step lines | 1 | ADR 0089's `Outcome` |
| WP-3 | C1xx in the RT-5 host with good and bad register fixtures; the index's `merge=union` line; C82's scope reduced in ADR 0049's register row | 0.5 | RT-5 landed |
| WP-4 | Rule and ER text (D7), lesson, integrator and coordinator brief lines | 0.25 | – |
| | **Total** | **3.5** | |

WP-0 and WP-4 can start on the user's yes; WP-1 and WP-2 in parallel; WP-3 after RT-5.

**Tag gating.** None. Nothing here is on an app's release path. (The decisions it tracks may gate tags; this ADR only
makes that visible.)

## Review answers (revision 1, `919381e26`)

| Finding | Answer |
|---|---|
| P1 WP-0 cannot merge the source branches | §1: measured non-docs files and behind counts; one docs-only commit on `origin/main` copies blobs with `git show`, never merges; the 12 train-only accepted ADRs are copied too; each copy gets a status note on its base; index regenerated; D1 states the method for daily landings |
| P2-1 counts | 12 train-only, 25 newest; `1589c46bb` is the same class but not covered by §4, `4d169393d` the real recurrence |
| P2-2 landed derived | §2: stored states are not-started, in-progress, dropped; landed when the WP's commit is an ancestor of `origin/main` |
| P2-3 C82 split | §3: C1xx is C82's ADR-index part; C82 keeps lessons, `performance.md`, `benchmarks.md`; recorded in 0049's register row (WP-3) |
| P2-4 status-line shape | §2: first word one of five, then the date; WP-1 normalises the 50 lines |
| P2-5 staleness | §4: 0089 report-only findings, exit 0, in the nightly summary and at the landing step |
| P2-6 R- rows | §2: approval inline or a repository file, `adr_by` date; never a memory file |
| P2-7 in-main query home | §4: `tools/paddock-dev/src/git_refs.rs`, built by whichever WP lands first |
| P2-8 `no_work` | §2 and C1xx use `no_work = "<reason>"` |
| P2-9 size | 3.5 days (WP-0 0.5 with 25 copies, WP-1 1.25 with the status-line pass) |
| README conflict with 0091 | §3: the second of the two to land fixes the row by hand until WP-1 |
