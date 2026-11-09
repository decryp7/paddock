# 75. Copy waivers are data, bound to the landing unit, not commit trailers

- Status: Proposed (2026-11-09). Amends ADR 0034 §3 (waivers, the entry guard's trailers, the range check's unit) and
  the trailer parts of rule 55 (a), (c) and (d). Everything else in ADR 0034 (the register, members, counted lines,
  modes a and c, the freeze rule) stays as it is.
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 (share first), ER-33 / rule 71 (a check
  counts only if its probe can see the thing), ER-36 (improve the process), rule 55 and ADR 0034 (one implementation per
  mechanism, check C58), ADR 0042 (`Batch-size-waived:`), ADR 0049 (`Switch-date:`, `Freeze-closed:`, `Release-part:`,
  landing trains), rule 50 (CI economy), C23 (`conformance/src/commit-lint.mjs`), `paddock.waivers.toml` and
  `conformance/src/util/waivers.mjs` (the existing data waivers of the conformance run).
- Evidence: read in git at `origin/main` `b51c72509`, `origin/fix/debrief-lint-1-temp-client` and
  `origin/train/2026-11-08-1` on 2026-11-09. The trailer audit below was made with a throwaway script over the repository's
  own history (no user data); it matches member paths only (no `item:` spans) and masks `#[cfg(test)]` modules and
  comment lines approximately, so its counts are an estimate.

## Pattern

**A `Copies-*` trailer that is wrong, missing or on the wrong commit cannot be corrected, because Paddock never rewrites
shared history.** Force pushes are denied (`.claude/settings.json` deny list and `.claude/hooks/no-force-push.sh`, local to
the coordinator's checkout), landings are merges, never rebases or squashes (ER-31 (d), ADR 0049 D6), so a commit message
is final the moment it is pushed. ADR 0034 §3 binds each waiver to the one commit that carries it, and only if that commit
itself touches the mechanism. Every mistake therefore needs a coordinator decision outside the rules, and C58, which would
catch the mistake before the push, does not exist on any branch (no `C58*` file under `conformance/src/checks/` on any
remote branch, 2026-11-09). Only reviewers catch these, after the push, when nothing can be changed.

**Invariant that should hold:** every rule 55 finding can be fixed by a further ordinary commit, and the checker tells the
author exactly what to add before the branch is pushed.

## Context: the incidents

All on 2026-11-09 unless dated otherwise.

| # | Where | What happened | Why it could not be fixed |
|---|---|---|---|
| 1 | `fix/debrief-lint-1-temp-client` `2e9ffb7a3` | Changed `apps/debrief/crates/chromium-driver/src/launch.rs` (the Debrief copy of `browser-driver`) and waived *that* member, spelt as a file (`.../src/launch.rs`) rather than the register's member path (`apps/debrief/crates/chromium-driver/src/**`). The two members it left unchanged (`crates/paddock-chromium/src/**`, `apps/spotter/crates/spotter-render/src/**`) had no waiver. | The message is final. |
| 2 | same branch, `c4f1d20fd` | The correct waivers were added on the next commit, which is test-only (its 14 lines are inside `mod tests` of `launch.rs`). ADR 0034 §3: "a waiver counts only in a commit that itself touches that mechanism"; test code is not counted (§2). The waivers count for nothing. | A test commit cannot carry them; no amend. |
| 3 | same branch, `b0481c236` | Third try on a commit that changes only the doc comment of `Profile::Throwaway`. Comment-only lines are not counted (§2): ignored again. | As above. |
| 4 | `train/2026-11-08-1` `a03a24725` | The coordinator put `Copies-waived: companion-link ...` on the train's merge of `fix/dyno-agent-handshake-close`. ADR 0034 does not say how a merge commit counts; `git show --cc` of a clean merge is empty, so a per-commit reading sees no touch there. | A decision outside the ADR. |
| 5 | `fix/dyno-agent-handshake-close` `828f43397`, `25b559213` | Both change `crates/paddock-ipc/src/link/server.rs` (canonical of `companion-link`) but waive only `ipc-endpoint` and `ipc-frames-peer *: same` (a 4-character reason; ADR 0034 asks for at least 10). The `companion-link` waiver came only with `63b1a9016` and again on the merge (#4). | The messages are final. |
| 6 | `pitwall/1.1-b1b-b6` `84bcb9732` (2026-11-08) | A merge carries waivers "for the launchd-job-report canonical changes the B6 merge brought in without a trailer" (`6298ae11a`, `8f8f9b536`, `b72381f28`). | Same as #4. |
| 7 | `9b32fd0cb` on main (2026-11-08) | A docs-only commit "carries" updater waivers for `1faab2a73`; it "does not itself touch the updater's sources" (its own message), so under ADR 0034 they count for nothing. | Same as #2. |

The audit of every `Copies-waived:` trailer on `origin/main` and the two open branches above (99 trailers in 51 commits):

| Finding (heuristic, see Evidence) | Trailers |
|---|---|
| the commit does not touch the waived mechanism's counted lines (so ADR 0034 ignores the waiver) | 44 |
| the member path is not a member path of the register (a file inside a member, or a file not in the register) | 5 |
| reason shorter than 10 characters | 2 |
| on a merge commit (meaning undefined) | 1 |
| fine under ADR 0034 | 48 |

Most of the 44 are precautionary: authors add `launchd-job-report` waivers to every `paddock-service` commit
(`c0fad0085`, `247bb4aee`, `769324b20` change `manager.rs` or `backend/windows.rs`, which belong to other mechanisms)
because they cannot run a checker to see which mechanisms they touch. The trailers are noise in about half the cases and
wrong or ignored in the cases that matter.

The same exposure exists for every other trailer-based record: `Copies-removed:` (11 lines on main), `Copies-date:` (1),
`Batch-size-waived:` (ADR 0042), `Switch-date:`, `Freeze-closed:`, `Release-part:` (ADR 0049). This ADR fixes the
`Copies-*` family and proposes the same mechanism for the others in a follow-up (decision D8).

## Decision

### 1. Records are data in `docs/known-copies.d/`, one file per branch

Waivers, removals and date moves are TOML entries in a sibling folder of the register, `docs/known-copies.d/`. Each branch
that needs one writes them into its own file, named after the branch with `/` replaced by `--`
(`docs/known-copies.d/fix--debrief-lint-1-temp-client.toml`). One file per branch means two branches never conflict on
it, and a reviewer sees a branch's records as one diff. The name is a convention for people; the binding (§2) is by diff,
not by name.

```toml
version = 1
branch = "fix/debrief-lint-1-temp-client"          # for people; not used for binding

[[waiver]]
mechanism = "browser-driver"
members = ["crates/paddock-chromium/src/**", "apps/spotter/crates/spotter-render/src/**"]   # register member paths, exactly
reason = "both already take the throwaway profile base from the caller; the Debrief copy now has the same shape"
# for = "a03a24725b41"   # optional: the unit this entry covers when it is not the unit that adds it (§2)

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

Rules for the fields (checked by mode a, so they fail before a commit is pushed):

- `mechanism` is a register id; each of `members` (or `member`) is exactly the first path of a member of that mechanism
  in the register at the branch head (a canonical's first path, or a copy's). `*` is allowed in `members` only under
  ADR 0034's rule (at most two members besides wiring). Incident #1's spelling fails here.
- `reason` is at least 10 characters (incident #5 fails here).
- **Lifetime.** A waiver has no date of its own: it lives as long as the copies it names. When a copy leaves the register,
  the same change deletes the waivers that name it (mode a error otherwise), so a copy's waivers are its drift log: the
  list of every difference from the canonical that was knowingly left in place, with the reason. That makes the copy's
  eventual deletion easier to plan. `removal` and `date_move` entries are history once their unit has landed; mode a does
  not require them to be deleted, and the release batch of the owning app deletes them with its register tidy-up (mode c
  lists them as a notice).
- A file with no entries left is deleted.

### 2. The unit, and how a record is bound to it without commit messages

ADR 0034 binds a waiver to a commit. This ADR binds every record to a **unit**, the smallest piece of history that one
author changes and one review approves:

- **On a topic branch** (no landing merges on its first-parent line, see below): the whole branch is one unit, its diff
  `merge-base(origin/main, HEAD)..HEAD`. Merges of `origin/main` into the branch (sync merges) are excluded by the
  merge-base, as they are for the affected plan.
- **On a train or release branch** (its first-parent line since the merge-base contains at least one landing merge): each
  **landing merge** `M` is one unit, with the diff the topic branch had, `merge-base(origin/main, M^2)..M^2`, plus the
  lines the merge commit itself adds relative to both parents (conflict resolution, `git diff --cc`). Each maximal run of
  consecutive direct (non-merge) commits on the train's first-parent line is one unit. A **sync merge** (its second
  parent is an ancestor of `origin/main`) is skipped and ends a run.
- A **landing merge** is a merge whose second parent is not an ancestor of `origin/main`. The check decides topic or train
  from the history alone; it reads no branch names.
- **When a train lands on main**, the steward runs the train form over `origin/main..<train>` before the merge (§4). A
  landing merge whose second side is itself a train is expanded into that train's units (recursively).

A record **belongs to a unit** when the unit's diff adds it or changes it (an entry is compared after normalising the TOML,
so an edited entry counts as new in the unit that edits it). An entry with `for = "<sha prefix, 12 or more hex digits>"`
belongs instead to the unit whose landing merge or last direct commit has that sha, if that unit is in the checked range.
That is the escape for a unit that has already landed on a train: a later topic or a steward commit adds the missing
waiver with `for`, and the earlier unit turns green. A `for` naming a unit already on `origin/main` is accepted as a
record only (main is not re-checked) and is printed as a notice.

Consequences of the unit:

- Incidents #2, #3 and #5 disappear: a waiver anywhere on the branch counts for the branch. Incident #4 and #6 have a
  defined answer (the branch's records, or a `for` entry), and no merge message carries anything.
- A waiver cannot leak to another branch in the same train, which is the reason ADR 0034 bound waivers to commits. Within
  one branch it applies to all of that branch's changes; the branch has one author and one review, so this is accepted.
- A unit is known before anything is committed (the branch's diff against the merge-base), so the checker can run on the
  working tree (`--range ...` plus the uncommitted changes with `--worktree`) and tell the author what to add before the
  commit exists.

### 3. The check (C58, amended)

ADR 0034 §3 stays the specification of modes a and c and of counted lines; mode b changes as follows.

- **Mode a** (`copies --check-register`) also validates every file in `docs/known-copies.d/` (§1 rules).
- **Mode b** (`copies --range <base>..HEAD [--worktree]`): computes the units (§2). For each unit: the touched members
  (ADR 0034 §2: non-test source, `#[cfg(test)]` and comments masked, whitespace ignored), then touch alike, with the
  unit's waivers. Errors and warnings per unit:
  - an untouched, unwaived member: **error**, with the exact TOML entry to paste (`--suggest` writes it into the branch's
    file with a `reason = "TODO"` that mode a rejects until filled in);
  - a waiver naming a member that the unit **changed**: **error** ("you changed it; a waiver is for members left
    unchanged; delete the entry") (incident #1);
  - a waiver naming a mechanism the unit does not touch: **warning** ("waives nothing; delete it"), like W003 for the
    conformance waivers. A warning, not an error, so that a record in the wrong unit is never an error that only a
    rewrite could fix; the fix for the unit that needed it is a `for` entry;
  - the entry guard of ADR 0034 §3b reads `removal` and `date_move` records of the unit instead of trailers.
  - Each run prints, per unit, its kind and diff, its counted lines per mechanism, the waivers used and unused (ER-27).
- **Mode c** (`copies --tag`) is unchanged, plus the notice of §1 for landed `removal` and `date_move` entries.
- The units walk and the record binding are one module, `conformance/src/util/units.mjs`, so the other trailer
  mechanisms (D8) can reuse them (ER-22).

### 4. No amend, no rebase, and landing trains

Nothing in this ADR needs a commit to change after it is pushed:

| Mistake | Fix by a new commit |
|---|---|
| missing waiver on a topic branch | add the entry to the branch's file |
| wrong member, short reason, waiver for a changed member | edit or delete the entry |
| missing waiver for a unit already merged into a train | any later commit adds an entry with `for = "<landing merge sha>"` |
| waiver in the wrong branch or commit | it is a warning; delete it whenever |
| missing waiver for a unit already on main | a `for` entry as a record; the reviewer notes it; no gate is involved |

Where the check runs:

1. The author, before the push: `copies --range $(git merge-base origin/main HEAD)..HEAD --worktree` (the `./dev`
   pre-push check of ADR 0049 runs it when the branch touches any registered path).
2. The plan job in CI on every branch run, next to commit-lint (as ADR 0034 §3b).
3. The steward, before merging a topic into a train (topic form, same result as 1 and 2) and before landing a train on
   main (train form over `origin/main..<train>`).

The merged train is not re-checked on main. A direct commit on main (rare) is a run of one unit and is checked by the
plan job of its push like any branch.

### 5. Existing trailers: honoured, then retired

- **Cutover** is the commit that lands WP-3 on main, written in the register header (`trailers_until = "<sha>"`).
- **Honoured.** In a unit whose commits are not descendants of the cutover (branches opened before it), a `Copies-waived:`,
  `Copies-removed:` or `Copies-date:` trailer on **any** commit of the unit (merges included) counts as a record of the
  unit, with the same validation as data, except that an old trailer which fails validation (a member it changed, a
  wrong spelling, a short reason) is a **warning and waives nothing**, since it cannot be edited. A missing or invalid old
  waiver is therefore fixed by adding a data entry; no history is touched. This retroactively makes incidents #2 to #6
  valid without decisions outside the rules, and turns #1 into a warning plus the two errors it should have been.
- **Retired.** commit-lint (C23) rejects a `Copies-*:` trailer in a commit that descends from the cutover, with "write it
  in `docs/known-copies.d/<branch>.toml` (ADR 0075)". A helper `copies trailers-to-data --range <r>` prints the entries
  for an open branch that wants to convert.
- **History on main** is not migrated. Main's landed units are never re-checked, so its 99 trailer lines (87 waivers,
  11 removals, 1 date move) stay where they are as a record; the drift logs start empty at the cutover, except where the WP-3 commit seeds them for the copies whose waivers
  were given in the last 14 days (optional, D4).

### 6. Positive controls (ER-33)

A C58 run counts only if it has shown it can see a violation:

- **Fixtures.** Each rule has a red and a green case in a temporary git repository built by the test with invented
  content (topic and train shapes, a sync merge, a landing merge with a conflict resolution that edits a member, a test-only
  and a comment-only commit, a `for` entry, an old trailer before and after the cutover).
- **Golden replays on real history.** Shared history is never rewritten, so past ranges are stable fixtures. The plan job
  runs, before the real range (well under a second):
  - `4772dc8ce..2e9ffb7a3` (topic form, as if the branch ended there) must report incident #1: a warning for the old
    trailer (a changed member, spelt as a file) and errors for the two unwaived members of `browser-driver`;
  - `4772dc8ce..b0481c236` must report no `browser-driver` error (the unit carries the waivers of `c4f1d20fd`);
  - the topic `fix/dyno-agent-handshake-close`, from its merge-base, must report no `companion-link` error at
    `63b1a9016` and must report it at `25b559213`.
  The expected results are pinned per mechanism, so unrelated findings in the same ranges do not make a replay flaky.
  If a replay does not give its expected result, the job reports `check broken` and fails, whatever the real range says.
- **No silent zero.** A range with registered paths in its diff but zero units computed is `check broken`; an empty
  range is reported as `not measured: no commits`.

## Alternatives considered

1. **Keep trailers, bind them to the unit, and allow empty commits that only carry trailers** (the minimal change: a
   carrier commit like `9b32fd0cb` becomes valid). It fixes #2 to #7 and costs about 0.5 agent-days less. It does not fix
   #1 well: a wrong trailer stays in history and must be downgraded to a warning, the author still cannot validate a
   trailer before committing, and the reviewer reads waivers in `git log`, not in the diff. **Fallback** if D1 is declined.
2. **`git notes` for waivers.** Notes can be added and edited after the fact. But `refs/notes/*` is not fetched by
   default or by the CI checkout, a concurrent edit of the notes ref needs a notes merge or a forced update (which the push
   rules forbid), and notes are not part of a branch's diff (`git-notes(1)`, https://git-scm.com/docs/git-notes; read,
   not tried here; whether Gitea shows notes on a pull request is unverified). Rejected.
3. **Allow amend and force-push for trailer fixes on unpushed or private branches.** The trailer is usually found wrong in
   review, after the push; and the force-push rule protects every agent's worktree. Rejected.
4. **Waivers in `paddock.waivers.toml`** (the conformance run's data waivers with `expires`). That file waives a check for
   a path glob for everyone until a date, so a copy waiver would leak to every branch; and one shared file conflicts.
   Its W001-W003 semantics are reused.
5. **Waivers inside the register, under each copy.** Same binding, but every branch edits a 945-line shared file, so
   trains conflict. Kept as the D2 alternative.
6. **Bind data records to commits (a `commit = "<sha>"` key).** A commit cannot name its own sha, and with no amend the
   entry always lands one commit late; that is incident #2 again. Rejected.

## Consequences

- Rule 55 (a), (c) and (d) change their wording from trailers to records (proposed text below); C23 learns one rule.
- Reviewers read waivers in the diff, next to the code they excuse.
- A branch's waivers cover the whole branch. Two fixes on one branch share them; split the branch if that matters.
- Each copy gains a drift log; `docs/known-copies.d/` grows with the number of open copies and shrinks as they are
  deleted (bounded by the register).
- The 44 precautionary trailers stop: `--suggest` writes only the waivers a unit needs.

Proposed rule 55 text for the user (not applied while Proposed): (a) "... or waives a member with a `[[waiver]]` entry in
the branch's file under `docs/known-copies.d/` (ADR 0075) ..."; (c) "... or with a `[[removal]]` entry there"; (d) "... a
delete-by date moves only with a `[[date_move]]` entry naming its approver ..."; (e) unchanged, plus "every finding is
fixed by a further commit; history is never rewritten for it".

## Parallelism and cost

- **Run time, measured** (Mac mini, warm cache, `git diff -U0` restricted to the 180 register paths of 49 mechanisms and
  86 copies at `b51c72509`): the 22 first-parent commits of `train/2026-11-08-1` took 0.31 s in total; the 90 first-parent
  commits of `origin/main` since 2026-11-02 took 1.1 s, of which 11 touched a registered path. Masking and the item walker
  add parsing only for touched files. One process, no parallelism needed; the cheaper-operation answer is already in the
  design: one pathspec-limited `git diff` per unit, the register parsed once per revision, and units skipped when their
  diff touches no registered path.
- **Agent work.** WP-1 and WP-3 can run in parallel; WP-2 needs WP-1's interface (one module signature) and can start
  from a stub; WP-4 needs WP-2. No builds; node only, so the machine load is negligible (ER-24).

## Work packages

This replaces ADR 0034 WP-1 (2.5 d, not started); the net addition is about 0.5 agent-days.

| WP | What | Size | Depends on |
|---|---|---|---|
| WP-1 | `conformance/src/util/units.mjs`: the units walk (topic, train, sync merge, landing merge with `--cc` lines, recursion for a train landing), record loading from `docs/known-copies.d/` and binding by normalised diff and `for`; unit tests on generated repositories | 0.75 d | none |
| WP-2 | C58 per ADR 0034 §2-§3 with this ADR's mode b: counted lines and masking, touch alike per unit, the error and warning set of §3, `--worktree` and `--suggest`, entry guard from records, mode a over the folder, mode c notice; fixtures for every rule | 1.5 d | WP-1 interface |
| WP-3 | Trailer cutover: honour old trailers per unit (§5), the C23 rule, `copies trailers-to-data`, `trailers_until` in the register header, the register and folder README text, rule 55 wording (after acceptance), lessons entry | 0.25 d | none |
| WP-4 | Positive controls and wiring: the golden replays and `check broken` in the plan job, the `./dev` pre-push hook call, the steward's topic and train forms, ADR 0034 status line, `docs/adr/README.md` | 0.5 d | WP-2 |
| | **Total** | **3.0 agent-days** | |
| WP-5 | Follow-up (D8): `Batch-size-waived:`, `Switch-date:`, `Freeze-closed:`, `Release-part:` on the same module | separate ADR | after a week of C58 |

Rollback: C58 is a check, not a runtime change. If it misbehaves, the plan job's C58 step is switched to report-only
(one line in `ci.yml`) and C23's trailer rule is reverted; the records folder keeps working as documentation, and
reviewers check by hand as today.

## Decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| D1 | Retire `Copies-*` trailers in favour of data records bound to the unit | **Yes.** Fallback: alternative 1 (unit-bound trailers plus carrier commits) |
| D2 | Where the records live | **One file per branch in `docs/known-copies.d/`** (no conflicts, one diff per branch); not inside the register |
| D3 | The binding unit | **The branch (topic) and each landing merge or run of direct commits (train)**, accepting that one branch's waivers cover all of that branch |
| D4 | Lifetime of waivers | **Kept as the copy's drift log until the copy leaves**; removal and date records pruned by the owner's release batch. Optionally seed the logs from the last 14 days of trailers (recommend: no, start empty) |
| D5 | A waiver that waives nothing | **Warning, not error**, so a misplaced record never needs a rewrite |
| D6 | Cutover | **Old trailers honoured for branches opened before the cutover; new ones rejected by C23** |
| D7 | Implementation language | **JavaScript in `conformance/`** next to C23, C58's spec and `util/waivers.mjs`; ported with the rest when the Rust move (ADR 0022 direction) reaches conformance |
| D8 | The other trailer records (ADRs 0042 and 0049) | **Same mechanism in a follow-up ADR**, after C58 has run a week |
| D9 | `for` entries for units already on main | **Allowed as a record only** (notice, no gate) |
