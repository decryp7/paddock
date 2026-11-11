# 91. Design documents pin their evidence, and reviews check it is still current

- Status: Deferred 2026-11-11 (user): nothing here is built until the user asks. Was: Accepted 2026-11-10 (user), all decisions as recommended. History: Proposed 2026-11-10; revision 1
  (`5d1c252c4`) was reviewed independently ("changes": P1-1, P1-2 and nine P2s); revision 2 (`f2bdba1df`) answered it
  and was approved by the re-review, whose P2s this text folds in (see "Review answers"). Drafted at the
  coordinator's request of 2026-11-10 (ER-21: one class, seen four times in one day's ADR work and twice before).
  Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-30 (wiki `verified:` dates), ER-33 / rule 71, ER-36, rule 50 (d) (docs-only changes land
  after the local conformance checks); ADR 0022 (developer tools are Rust in `./dev`) and the plan
  `docs/plans/rust-tooling-2026-11-10.md` on `origin/docs/rust-tooling-plan` (§2: no new tool written as a script first;
  §3: a package owns a module folder and adds one CLI variant; generic code stays generic); ADR 0088 (accepted: one
  ratchet engine, registers); ADR 0089 (accepted: `paddock_cli::outcome::Outcome` and `Verdict`, exit codes and their
  precedence, its §1, §2 and D1 to D3); ADR 0090 (accepted 2026-11-10 at `9c2cd4d8e`: captured tool output, C104); the
  RT-5 check host (`./dev check`, `origin/feat/rt5-check-host`); ADR 0096 (to be drafted: accepted decisions land on
  main and their work is tracked).
- Number: 0091 was free on every `origin` branch on 2026-11-10 (highest file `0090`; no branch's `docs/` named "ADR
  0091"; no ADR worktree held an uncommitted 009x file). Check id C105: C104 (ADR 0090) is the highest on any `origin`
  branch (`git grep -wE 'C1(0[5-9]|[1-9][0-9])'` over `docs`, `conformance` and `tools` of every `origin` branch found
  none).
- Evidence: read on 2026-11-10 with `git show`, `git log`, `git grep`, `git merge-base --is-ancestor`, `git log
  --cherry-mark` and `git tag --contains` after `git fetch`. Paths without a branch are from this ADR's base
  `origin/train/2026-11-09-3` `67a633ef3`; before the acceptance commit the train moved to `c151422a4` (one test file,
  `tools/paddock-dev/tests/it/golden.rs`) and RT-8 to `0d40eafed` (`ci_runs/{client,flakes,mod}.rs`, `cli.rs`), none of
  them a file this ADR names, so the block was accepted forward (report-only moves, §2). No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/docs/adr-not-measured 0df6835a9
origin/docs/adr-captured-tool-output 9c2cd4d8e
origin/docs/adr-windows-gate-ratchets db7782e01
origin/docs/rust-tooling-plan 2791c634c
origin/feat/rt5-check-host ec8530a95
origin/feat/rt8-ci-flakes 0d40eafed
origin/docs/rule-48-merge-check 3d45cdb57
engineering-rules main ca4124bdc
```

## Context: the incidents

Times are committer times from `git log -1 --format=%ci` (all +08:00, 2026-11-10 unless dated).

| # | Document | What it said | What git shows |
|---|---|---|---|
| 1 | ADR 0089 revision 1, `e5e4f300d` (18:34:59) | RT-5's check host at `40b7faf8f` and RT-8 at `dff738952` have the not-measured defects (its incident table, rows 4 and 5) | RT-5's fix `8550764e7` was committed at 18:32:21, 2.5 minutes before; RT-8's fix `d95a47ceb` at 18:36:59, 2 minutes after. The review asked for them (0089 "Review answers (revision 1)": P1-5 "stale defects") |
| 2 | ADR 0089 revision 2, `90d28a0a6` (18:50:53) | RT-5 at `8550764e7` (its line 18 and the instance table, line 85) | RT-5's next fix `0a94bf347` ("one function decides a check run's status") was committed at 18:50:51, two seconds before. Revision 3 moved to it. (The coordinator's note says it landed 20 minutes later; committer times cannot show push times, so the gap is unverified, the miss is not) |
| 3 | ADR 0090 revision 2, `9687d644b` (19:05:06) | cites `origin/feat/rt8-ci-flakes` `da05b54e8` (its Evidence line) and describes RT-8's readers | its survey row still described `ci_runs/logfind.rs` with nextest 0.9.151 fixtures, while `da05b54e8` (19:01:39) had made `nextest_log.rs` the one nextest reader with `logfind.rs` a caller; the re-review raised it as P1-A, answered in revision 3 `22b6a4b16` (0090 "Review answers (revision 3)") |
| 4 | ADR 0090 revisions 1 and 2, and a reviewer | Pitcrew's `launchctl print` line scan is a shipped bug (a 1.0.x fix and a release-notes note were proposed) | the scan came in with `5ff3b0a39` (Pitcrew) and `638a61a6b` (Pitwall), both 2026-10-23. `git tag --contains` on either lists only `dyno-latest dyno-v1.0.0 dyno-v1.1.0`; revision 3 withdrew the claim (0090 P1-B). Measured here: at `pitcrew-v1.0.1`, `apps/pitcrew/app/src-tauri/src/service_repair.rs` has 0 lines matching `starts_with("state = ")` (the scan) and 7 naming `stale_registration`; `git log --cherry-mark pitcrew-v1.0.1...5ff3b0a39` marks no equivalent patch |
| 5 | A scout lookup, 2026-11-03 | Dyno has no disk-map UI and no reveal action | it read the coordinator's main checkout, 1008 commits behind `origin/main`; both exist there (memory "lookups-read-origin-main") |
| 6 | Commit `b48df1818` (2026-11-05 18:28), one parent `945798909` | an uninstall fix | built on a stale tree, it also reverted the macOS memory fix, MCP key redaction and e2e fixes; a steward caught it by hand (memory "merges-check-stale-reverts"). The merge check for this lives on `origin/docs/rule-48-merge-check` (`scripts/merge-regression-check.mjs`, not on main) |

**It is happening now.** Read at 2026-11-10 20:04 +08:00, after `git fetch`, against the tips in this ADR's block:
accepted ADR 0089 (`0df6835a9`) pins nine heads in its Evidence line, and four have moved: `origin/train/2026-11-09-3`
`67a633ef3` → `c151422a4` (+3), `origin/feat/rt5-check-host` `0a94bf347` → `ec8530a95` (+1), `origin/feat/rt8-ci-flakes`
`da05b54e8` → `0d40eafed` (+3) and `origin/docs/adr-captured-tool-output` `9687d644b` → `9c2cd4d8e` (+2); five are
current. Accepted ADR 0090 (`9c2cd4d8e`) pins six heads; two have moved (the train +3, RT-8 `1313b8d9e` → `0d40eafed`
+2), four are current. (At the revision 2 fetch, about 19:40, 0089 had three moved and 0090 none: the count grows by
the hour.) Measured with `git rev-parse` and `git rev-list --count` per head; eleven heads took 0.19 s wall time on this
Mac. A reader of 0089 today cannot tell from its text that four of its facts may be out of date.


**The practice exists, by hand.** 26 of the 57 family ADR files on the train have a `- Evidence` line, 29 cite a
9-character commit, and 27 name an `origin/` branch (`grep -lE` over `docs/adr/0*.md`). The lines are prose in many
shapes ("a read of `origin/main` at `233c1339b` and of the branches ...", "CI run 6512 ..."), so no tool can compare
them. ADR 0090 revision 3 already checks shipped claims with `git tag --contains` and then reads the code at the tags.
The wiki has the date-level version of the same idea: `verified: YYYY-MM-DD` and "Facts that are only on a branch say
so ("on branch X at commit Y, not on main")" (`docs/wiki/README.md:46-50`).

## Pattern

**A design document, review or lookup states a fact about a branch at a point in time, and nothing tells the reader
when that point has passed; or it states a fact about shipped code without checking what any tag holds.** Branches in
this repository move every few minutes on a busy day (rows 1 to 3: fixes 2 seconds to 4 minutes apart from the
document), so a fact read an hour ago is a guess. Each author and reviewer re-checks by memory, and a review spends a
round on it.

**Invariant.** Every fact a design document takes from a moving branch is pinned to a commit in one machine-readable
block; anyone can ask "what moved since?" with one command whose answer is an ADR 0089 `Outcome`; a review starts with
that answer; and a claim that code shipped, or did not, names what each tag holds.

## Decision

### 1. An `evidence-heads` block

**Which documents (the D1 predicate).** A design document needs the block when its text names a backticked
`origin/<branch>`, or cites a commit that is not an ancestor of the document's base. Design documents are: family
`docs/adr/` and `docs/adr/amendments/`, `docs/plans/`, `docs/research/`; each app's `docs/adr/`, `docs/plans/` (Pacenotes,
Pitwall, Sighter today) and `docs/research/` (Debrief today); and review notes, by the reviewer brief.

**Form.** One fenced block with the info string `evidence-heads`: a `read: YYYY-MM-DD` line (from the clock, as the
wiki), then one line per ref, `<remote>/<branch> <sha>` (9 or more hex digits). A ref of another repository is written
`<name> <branch> <sha>`; it is compared only when `<name>` has a configured path (§2), and otherwise listed as not
measured. Prose keeps citing commits as now; the block is what the tool reads.

### 2. `./dev evidence check <file>...`, judged by ADR 0089's `Outcome`

Rust, a new module `tools/paddock-dev/src/evidence/`, one CLI variant (plan §3). It fetches `origin` (unless
`--no-fetch`), parses the block and judges each ref into ADR 0089's `paddock_cli::outcome::Outcome`; the run's exit and
JSON come only from 0089's `Verdict` (precedence 7, then failing findings 5, then 6, then 0). No own codes.

| Ref state | How it is read | `Outcome` |
|---|---|---|
| current | pinned sha is the tip | `Clean` |
| merged, no later change | branch gone, pinned sha in `origin/main`, and no `origin/main` commit after it touches a file the document names | `Clean` |
| moved +N | pinned sha an ancestor of the tip | `Findings`: one finding per moved ref, listing the new commits (one line each) and the files they touch. **Failing** when a new commit touches a file the document names; otherwise **report-only** (listed, exit 0), so a document that pins `origin/main` does not exit 5 on every unrelated landing |
| merged, later change | as merged, but a later `origin/main` commit touches a named file | `Findings`, failing |
| rewritten | pinned sha known, not an ancestor of the tip | `Findings`, failing |
| gone | branch gone, pinned sha not in `origin/main` | `NotMeasured` |
| unknown commit | the pinned sha is not in the repository (`git merge-base` exits 128) | `NotMeasured` |
| fetch or read failure | `git fetch` or any git read fails | `NotMeasured` (a failed read is never `Clean`; `plan_repo::git_text`, line 16, returns `Option` and is not reused as is; the shape wanted is its test `a_failing_git_read_is_an_error_never_an_empty_list`, line 151, through a `Result`) |
| other repository, no configured path | – | `NotMeasured` |
| a panic in the command | – | `CheckBroken` (0089 §1) |

**No block.** `NotApplicable` when the §1 predicate says the document needs none; `NotMeasured` when it needs one.

**Name matching.** A file the document "names" is any backticked path in its text, after stripping a trailing `:line`,
`:a-b` and a parenthesised suffix (`(…)`); it matches a changed file when it is a path suffix of it (`record.rs:112`
matches `tools/paddock-dev/src/check/record.rs`).

**Other repositories.** `--repo <name>=<path>` (or a `[evidence.repos]` entry in the tool's config) lets the
engineering-rules line be compared against that checkout's `origin`.

**`--accept`** rewrites the shas to the tips and the `read:` line to today, after `check` has printed the moves in the
same run. It refuses on a document whose `- Status:` says Accepted: an accepted document's block is history, and its
moves are a revision's or amendment's business.

**Shared query.** The test "is commit X in `origin/main`" lives in one place, `tools/paddock-dev/src/git_refs.rs`
(`is_in(commit, "origin/main") -> Result<bool, GitReadError>`), written by whichever of this ADR's WP-1 and ADR 0096's
WP-2 lands first, and used by both.

### 3. `./dev evidence shipped <commit> [--app <app>] [--grep <pattern> [--path <path>]]`

Prints one row per tag (filtered to `<app>-v*`) with four columns:

| Column | How | What it says |
|---|---|---|
| contains | `git merge-base --is-ancestor <commit> <tag>` (as `git tag --contains`) | the commit itself is in the tag |
| tag before commit | `git merge-base --is-ancestor <tag> <commit>` | the tag is an ancestor of the commit, so the commit came later and cannot be in it except as a copy |
| equivalent patch | `git log --cherry-mark --right-only --format='%m %H' <tag>...<commit>`, the one line whose full sha is `<commit>`: `=` means the tag holds a patch with the same patch-id, `>` means it does not | a cherry-pick onto a release branch |
| grep hits | with `--grep`, `git grep` at the tag (in `--path`, or the whole tree) | the code itself |

Containing the introducing commit is **neither necessary nor sufficient** for shipping: a cherry-pick ships the code
without the commit, and a later commit may remove it. The grep hits are the evidence; the other three columns say how
it got there. A document that says code shipped, or did not, cites this output.

Measured (2026-11-10): for `5ff3b0a39` at `pitcrew-v1.0.1`, contains no, tag before commit yes, equivalent patch `>`
(none), and `service_repair.rs` at the tag has 0 lines matching `starts_with("state = ")` and 7 naming
`stale_registration` (row 4). On a non-trivial range, `pitcrew-v1.0.1...4d169393d` (5,507 commits), the cherry-mark
line for `4d169393d` is `>` and the whole `git log` took 0.04 s; `dyno-v1.1.0...4d169393d` (171 commits) 0.01 s. Positive
control on a throw-away repository: a commit cherry-picked onto a tagged release branch reads contains no, equivalent
patch `=`, grep hit 1. (Match the full `%H`: an abbreviated `%h` has a repository-dependent length, and a fixed-width
match missed the `=` line in the first run of that control.)

### 4. Reviews start with it

The reviewer brief (Paddock `.claude/agents/reviewer.md`, engineering-rules `agents/reviewer.md`) gets one line: run
`./dev evidence check` on the document first; read every failing finding before anything else; a fact the moves
contradict is a P1. The engineering-rules pre-review checklist (`checklists/pre-review.md`) gets two lines: re-read the
heads and run the check right before committing a design document; cite `./dev evidence shipped` for shipped claims.

### 5. C105 in the RT-5 host: the block is there and well formed

For each design document of §1 whose `- Date:` is on or after this ADR's acceptance, C105 fails when the text names a
backticked `origin/<branch>` and has no `evidence-heads` block, when the block is malformed, or when a backticked
`origin/<branch>` and a backticked sha on the same line name a branch missing from the block. Its fixtures include the
Evidence-line shapes of ADR 0089 (`origin/feat/rt5-check-host` `0a94bf347`, several pairs on one line) and ADR 0090 (a
pair followed by a parenthesised note); a planted bad file is its positive control (ER-33). C105 reads only the tree:
the "commit not in the base" half of the predicate needs git and is judged by `./dev evidence check` (no block where
one is needed: `NotMeasured`). C105 does **not** compare heads: a result that changes when someone else pushes would
turn a green commit red with no change of its own, a flaky gate by construction (ER-12). Older documents are out of
scope by their date, so no ADR 0088 register is needed. C105 runs in the local conformance checks a docs-only change
lands after (rule 50 (d)), so it costs no CI run.

### 6. Push gate (RT-12)

For each design document a push changes, the gate runs `./dev evidence check --no-fetch` and prints a `::warning::`
with the failing findings; a `NotMeasured` result prints a "not measured" warning naming the read that failed. It is
never promoted to an error.

### What this does not cover

- A commit built on a stale tree (row 6): the merge check's job (`origin/docs/rule-48-merge-check`, a script still to be
  ported to Rust under the plan).
- Lookups (row 5): the brief rule "read `origin/main` via `git show`", which the scout brief keeps.
- Accepted ADRs that have not landed on main, and accepted work that never got built: ADR 0096.
- ADRs that live only on docs branches: C105 sees only the tree it runs on, so it checks such an ADR on its own branch
  and from the day it lands, not from main before then.

## Parallelism and cost (rule 51)

One `git fetch` (seconds on this network) plus one `rev-parse`, one `merge-base` and one `log` per ref; eleven refs took
0.19 s here, and `git log --cherry-mark` over a 5,507-commit tag range 0.04 s (§3). Nothing to parallelise. C105 is a text scan of under
250 files (57 family ADRs, 153 app ADRs, 8 family plans on the train), within the RT-5 host's tree read.

## Alternatives

- **Prose only, a rule in the brief.** That is today; rows 1 to 4 happened under it, all by careful authors and
  reviewers. Rejected as the only measure, kept as the rule text.
- **Compare heads in conformance (C105) or fail the push gate on moves.** Time-dependent: rejected (§5, §6).
- **Own exit codes for the command.** Revision 1 did; it would be one more copy of "not measured" (ADR 0089's class).
  Rejected.
- **Pin by date, as the wiki.** A date cannot tell which branch moved; rows 1 and 2 were minutes apart on one date.
- **Pin every cited commit, not branch heads.** Commits do not move; the question is what came after them on the branch.

## Consequences

- An author spends one command before committing and one `--accept`; a reviewer gets the list of what changed first, so
  a stale fact costs a minute, not a review round.
- Documents carry a few more lines. Accepted documents keep their block as history (`--accept` refuses them).
- The check fetches; offline, or with `--no-fetch` on stale refs, a read failure says "not measured" (exit 6), never
  "current".

## Rollback

Delete the module and the CLI variant, drop C105 from the registry, the push-gate step and the brief lines. Blocks left
in documents are inert text.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Which documents need the block | (a) design documents (§1 folders) that name an `origin/` branch or cite a commit not in their base, plus review notes by brief; (b) every ADR and plan; (c) ADRs only | **(a)**: the class is about facts from moving branches; a document with none has nothing to pin |
| D2 | Where the comparison lives | (a) `./dev evidence` (Rust, per ADR 0022 and the plan); (b) a generic `devkit evidence` now | **(a)** now, its engine free of Paddock paths (plan §3) so it moves to `devkit` without a rewrite when G2 lands |
| D3 | What C105 checks | (a) presence and form only, from the ADR's acceptance date; (b) also staleness; (c) no check, brief lines only | **(a)**: (b) is a flaky gate by construction; (c) leaves the block to memory, which is the class |
| D4 | Push gate (RT-12) | (a) a `--no-fetch` `::warning::` with moves and "not measured", never an error; (b) an error; (c) nothing | **(a)** |
| D5 | Shipped claims | (a) cite `./dev evidence shipped` (tags containing, patch-id equivalents, `--grep` hits at each tag); (b) `git tag --contains` alone | **(a)**: (b) is neither necessary nor sufficient |
| D6 | Wiki pages (ER-30) | (a) keep `verified:` dates; a page may add a block, the wiki lint does not require it; (b) require blocks | **(a)**: wiki pages summarise main and already mark branch facts |
| D7 | Rule text | (a) one new Paddock rule (number taken at landing) **and** an ER rule, since general Paddock rules become ER-n: either a new ER or an ER-33 amendment, "a fact counts only if its read is pinned", plus the pre-review checklist lines; (b) amend ER-30 only | **(a)**, as an ER-33 amendment: ER-33 already says a check counts only if its probe could see the thing; a design fact is the same claim about a read |

## Work packages (raw agent-days, one agent)

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-1 | `./dev evidence check` and `--accept`: block parser, git reads through a `Result`, the state table judged into ADR 0089's `Outcome` and `Verdict`, name matching, `--repo`, the public "in `origin/main`" query; tests on a throw-away git repository in the shared test scratch (ADR 0078) with a planted current, moved (named and unnamed file), merged, rewritten, gone and unknown head each (positive controls); goldens of the text output | 1.25 | ADR 0089 WP-1 (`paddock_cli::outcome`, 1.5 days), strictly: no local shim, so no second copy of the outcome type to register |
| WP-2 | `./dev evidence shipped` with `--app`, `--grep`, `--path` and the four columns of §3; tests on a throw-away repository with two tags, a cherry-pick (the `=` control) and a commit after the tag | 0.25 | WP-1 |
| WP-3 | C105 in the RT-5 host (`check/c105.rs`, one registry line), fixtures of the 0089 and 0090 shapes, a bad file as control | 0.5 | RT-5 landed |
| WP-4 | Paddock rule and lesson, ER-33 amendment (D7), reviewer brief lines in both repositories, two pre-review checklist lines, the block's form in `docs/adr/README.md` (no ADR template file exists under `docs/adr/` or `templates/` today) | 0.25 | – |
| WP-5 | Blocks for open **proposed** ADRs only (accepted ones such as 0089 and 0090 keep theirs as history); RT-12's warning (§6) | 0.5 | WP-1; RT-12 for the warning |
| | **Total** | **2.75** | |

WP-1, WP-2 and WP-4 can start now; WP-3 waits for RT-5 to land; WP-5's warning waits for RT-12.

**Tag gating.** None. Nothing here is on an app's release path; no tag waits for it.

## Review answers (revision 1, `5d1c252c4`)

| Finding | Answer |
|---|---|
| P1-1 own exit codes | §2: each ref state judged into ADR 0089's `Outcome`, exit and JSON only through its `Verdict`; panic is `CheckBroken`; no block is `NotApplicable` or `NotMeasured` by the §1 predicate |
| P1-2 ADR 0090 accepted | Related line, "happening now" (0090 pins six heads, all current; 0089 now has three moved), WP-5 limited to proposed ADRs; own block refreshed (RT-8 `1313b8d9e`, 0090 `9c2cd4d8e`) |
| P2 unrelated moves | report-only, exit 0, still listed; name matching defined (§2) |
| P2 shipped claims | "neither necessary nor sufficient", cherry-mark column, `--grep [--path]` hits per tag, the `pitcrew-v1.0.1` measurement (§3, row 4) |
| P2 C105 trigger | aligned with D1; same-line pair parsing with 0089 and 0090 fixtures; amendments, app plans and research added; runs in the local docs-only checks (§1, §5) |
| P2 push gate | `--no-fetch`, never an error, "not measured" warning (§6) |
| P2 `--accept`, other repositories | updates `read:`, refuses Accepted documents; `--repo <name>=<path>` (§2) |
| P2 D7 | an ER-33 amendment recommended alongside the Paddock rule |
| P2 WP-1 size | 1.25 days; total 2.75 |
| P2 scope | "What this does not cover" names accepted-but-unlanded ADRs (ADR 0096) and docs-branch-only ADRs; the in-main query is public |
| P2 README | the blank line between the 0078 and 0080 rows removed |

## Review answers (revision 2, `f2bdba1df`, approved; P2s folded in at acceptance)

| Finding | Answer |
|---|---|
| P2 `shipped` columns | §3: a "tag before commit" column (`git merge-base --is-ancestor <tag> <commit>`); the cherry-mark read uses `--right-only` and the one line of the commit's full sha; re-measured on a 5,507-commit range, with a cherry-pick control |
| P2 Outcome shim | WP-1 depends strictly on ADR 0089 WP-1; no shim, so no known-copies entry is needed |
| P2 "six are current" | dated: read 2026-11-10 after the revision 2 fetch, at the heads of this ADR's block |
