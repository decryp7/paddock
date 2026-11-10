# 91. Design documents pin their evidence, and reviews check it is still current

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10 (ER-21: one class, seen four times in
  one day's ADR work and twice before). Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-30 (wiki `verified:` dates), ER-33 / rule 71, ER-36; ADR 0022 (developer tools
  are Rust in `./dev`) and the plan `docs/plans/rust-tooling-2026-11-10.md` on `origin/docs/rust-tooling-plan` (§2: no new
  tool written as a script first; §3: a package owns a module folder and adds one CLI variant); ADR 0088 (accepted: one
  ratchet engine, registers); ADR 0089 (accepted: one outcome type, exit codes 5 findings, 6 not measured, 7 check broken,
  its D3); ADR 0090 (proposed: captured tool output, check C104); the RT-5 check host (`./dev check`,
  `origin/feat/rt5-check-host`).
- Number: 0091 was free on every `origin` branch on 2026-11-10 (highest file `0090`, on
  `origin/docs/adr-captured-tool-output`; no branch's `docs/` names "ADR 0091"; no ADR worktree under
  `/Volumes/d7sst7/dev-targets/wt/` holds an uncommitted 009x file). Check id C105: C104 (ADR 0090) is the highest check
  id on any `origin` branch (`git grep -wE 'C1(0[5-9]|[1-9][0-9])'` over `docs`, `conformance` and `tools` of every
  `origin` branch found none).
- Evidence: read on 2026-11-10 with `git show`, `git log`, `git grep`, `git merge-base --is-ancestor` and
  `git tag --contains` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3`.
  No user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/docs/adr-not-measured 0df6835a9
origin/docs/adr-captured-tool-output 22b6a4b16
origin/docs/adr-windows-gate-ratchets db7782e01
origin/docs/rust-tooling-plan 2791c634c
origin/feat/rt5-check-host ec8530a95
origin/feat/rt8-ci-flakes da05b54e8
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
| 4 | ADR 0090 revisions 1 and 2, and a reviewer | Pitcrew's `launchctl print` line scan is a shipped bug (a 1.0.x fix and a release-notes note were proposed) | the scan came in with `5ff3b0a39` (Pitcrew) and `638a61a6b` (Pitwall), both 2026-10-23. `git tag --contains` on either lists only `dyno-latest dyno-v1.0.0 dyno-v1.1.0`, no Pitcrew or Pitwall tag (measured here); revision 3 withdrew the claim (0090 P1-B) |
| 5 | A scout lookup, 2026-11-03 | Dyno has no disk-map UI and no reveal action | it read the coordinator's main checkout, 1008 commits behind `origin/main`; both exist there (memory "lookups-read-origin-main") |
| 6 | Commit `b48df1818` (2026-11-05 18:28), one parent `945798909` | an uninstall fix | built on a stale tree, it also reverted the macOS memory fix, MCP key redaction and e2e fixes; a steward caught it by hand (memory "merges-check-stale-reverts"). The merge check for this lives on `origin/docs/rule-48-merge-check` (`scripts/merge-regression-check.mjs`, not on main) |

**It is happening now.** Accepted ADR 0089 (`0df6835a9`) pins nine heads in its Evidence line. Compared with the tips
fetched for this ADR, two have moved: `origin/feat/rt5-check-host` `0a94bf347` → `ec8530a95` (+1 commit) and
`origin/docs/adr-captured-tool-output` `9687d644b` → `22b6a4b16` (+1); the other seven are current. ADR 0090's three
pinned heads are current. Measured with `git rev-parse` and `git merge-base --is-ancestor` per head; eleven heads took
0.19 s wall time on this Mac. A reader of 0089 today cannot tell from the text that two of its facts may be out of date.

**The practice exists, by hand.** 26 of the 57 family ADR files on the train have a `- Evidence` line, 29 cite a
9-character commit, and 27 name an `origin/` branch (`grep -lE` over `docs/adr/0*.md`). The lines are prose in many
shapes ("a read of `origin/main` at `233c1339b` and of the branches ...", "CI run 6512 ..."), so no tool can compare
them. ADR 0090 revision 3 already checks shipped claims with `git tag --contains` and then reads the code at the tags
(`service_repair.rs:80-83` at `pitcrew-v1.0.1`), the right two-step form. The wiki has the date-level version of the
same idea: `verified: YYYY-MM-DD` and "Facts that are only on a branch say so ("on branch X at commit Y, not on main")"
(`docs/wiki/README.md:46-50`).

## Pattern

**A design document, review or lookup states a fact about a branch at a point in time, and nothing tells the reader
when that point has passed; or it states a fact about shipped code without checking what any tag contains.** Branches
in this repository move every few minutes on a busy day (rows 1 to 3: fixes 2 seconds to 4 minutes apart from the
document), so a fact read an hour ago is a guess. Each author and reviewer re-checks by memory, and a review spends a
round on it.

**Invariant.** Every fact a design document takes from a moving branch is pinned to a commit in one machine-readable
block; anyone can ask "what moved since?" with one command; a review starts with that answer; and a claim that code
shipped names the tags that contain it.

## Decision

1. **An `evidence-heads` block.** Every ADR, plan and research note (family `docs/adr/`, `docs/plans/`,
   `docs/research/`, and each app's `docs/adr/`) that states facts read from a branch other than its own base carries one
   fenced block with the info string `evidence-heads`: a `read: YYYY-MM-DD` line (from the clock, as the wiki), then one
   line per ref, `<remote>/<branch> <sha>` (9 or more hex digits). A ref outside this repository is written `<repo>
   <branch> <sha>` and is listed, not compared. Prose keeps citing commits as now; the block is what the tool reads.
   A review note carries the same block for what it read.
2. **`./dev evidence check <file>...`** (Rust, a new module `tools/paddock-dev/src/evidence/`, one CLI variant, per the
   plan's §3). It fetches `origin` (unless `--no-fetch`), parses the block and prints, per ref: `current`; `moved +N`
   with the one-line log of the new commits and, of the files they touch, those the document names in backticks;
   `rewritten` (the pinned commit is not an ancestor of the tip); `merged` (branch gone, commit in `origin/main`); or
   `gone`. Exit codes follow ADR 0089 D3: 0 all current, 5 something moved, 6 not measured (fetch failed, a ref
   unreadable, or no block). A failed git read is "not measured", never "current" (the shape of
   `plan_repo.rs`'s test `a_failing_git_read_is_an_error_never_an_empty_list`, line 151, but through a `Result`:
   `plan_repo::git_text` returns `Option`, line 16, and must not be reused as is). `--accept` rewrites the shas to the
   tips after the author has re-read the moves; it never runs without `check` having printed them.
3. **`./dev evidence shipped <commit> [--app <app>] [--path <path>]`.** Prints the tags that contain the commit
   (`git tag --contains`, filtered to `<app>-v*`) and, with `--path`, whether the path exists at each such tag. A
   document that says code shipped, or did not, cites this output. Containing the introducing commit is necessary, not
   sufficient: a later commit may have removed the code, so the document also reads the code at the tag (as ADR 0090
   revision 3 did).
4. **Reviews start with it.** The reviewer brief (Paddock `.claude/agents/reviewer.md`, engineering-rules
   `agents/reviewer.md`) gets one line: run `./dev evidence check` on the document first; every `moved` ref whose new
   commits touch a file the document names is read before anything else; a fact the moves contradict is a P1. The
   engineering-rules pre-review checklist (`checklists/pre-review.md`) gets two lines: re-read the heads and run the
   check right before committing a design document; cite tags for shipped claims.
5. **C105 in the RT-5 host: the block is present and well formed.** For each design document under the paths of §1 whose
   `- Date:` is on or after this ADR's acceptance, C105 fails when the text cites an `origin/` branch or a 9-plus-digit
   commit and has no `evidence-heads` block, when the block is malformed, or when a branch named with a commit in the
   prose is missing from the block. A planted bad file is its positive control (ER-33). C105 reads only the tree: it
   does **not** compare heads, because a result that changes when someone else pushes would turn a green commit red
   with no change of its own, which is a flaky gate by construction (ER-12). Older documents are out of scope by their
   date, so no ADR 0088 register is needed.

What this does not cover: a commit built on a stale tree (row 6) is the merge check's job
(`origin/docs/rule-48-merge-check`, a script still to be ported to Rust under the plan); lookups (row 5) are already
covered by the brief rule "read `origin/main` via `git show`", which the scout brief keeps. Both are listed here because
they are the same class at another level, not because this ADR changes them.

## Parallelism and cost (rule 51)

One `git fetch` (seconds on this network) plus one `rev-parse`, one `merge-base` and one `log` per ref; eleven refs took
0.19 s here. Nothing to parallelise. C105 is a text scan of under 250 files (57 family ADRs, 153 app ADRs, 8 plans on the
train), within the RT-5 host's tree read.

## Alternatives

- **Prose only, a rule in the brief.** That is today; rows 1 to 4 happened under it, all by careful authors and
  reviewers. Rejected as the only measure, kept as the rule text.
- **Compare heads in conformance (C105) or in the push gate as an error.** Time-dependent: rejected for the gate (§5); a
  warning in RT-12's push gate for changed design documents is D4.
- **Pin by date, as the wiki.** A date cannot tell which branch moved; rows 1 and 2 were minutes apart on one date.
- **Pin every cited commit, not branch heads.** Commits do not move; the question is what came after them on the branch,
  which needs the branch.

## Consequences

- An author spends one command before committing and one `--accept`; a reviewer gets the list of what changed first, so
  a stale fact costs a minute, not a review round.
- Documents carry a few more lines. Accepted documents keep their block as history; it is not refreshed after
  acceptance (the moves of an accepted ADR are the next revision's or amendment's business).
- The check fetches; offline it says "not measured" (exit 6), never "current".

## Rollback

Delete the module and the CLI variant, drop C105 from the registry and the brief lines. Blocks left in documents are
inert text.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Which documents need the block | (a) ADRs, plans and research notes that cite another branch, plus review notes by brief; (b) every ADR and plan; (c) ADRs only | **(a)**: the class is about facts from moving branches; a document with none has nothing to pin |
| D2 | Where the comparison lives | (a) `./dev evidence` (Rust, this repo's tool, per ADR 0022 and the plan); (b) a generic `devkit evidence` usable by other repositories | **(a)** now, its engine free of Paddock paths (plan §3 "generic code stays generic") so it moves to `devkit` without a rewrite when G2 lands |
| D3 | What C105 checks | (a) presence and form only, from the ADR's acceptance date; (b) also staleness; (c) no check, brief lines only | **(a)**: (b) is a flaky gate by construction; (c) leaves the block to memory, which is the class |
| D4 | Push gate (RT-12) | (a) a `::warning::` listing moved heads of changed design documents; (b) an error; (c) nothing | **(a)**: shows the moves at the moment of committing without blocking unrelated pushes |
| D5 | Shipped claims | (a) cite `./dev evidence shipped` output and read the code at the tag; (b) `git tag --contains` alone | **(a)**: (b) misses code removed after the introducing commit |
| D6 | Wiki pages (ER-30) | (a) keep `verified:` dates; a page may add a block, the wiki lint does not require it; (b) require blocks on wiki pages | **(a)**: wiki pages summarise main; their branch facts are already marked "on branch X at commit Y" |
| D7 | Rule text | (a) one new Paddock rule (number taken at landing) and the two lines in engineering-rules `checklists/pre-review.md` (ER-36); (b) amend ER-30 | **(a)**: ER-30 is about the wiki; this is about design documents and reviews |

## Work packages (raw agent-days, one agent)

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-1 | `./dev evidence check` and `--accept`: block parser, git reads through a `Result`, the five states, exit codes 0/5/6; tests on a throw-away git repository in the shared test scratch (ADR 0078) with a planted moved, rewritten, merged and gone head each (positive controls); goldens of the text output | 1 | – |
| WP-2 | `./dev evidence shipped` with `--app` and `--path`; tests on a throw-away repository with two tags | 0.25 | WP-1 |
| WP-3 | C105 in the RT-5 host (`check/c105.rs`, one registry line), good and bad fixtures, its control | 0.5 | RT-5 landed |
| WP-4 | Rule and lesson (D7), reviewer brief lines in both repositories, two pre-review checklist lines, the block described in `docs/adr/README.md` (no ADR template file exists under `docs/adr/` or `templates/` today) | 0.25 | – |
| WP-5 | Blocks for the open proposed ADRs (0090 and any later ones) and RT-12's warning (D4) | 0.5 | WP-1; RT-12 for the warning |
| | **Total** | **2.5** | |

WP-1, WP-2 and WP-4 can start now; WP-3 waits for RT-5 to land; WP-5's warning waits for RT-12.

**Tag gating.** None. Nothing here is on an app's release path; no tag waits for it.
