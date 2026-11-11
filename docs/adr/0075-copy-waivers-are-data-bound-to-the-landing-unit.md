# 75. Copy waivers are data, bound to the landing unit, not commit trailers

- Status: Accepted 2026-11-09 by the user: D1–D10 as recommended (revision 3, after independent review rounds 1
  (P1-1 to P1-5, P2s) and 2 (P2-1 to P2-5, P2-7)). Amends
  ADR 0034 §3 (waivers, the entry guard's trailers, the range check's unit) and the trailer parts of rule 55 (a), (c) and
  (d). Everything else in ADR 0034 (the register, members, counted lines, modes a and c, the freeze rule) stays as it is.
- Trimmed 2026-11-11 (user, comparable-practice review): DROPPED: the unit walk is moot since a reached delete-by no longer blocks a tag (rule 55); `docs/known-copies.toml` stays as documentation.
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 (share first), ER-33 / rule 71 (a check
  counts only if its probe can see the thing), ER-36 (improve the process), rule 48 (benchmarks recorded), rule 55 and
  ADR 0034 (one implementation per mechanism, check C58), ADR 0022 (Rust tools, `./dev`), ADR 0042
  (`Batch-size-waived:`), ADR 0049 (`Switch-date:`, `Freeze-closed:`, `Release-part:`, landing trains, `merge=union`),
  rule 50 (CI economy), C23 (`conformance/src/commit-lint.mjs`), `paddock.waivers.toml` and
  `conformance/src/util/waivers.mjs` (the conformance run's data waivers).
- Evidence: read in git on 2026-11-09 at `origin/main` `b51c72509` and the remote branches named below. Two throwaway
  prototypes over the repository's own history (no user data): a trailer audit, and the unit walk and mode b of this ADR
  with trailers honoured. Both approximate ADR 0034 §2: comment lines and `#[cfg(test)]` modules are masked by line
  patterns, `item:` spans by a brace walk without string masking. Their counts are estimates; they live in the scratch
  folder of the change and are not part of the repository. Timings: [benchmarks](../benchmarks.md#2026-11-09-c58-prototype-unit-walk-and-mode-b-adr-0075).

## Pattern

**A `Copies-*` trailer that is wrong, missing or on the wrong commit cannot be corrected, because Paddock never rewrites
shared history.** Force pushes are denied (`.claude/settings.json` deny list and `.claude/hooks/no-force-push.sh` in the
coordinator's checkout; both untracked), and landings are merges, never rebases or squashes (ER-31 (d), ADR 0049 D6), so a
commit message is final once pushed. ADR 0034 §3 binds each waiver to the one commit that carries it, and only if that
commit itself touches the mechanism. Every mistake then needs a coordinator decision outside the rules. C58, which would
catch the mistake before the push, does not exist on any branch (no `C58*` file under `conformance/src/checks/` on any
remote branch). Only reviewers catch these, after the push.

**Invariant that should hold:** every rule 55 finding can be fixed by a further ordinary commit, and the checker tells the
author exactly what to add before the branch is pushed.

## Context: the incidents

All on 2026-11-09 unless dated otherwise.

| # | Where | What happened | Why it could not be fixed |
|---|---|---|---|
| 1 | `fix/debrief-lint-1-temp-client` `2e9ffb7a3` | Changed `apps/debrief/crates/chromium-driver/src/launch.rs` (the Debrief copy of `browser-driver`) and waived *that* member, spelt as a file rather than the register's member path (`apps/debrief/crates/chromium-driver/src/**`). The two members it left unchanged (`crates/paddock-chromium/src/**`, `apps/spotter/crates/spotter-render/src/**`) had no waiver. | The message is final. |
| 2 | same branch, `c4f1d20fd` (six commits later) | The correct waivers came on a test-only commit (its 14 lines are inside `mod tests` of `launch.rs`). ADR 0034 §3 counts a waiver only in a commit that itself touches the mechanism, and test code does not count (§2). | No amend. |
| 3 | same branch, `b0481c236` | Third try, on a commit that changes only the doc comment of `Profile::Throwaway`. Comment lines do not count (§2). | As above. |
| 4 | `train/2026-11-08-1` `a03a24725` | The coordinator put `Copies-waived: companion-link ...` on the train's merge of `fix/dyno-agent-handshake-close`. The merge had conflicts (`CHANGELOG.md`, `docs/lessons.md`, per its message), none in a member; ADR 0034 does not say how a merge commit counts. | A decision outside the ADR. |
| 5 | `fix/dyno-agent-handshake-close` `6d70a2005`, `828f43397`, `25b559213` | `6d70a2005` changes `handshake.rs` (`ipc-frames-peer`) and `endpoint/*` (`ipc-endpoint`) with no trailer. `828f43397` and `25b559213` change `link/server.rs` (canonical of `companion-link`) but waive only `ipc-endpoint` and `ipc-frames-peer *: same` (a 4-character reason; ADR 0034 asks for at least 10). The `companion-link` waiver came only with `63b1a9016` and again on the merge (#4). | The messages are final. |
| 6 | `pitwall/1.1-b1b-b6` `84bcb9732` (2026-11-08) | A merge carries waivers "for the launchd-job-report canonical changes the B6 merge brought in without a trailer" (`6298ae11a`, `8f8f9b536`, `b72381f28`). | Same as #4. |
| 7 | `pitwall/1.1-b1b-b6` `9b32fd0cb` (2026-11-08; not on main) | A docs-only commit "carries" updater waivers for `1faab2a73`; it "does not itself touch the updater's sources" (its own message), so ADR 0034 ignores them. | Same as #2. |

**The trailer audit** of every `Copies-waived:` trailer on `origin/main` and the two open branches of #1 and #4 (99
trailers in 51 commits; `origin/main` alone carries 86 `Copies-waived:`, 11 `Copies-removed:` and no `Copies-date:`):

| Finding (estimate, see Evidence) | Trailers |
|---|---|
| the commit does not touch the waived mechanism's counted lines (ADR 0034 ignores the waiver) | 44 |
| the member path is not a member path of the register | 5 |
| reason shorter than 10 characters | 2 |
| on a merge commit (meaning undefined) | 1 |
| fine under ADR 0034 | 48 |

Most of the 44 are precautionary: `launchd-job-report` waivers on `paddock-service` commits that change other mechanisms'
files (`c0fad0085`, `247bb4aee`, `769324b20` change `manager.rs` or `backend/windows.rs`). Authors cannot run a checker,
so they guess.

**What the cutover would find (P1-4).** The prototype of §2-§3, with old trailers honoured (§5), over the 78 remote
branches not merged into `origin/main`, with a tip after 2026-11-02 and a diff that reaches a registered path: **61 are
red, in 73 distinct red units** (many branches share units, for example the Pitwall 1.1 lines). The most frequent
mechanisms: `dialog-windows` (29 unit findings), `service-registration` (26), `launchd-job-report` (25),
`tray-popover-footer` (16), `plugin-platform` (15). Some findings are the prototype's approximations (`item:` spans), most
are real unwaived touches nobody could see. Incident #5 stays red after the cutover: `ipc-frames-peer` is touched by
`6d70a2005` and both of its waivers are invalid. That is why §5 makes old units report-only.

The prototype also found a register defect: on `train/2026-11-08-1` the mechanism `rust-test-wait` has members made only of
test files (`crates/paddock-ipc/tests/it/clocked.rs` and others). A test-only member can never be touched (ADR 0034 §2), so
it would be required in every unit that touches its canonical. §3 exempts such members with a notice (D10).

The same exposure exists for every other trailer record: `Batch-size-waived:` (ADR 0042), `Switch-date:`,
`Freeze-closed:`, `Release-part:` (ADR 0049). This ADR fixes the `Copies-*` family and proposes the same mechanism for the
others later (D8).

## Decision

### 1. Records are data in `docs/known-copies.d/`

Waivers, removals and date moves are TOML entries in a sibling folder of the register, `docs/known-copies.d/`, one file
per branch, named after the branch with `/` replaced by `--` (`docs/known-copies.d/fix--debrief-lint-1-temp-client.toml`).
Two branches never conflict on it, and a reviewer sees a branch's records as one diff. The name is a convention for people;
the binding (§2) is by history, not by name. (D2 lists the other layouts.)

```toml
version = 1
branch = "fix/debrief-lint-1-temp-client"          # for people; not used for binding

[[waiver]]
mechanism = "browser-driver"
members = ["crates/paddock-chromium/src/**", "apps/spotter/crates/spotter-render/src/**"]   # register member paths, exactly
reason = "both already take the throwaway profile base from the caller; the Debrief copy now has the same shape"
# for = "c4f1d20fda70"   # optional: a commit of the unit this entry covers, when another unit adds it (§2)

[[removal]]                                         # replaces Copies-removed:
mechanism = "launchd-job-report"
member = "apps/pitwall/app/src-tauri/src/service_repair.rs"
reason = "deleted by b7151a57f when Pitwall's macOS service moved onto paddock_service::Manager (B1)"

[[date_move]]                                       # replaces Copies-date:
mechanism = "browser-driver"
member = "apps/debrief/crates/chromium-driver/src/**"
old = "debrief-v1.2.0"
new = "debrief-v1.3.0"
approved = "user"                                   # "coordinator" | "user"; a later release line needs "user" (rule 55 (d))
reason = "..."
```

Field rules:

- `mechanism` is a register id and each of `members` (or `member`) is exactly the first path of a member of that
  mechanism, **in the register at the tip of the unit the record belongs to** (§2, §3). The check resolves that path to
  the member's **identity**: the mechanism id plus `canonical`, or plus the copy's `key`. `key` is a new optional field
  of `[[mechanism.copy]]` (kebab-case, unique within the mechanism; default the copy's `owner`, so it is needed only where
  one owner has two copies of a mechanism). A copy keeps its identity when its paths are renamed, so a rename never
  turns a required member into a satisfied one (§3). `*` in `members` follows ADR
  0034 (at most two members besides wiring). `reason` has at least 10 characters. Incidents #1 and #5 fail here.
- **Lifetime.** A waiver has no date of its own. Its member's `delete_by` bounds it: once the member has left the register,
  the record is **stale**, which is a notice, never an error (P1-3; see §3). `copies --prune` deletes stale records; the
  owning app's release batch runs it with its register tidy-up, and mode c lists what is left as a notice. Until then a
  copy's waivers are its drift log: every difference from the canonical knowingly left in place, with the reason.
  `copies --drift <member>` prints that log across all files (record, reason, the commit that added it).
- **Size.** The folder is not bounded by the register: it grows with every unit that waives and shrinks only when copies
  leave. At today's rate (86 waivers on main in the nine days since 2026-10-31) and Debrief 1.2's copies living until
  about 2027-06, it could reach the low thousands of entries before the big prune. The entries are small and only the
  current range's are read; if size matters, D4's pruning option bounds it by landing age instead.
- A file with no entries left is deleted.

### 2. The unit, and how a record binds to it

ADR 0034 binds a waiver to a commit. This ADR binds every record to a **unit**: the commits that one branch brings to the
line it lands on. Units are computed from the history alone, without branch names or classification.

**Inputs.** A range `B..X` and a main-equivalent commit `T` (normally `origin/main`; pinned for replays). A commit
**on main** is an ancestor of `T`.

**The walk.** `Units(B, X)` visits the first-parent commits of `B..X`, oldest first, with one set of **claimed** commits
shared by the whole walk. Commits on main and commits already claimed are skipped. For each other commit `c`:

- a non-merge commit joins the **own-work unit** of the segment `B..X` (one per segment) and is claimed;
- a merge whose second parent is on main is a **main sync**; it has no content of its own and is claimed;
- any other merge `m` with second parent `P` is a **landing merge**: the walk recurses into `Units(merge-base(m^1, P), P)`
  and then makes `m` a **carrier** of the unit that recursion produced for `P`'s own segment (or, if the side brought
  nothing unclaimed, of a merge-only unit of its own). A carrier's message records and its own lines (`git diff --cc`,
  conflict resolution) belong to that unit.

So **a unit's commits are those reachable from `P` and not from `m^1`, minus commits on main and commits an earlier unit
claimed**, and each commit belongs to the first unit that reaches it. Consequences for the shapes the review named:

| Shape | Result |
|---|---|
| plain topic, merged into a train | one unit; its diff `merge-base(m^1, P)..P` |
| topic that synced `origin/main` | still one unit; the synced commits are on main and skipped |
| **stacked topic** (T2 branched from an unlanded T1) | if T1 landed first, its unit claimed T1's commits and T2's unit holds T2's own commits (and any T1 commits made after T2 branched are T1's); if T2 lands first, its unit holds the T1 commits it contains as well, and T1's later merge holds only T1's commits T2 did not bring, or is a merge-only unit |
| **upstream sync** (a line merged into a downstream line, `5ff58bd04` merges `pitwall/1.1-b1` into `pitwall/1.1-b1b`) | expanded: the upstream's own landing merges become their own units and its direct commits its own-work unit, each holding only commits the downstream did not have; never one lump |
| **release line off a release line** | the upstream's landing merges stay separate units; its direct commits fall into the downstream's own-work unit unless the upstream landed first (a known limit; the steward lands upstream lines first) |
| `84bcb9732` (P1-1's example) | one unit of **1 commit** (`5cd3ff15d`) with `84bcb9732` as carrier, not 212; the whole `pitwall/1.1-b1b-b6` line walks into 36 units over 206 commits |

**A topic branch's own check** is `Units(merge-base(T, HEAD), HEAD)` with the same walk. A plain topic gives one unit; a
stacked topic includes its unlanded base's commits in its own unit. That unit is larger than the one the train will form
if the base lands first, so the topic's own result can differ from its result at landing in either direction (a base's
waiver can cover the topic, or the base's touches can require more of it); the steward's landing check is the one that
counts. There is no
"train form": the walk is the same everywhere.

**The touches of a unit** (ADR 0034 §2: counted lines, test and comment code masked) come from the net diff
`merge-base(m^1, P)..P` when the unit's commits are exactly `rev-list merge-base(m^1, P)..P` **after leaving out main
syncs and commits on main** (the common case, synced topics included; when the line is older than the main a topic
synced, the net diff would carry main's newer changes, so that case is the union one), and otherwise from the union of
its commits' own diffs. Both add the carriers' `--cc` lines. In the union case a change and its revert inside the same
unit both count; the check prints a notice saying so. (The prototype behind the numbers in this ADR does not read the
carriers' `--cc` lines; WP-1 keeps the fixture for a conflict resolution that edits a member.)

**A record belongs to a unit** when one of that unit's commits (or carriers) adds it or changes it. Entries are compared
after normalising the TOML, so an edited entry belongs to the unit that edits it. An entry with
`for = "<sha prefix, 12 or more hex digits>"` belongs instead to the unit that claims that commit in the checked range;
any commit of the unit will do (its first, last or a carrier). That covers:

- a unit already merged into a train that lacks a waiver: any later commit adds an entry with `for`;
- **a branch landed in parts** (merged into a train twice): each merge is its own unit (the second holds only the new
  commits), and `for` names a commit of the part it means;
- **a revert split across units**: the revert is a touch in the later unit and needs its own alike change or waiver
  ("reverts <sha> of the earlier unit" is a valid reason).

A `for` naming a commit already on main is a record only (main is not re-checked), printed as a notice. A `for` naming a
commit outside the range is a notice ("waives nothing here").

Why the unit and not the commit: a waiver anywhere on a branch counts for that branch's changes, so incidents #2, #3, #6
and #7 disappear and #4 has a defined place. A waiver cannot leak to another branch in the same train, which was ADR
0034's reason for binding waivers to commits. Within one branch the waivers cover all of its changes; a branch has one
author and one review, and a branch that needs separate judgements is split.

### 3. The check (C58, amended)

ADR 0034 §3 stays the specification of counted lines and modes a and c. Mode b and the record handling change:

- **Mode a** (`copies --check-register`, at the head) validates the folder's syntax and fields, and that copy `key`s
  are unique within a mechanism. A record whose member identity is absent from the **head** register is a **stale
  notice**, not an error, so a branch that deletes a copy never makes another branch's record invalid (P1-3); a renamed
  copy keeps its identity, so its records stay valid. A member whose paths are all non-counted (test files, manifests,
  docs) can never be touched; it is **exempt from touch alike with a notice** (D10), and mode a prints it once per run.
- **Mode b** (`copies --range <B>..<X> [--main <T>] [--worktree]`) walks the units (§2) and judges **each unit against
  the register at that unit's tip** (`P` for a landing merge, the last own commit otherwise). For each unit:
  - an untouched, unwaived member: **error**, with the TOML entry to paste; `--suggest` writes it into the branch's file
    with `reason = "TODO"`, which mode a rejects until filled in;
  - a required member whose identity is absent from the **head** register (deleted by another branch since): satisfied,
    with a notice (P1-3: no deadlock between a branch that deletes copy K and one that waives K). A member renamed since
    (same identity, other paths) **stays required**: the error names its head paths, and a waiver for it may name either
    path;
  - a waiver naming a member the unit **changed**: **error** ("a waiver is for members left unchanged; delete it")
    (incident #1);
  - a waiver naming a mechanism the unit does not touch: **warning** ("waives nothing"), like W003, so a record in the
    wrong unit is never an error that only a rewrite could fix;
  - the entry guard of ADR 0034 §3b reads the unit's `removal` and `date_move` records instead of trailers.
  Each run prints per unit its commits, carriers, net or union diff, counted lines per mechanism and the records used and
  unused (ER-27), and a summary line (units, red units, time).
- **Mode c** (`copies --tag`) is unchanged, plus a notice for stale records of the tag's app.
- `--worktree` adds the uncommitted changes to the newest unit, so the author sees the result before committing.

### 4. No amend, no rebase, and landing trains

| Mistake | Fix by a new commit |
|---|---|
| missing waiver on a topic branch | add the entry to the branch's file |
| wrong member, short reason, waiver for a changed member | edit or delete the entry |
| missing waiver for a unit already merged into a train | any later commit adds an entry with `for` |
| waiver in the wrong branch or commit | a warning; delete it whenever |
| missing waiver for a unit already on main | a `for` entry as a record; no gate is involved |
| another branch deleted the waived copy | nothing: stale notice, satisfied |
| another branch renamed the waived copy | nothing: the record follows the copy's identity |

Where the check runs: the author before pushing (`copies --range $(git merge-base origin/main HEAD)..HEAD --worktree`;
the `./dev` pre-push check of ADR 0049 calls it when the branch reaches a registered path), the plan job on every branch
run next to commit-lint, and the steward before merging a topic into a train and before landing a train on main (the
same walk over `origin/main..<train>`). The landed train is not re-checked on main.

### 5. Existing trailers and the cutover

- **The cutover is read from history, per commit** (no marker that names itself): a commit is **post-cutover** when its
  first parent's tree contains `docs/known-copies.d/README.md`, which WP-4 adds. One `git cat-file -e` per commit.
- **The cutover date** is the committer date of the first commit on main whose tree contains that README.
- **Old units are report-only, for at most 14 days.** A unit none of whose commits or carriers is post-cutover (by
  ancestry, or by a committer date on or after the cutover date) is judged and printed, but its errors are warnings: the
  61 red branches above keep landing as they would today, and the steward and reviewer still see the findings. In such
  a unit a `Copies-waived:`, `Copies-removed:` or `Copies-date:` trailer on any of its commits or carriers counts as a
  record of the unit; an invalid old trailer waives nothing.
- **New units are enforced.** A unit is enforced when any of its commits **or carriers** is post-cutover. So a
  pre-cutover branch that a steward merges into a train after the cutover is enforced at that landing (the merge is a new
  act, judged by today's rule; its missing waivers are added as data with `for`). Old trailers inside an enforced unit
  still count.
- **The window closes.** From the cutover date plus 14 days, **every unit is enforced, whatever its branch base**; the
  check reads the date from its run's clock, and replays pass a pinned `--as-of` so they stay deterministic. A branch
  still open by then adds its waivers as data (`copies trailers-to-data` and `--suggest` write them).
- **Trailers retired.** C23 rejects a `Copies-*:` trailer on any commit that is post-cutover by ancestry **or** whose
  committer date is on or after the cutover date (so a commit made today on an old branch is caught too), **merges
  included** ("write it in
  `docs/known-copies.d/<branch>.toml`, ADR 0075"). Today `messagesInRange` lists merges only with `--first-parent` and
  `lint(message)` sees no ancestry, so this is a new range-level function in `commit-lint.mjs` over all commits of the
  range, not a change to `lint()`. The escape for a commit that must keep such a trailer (a cherry-pick of an old commit) is
  the existing C23 commit waiver in `paddock.waivers.toml` (`check = "C23"`, `path = "commit <sha>"`, with `expires`);
  its trailer still waives nothing.
- **History on main** is not migrated; landed units are never re-checked. `copies trailers-to-data --range <r>` prints the
  entries for an open branch that wants to convert.

### 6. Positive controls (ER-33)

A C58 run counts only if it has shown that it can see a violation.

- **Fixtures**, in temporary git repositories built by the tests with invented content, red and green for each rule,
  including: a plain topic; a topic with a main sync; a **stacked topic** landed before and after its base; a **release
  line off a release line**; an **upstream sync** merge; a landing merge whose conflict resolution edits a member; a
  test-only and a comment-only commit; a `for` entry for a merged unit, for a branch landed in parts and for a split
  revert; a deleted and a renamed copy with another branch's waiver (no deadlock); old trailers before and after the
  cutover and after the 14-day window; a post-cutover carrier of an old branch (enforced); a renamed copy with another
  branch's waiver (still required, record follows); a test-only member (exempt, notice).
- **Golden replays of real history**, pinned so that they never change: each names its main-equivalent `T`, its base and
  its tip explicitly, and the commits are kept alive by tags `refs/tags/c58-golden/<name>/{main,base,tip}` that WP-5
  pushes; the plan job fetches `+refs/tags/c58-golden/*:refs/tags/c58-golden/*`. Expected results are pinned per mechanism,
  so other findings in the same range do not matter. With `T = b51c72509`:

  | Replay | Base | Tip | Expected (prototype result) |
  |---|---|---|---|
  | lint-1 at incident #1 | `4772dc8ce` | `2e9ffb7a3` | `browser-driver`: errors for `crates/paddock-chromium/src/**` and `apps/spotter/crates/spotter-render/src/**`; a warning for the old trailer (not a member path) |
  | lint-1 after #2 and #3 | `4772dc8ce` | `b0481c236` | `browser-driver`: no error |
  | handshake before the fix | `b51c72509` | `25b559213` | `companion-link`: error; `ipc-frames-peer`: error |
  | handshake at its tip | `b51c72509` | `63b1a9016` | `companion-link`: no error; `ipc-frames-peer`: error (incident #5 stays red) |
  | `84bcb9732` alone | `84bcb9732^1` | `84bcb9732` | one unit, one commit (`5cd3ff15d`), carrier `84bcb9732`, `launchd-job-report` touched, no error |
  | `pitwall/1.1-b1b-b6` | `96c2a16ae` | `e20ae50c3` (the line's tip on 2026-11-09) | 36 units, 206 commits; the unit of `84bcb9732` has one commit |

  The prototype gave every expected result above. The replays are judged as old units with errors kept (the replay
  asserts the findings, not the gate).
- **A replay failure fails only the C58 step** with `check broken` and the replay's name; other steps and jobs are not
  affected, and the step's real-range result is not trusted for that run.
- **No silent zero.** A range whose diff reaches registered paths but yields zero touched units is `check broken`; a range
  with no commits is `not measured: no commits`.

## Alternatives considered

1. **Keep trailers, bind them to the unit, and allow empty commits that only carry trailers** (a carrier commit like
   `9b32fd0cb` becomes valid). It fixes #2 to #7 and costs about one agent-day less. A wrong trailer stays in history and
   must be downgraded to a warning, the author cannot validate a trailer before committing, and the reviewer reads waivers
   in `git log`, not in the diff. **Fallback** if D1 is declined; the unit walk, the cutover and the controls are the
   same.
2. **`git notes`.** Notes can be added later, but `refs/notes/*` is not fetched by default or by the CI checkout, a
   concurrent edit needs a notes merge or a forced update (which the push rules forbid), and notes are not in a branch's
   diff (`git-notes(1)`, https://git-scm.com/docs/git-notes; read, not tried; whether Gitea shows notes is unverified).
   Rejected.
3. **Allow amend and force-push for trailer fixes.** The mistake is usually found in review, after the push, and the
   force-push rule protects every agent's worktree. Rejected.
4. **Waivers in `paddock.waivers.toml`.** That file waives a check for a path glob for everyone until a date, so a copy
   waiver would leak to every branch, and one shared file conflicts. Its W001-W003 semantics are reused.
5. **Bind data records to commits (`commit = "<sha>"`).** A commit cannot name its own sha; with no amend the entry
   always lands one commit late, which is incident #2 again. Rejected.
6. **Classify branches as topic or train** (by name or by structure), as revision 1 did. By revision 1's structural rule, 42 of
   230 topic-named branches would have been classified as trains, among them the replay branch of incident #1 (review
   round 1; not re-measured); `pitwall/1.1-wp2a` and
   `pitwall/1.1-b1b-b6` share a prefix but not a role. Replaced by the one recursive walk.

## Consequences

- Rule 55 (a), (c) and (d) change from trailers to records (proposed text below); C23 gains one range rule.
- Reviewers read waivers in the diff, next to the code they excuse.
- A branch's waivers cover the whole branch; split a branch that needs separate judgements.
- The 61 currently red branches are not blocked for the first 14 days (report-only old units); their findings become visible at once, which is
  itself the backlog that rule 55 was meant to prevent. The steward decides whether to waive them as data or fix them.
- The precautionary trailers stop: `--suggest` writes only what a unit needs.

Proposed rule 55 text for the user (not applied while Proposed): (a) "... or waives a member with a `[[waiver]]` entry in
the branch's file under `docs/known-copies.d/` (ADR 0075) ..."; (c) "... or with a `[[removal]]` entry there"; (d) "... a
delete-by date moves only with a `[[date_move]]` entry naming its approver ..."; (e) unchanged, plus "every finding is
fixed by a further commit; history is never rewritten for it".

## Parallelism and cost

- **Measured** ([benchmarks](../benchmarks.md#2026-11-09-c58-prototype-unit-walk-and-mode-b-adr-0075), load 19-27):
  the prototype's full mode b (walk, diffs, masking, judging) takes 0.12-0.13 s for a one-commit topic, 0.30-0.33 s for
  the handshake topic (7 commits), 0.13-0.17 s for `84bcb9732`, 3.65-3.81 s for `train/2026-11-08-1` (9 units, 83 commits)
  and 10.1-11.5 s for `pitwall/1.1-b1b-b6` (36 units, 206 commits); the 78-branch sweep took 161 s at load 22-41. The
  reviewer's raw diff loop (no walk, no judging) took 0.42-0.67 s for the train and 1.46-1.53 s for 90 main commits at
  load 32.
- **Where the time goes:** the union units (52 and 56 commits on the Pitwall line) spawn one `git diff` and up to two
  `git show` per commit. **Cheaper first:** one `git log -p --first-parent`-style pass per unit restricted to the register's
  pathspecs, `merge-base --is-ancestor` replaced by one `rev-list T` set, blobs read only for files with counted hunks,
  and units without a registered path skipped before any blob is read. Target: under 2 s for the Pitwall line.
  **Then parallel:** units are independent once the walk has claimed commits; judging them on up to cores-minus-one
  workers (an "Auto" default, ER-20) is the second step, measured against the batched serial version.
- **Agent work.** WP-1 and WP-4 can run in parallel; WP-2 starts from WP-1's interface; WP-5 needs WP-2. No app builds;
  one small Rust tool build (`./dev`), so machine load is light (ER-24).

## Work packages

This replaces ADR 0034 WP-1 (2.5 d, not started). Estimates assume D7's recommendation (Rust in `./dev`).

| WP | What | Size | Depends on |
|---|---|---|---|
| WP-1 | The unit walk (§2: claiming, main syncs, landing merges, recursion, carriers, net or union diff, `for`) over `paddock-repo-graph`'s `History` port, with fixtures for every shape in §6 | 1.25 d | none |
| WP-2 | C58 modes a-c (ADR 0034 §2-§3 with this ADR's changes): register and folder parsing with the `toml` crate, counted lines and masking (`item:` walker as C39's `maskRust`), touch alike per unit against the unit-tip register, stale notices, member identity and the copy `key` field, test-only members exempt, errors and warnings, `--worktree`, `--suggest`, `--drift`, `--prune`, entry guard from records; fixtures for every rule | 1.75 d | WP-1 interface |
| WP-3 | Speed: the batched diff pass and, if measured to help, parallel judging; results block per rule 48 | 0.5 d | WP-2 |
| WP-4 | Cutover: report-only old units with the 14-day window and `--as-of`, enforced post-cutover carriers, old trailers as records, the C23 range rule (merges included, by ancestry or date) with its waiver escape, `trailers-to-data`, `docs/known-copies.d/README.md`, register header, rule 55 wording (after acceptance), lessons entry | 0.5 d | none |
| WP-5 | Controls and wiring: the golden replays and their `c58-golden` tags, `check broken` confined to the C58 step, the plan job, the `./dev` pre-push call, the steward's landing call, the C58 entry of `paddock-conformance` calling `./dev copies`, ADR 0034 status | 0.5 d | WP-2 |
| | **Total** | **4.5 agent-days** | |
| WP-6 | Follow-up (D8): `Batch-size-waived:`, `Switch-date:`, `Freeze-closed:`, `Release-part:` on the same walk | separate ADR | after a week of C58 |

Rollback: C58 is a check, not a runtime change. If it misbehaves, its plan-job step is switched to report-only (one line
in `ci.yml`) and C23's trailer rule is reverted; the records keep working as documentation and reviewers check by hand.

## Decisions for the user

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | How waivers are recorded | (a) data records bound to the unit; (b) trailers bound to the unit, plus trailer-only carrier commits (alternative 1); (c) keep ADR 0034 as is | **(a)**; fallback (b) |
| D2 | Where the records live | (a) one file per branch in `docs/known-copies.d/` (no conflicts, one diff per branch); (b) one file per copy, `docs/known-copies.d/<mechanism>/<copy>.toml` with `merge=union` (drift log in one place; union merges can garble two edits of the same entry); (c) inside the register under each copy (every branch edits a 945-line file) | **(a)**, with `copies --drift` for the per-copy view |
| D3 | The binding unit | (a) the recursive walk of §2 (a branch's commits not already claimed); (b) per commit, as ADR 0034 (cannot be corrected without a rewrite); (c) the whole range as one unit (leaks between branches of a train) | **(a)**, accepting that one branch's waivers cover the whole branch |
| D4 | Lifetime of waivers | (a) kept as the copy's drift log until the copy leaves, pruned as stale by the owner's release batch; (b) pruned after the unit lands (by age, for example 30 days after landing on main; bounded, loses the drift log); (c) kept forever | **(a)** |
| D5 | A waiver that waives nothing | (a) warning; (b) error | **(a)**, so a misplaced record never needs a rewrite |
| D6 | Cutover | (a) old units report-only for at most 14 days, a post-cutover carrier enforces its unit, then every unit enforced; C23 rejects new trailers by ancestry or date (61 of 78 active branches would otherwise be red at once); (b) enforce everything at the cutover after a one-off waiver sweep of the 73 red units (about one agent-day, reviewed); (c) keep trailers and data side by side for good | **(a)**; run (b)'s sweep for the release lines that tag next if the findings show real drift |
| D7 | Implementation | (a) Rust in `./dev copies` on `paddock-repo-graph`'s `History` port and the `toml` crate already in its lockfile, with `paddock-conformance`'s C58 calling it: 4.5 d, no port later; (b) JavaScript in `conformance/` next to C23 (needs a TOML parser: `smol-toml` is in the lockfile but not a conformance dependency, and `util/waivers.mjs` parses only a string subset): about 4 d now plus 1-1.5 d to port when the Rust move reaches conformance | **(a)** |
| D8 | The other trailer records (ADRs 0042 and 0049) | same walk and data records in a follow-up ADR, or keep them as trailers | **Follow-up ADR** after C58 has run a week |
| D9 | `for` entries for units already on main | record only (notice), or rejected | **Record only** |
| D10 | Members made only of test files (today `rust-test-wait` on `train/2026-11-08-1`) | (a) exempt from touch alike with a notice; (b) a declared `kind = "test"` for such copies, counted by its own rule (test files counted for that mechanism only); (c) a mode a error until the register is changed | **(a)** now; (b) if a test-only mechanism ever needs its copies kept alike |
