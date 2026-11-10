# 88. Ratchets fail on stale entries: one ratchet engine in `./dev check`, a `--write` that only removes, and a control per list

- Status: Proposed (2026-11-10), revision 2. Drafted at the user's request of 2026-11-10 (ER-21 redesigns). Revision 2
  answers the independent review of revision 1 (`fe314225c`, "changes": P1-1 to P1-4 and the P2s; see "Review answers").
  Draft only: nothing here is built. It needs the user's decisions below and a re-review.
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-22 / rules 42 and 55 (share first; one implementation per mechanism), ER-27, ER-31,
  ER-33 / rule 71 (a check counts only if it can see the thing), ER-36; ADR 0022 (Rust tools in `./dev`; no new shell or
  Node tooling), ADR 0034 (copies register, C58), ADR 0041 (C71 and its shrink-only baseline), ADR 0060 (C100 and
  `docs/gates.toml`), ADR 0072 (C102, `docs/ipc-endpoints.toml` and `docs/ipc-endpoint-ratchet.toml`), ADR 0075 (copy
  waivers as data, accepted 2026-11-09), ADR 0076 amendment (C79's `ADOPTING`), ADR 0082 (C71 extended over counts,
  accepted 2026-11-10, on `origin/docs/adr-0081-0082`), ADR 0087 (its enrolment list is one of these lists); the Rust
  tooling plan `docs/plans/rust-tooling-2026-11-10.md` (RT-5 `./dev check` host with "expiring waivers, shrink-only
  baselines by text" and its shadow run beside the JS checks, RT-9 C71 port, RT-17 the conformance remainder; on
  `origin/docs/rust-tooling-plan`); lesson "A rule that counts sites needs a baseline by text, not by count"
  (2026-11-04); memory `coverage-per-crate` (2026-11-04, a per-crate coverage register, not built yet).
- Number: 0088 was free on every `origin` branch on 2026-11-10 (same listing as ADR 0087: family ADRs end at 0086).
- Evidence: read and run at `origin/train/2026-11-09-3` `67a633ef3` on 2026-11-10, with `origin/main` fetched the same
  day: `node conformance/bin/paddock-conformance.mjs --app paddock --json` (the whole root run), `./dev gates check`
  (C100) and `./dev check ipc-endpoints` (C102) with the cached `paddock-dev` of that tree, `git merge-base
  --is-ancestor` for every C23 commit waiver, and small read-only scripts in the researcher's scratch folder for the lists
  no check reads. No user data.

## Context: the incidents

- **C71 timing baseline.** The root run warns: `conformance/baselines/C71.json lists a site that is gone (hash
  064af9f8fe17ec13)` (Debrief, `apps/debrief/app/src/app/core/mock/mock-clock.spec.ts`). It is a warning, so it stays.
  The same day `6bf1b44c4` (Pitwall Playground fix) removed two dead entries by hand; nothing required it.
- **Waivers that waive nothing.** The same run reports 20 `W003` warnings for `paddock.waivers.toml`. Read against every
  reader of the file, **9 are stale** and 11 are not:
  - the C53 path waiver at line 68 (`apps/debrief/crates/chromium-driver/src/launch.rs`: the file exists, no C53 site
    is left in it) and the C54 path waiver at line 222 (`apps/dyno/crates/dyno-helper/src/install.rs`: the file no
    longer exists);
  - 7 C23 commit waivers whose commits are ancestors of `origin/main` and 745 to 1,952 commits behind it, outside C23's
    200-commit window, so neither the conformance run nor `commit-lint --range` on any later branch can see them;
  - **not stale:** the other 11 C23 commit waivers name commits that are on the train but **not yet on `origin/main`**;
    `commit-lint` honours waivers, and the train's landing will lint those commits, so they are still needed although
    the root run on the train reports them. (Two more C23 commit waivers were used by the root run and are live.)
- **Silent ones.** `docs/gates.toml`'s C13 ratchet for Debrief lists `crates/debrief-ai-worker` in `not_yet`, a folder
  that no longer exists; C13 skips the member and says nothing. C79's `ADOPTING` list (in the check's source) names 3
  mains under `debrief-ai-worker` and `debrief-ai` that no longer exist; C79 iterates only the mains it finds.

**Why it matters.** A stale entry is a dormant waiver. The C53 path waiver for `launch.rs` will waive the next bare-pid
kill written in that file until 2026-12-31; the gone `debrief-ai-worker` member would exempt a re-created crate of that
name from the lint preset; a C71 hash entry passes a new line that normalises to the same text in that file. And a waiver
that waives nothing may mean the check went blind, which only a control can tell (ER-33). The C23 case also shows the
trap on the other side: a list can look stale to one reader and be needed by another.

## Pattern

**Shrink-only lists keep entries that match nothing, because staleness is a warning or invisible, and nobody can tell a
dead entry from one that another reader still needs.** Each check decides on its own what a dead entry is and how loud to
be; most choose a warning or nothing, and none asks the list's other readers.

**Invariant.** Every entry of a ratchet or register matches at least one live site or finding for at least one of the
list's declared readers, in runs whose recorded scope covers the entry; an entry that every covering reader reports as
unmatched is stale, and stale is an error where the tree is the reference (the root run on main and a branch's own
change) and a notice where staleness came from time or other commits (app and release runs); an entry no run covered is
`not measured`, never stale and never fine. The only automatic edit is removal, and it is reversible.

## Instances: every ratchet, baseline and register (2026-11-10, `67a633ef3`)

Found with `git grep -liE "baseline|ratchet|waiv|register"` over `conformance/` and `tools/paddock-dev/src`, the
`docs/*.toml` registers, `paddock.conformance.json`'s lists, each app's `deny.toml` and `clippy.toml`, and the e2e
baselines. "Stale case" is what the reader does with an unmatched entry today.

| # | List (file) | Readers | Stale case today | Entries | Stale today |
|---|---|---|---|---|---|
| 1 | `paddock.waivers.toml` (path and C23 commit waivers) | conformance runner (`util/waivers.mjs`, `W003`), `commit-lint` (C23 commit waivers) | **warning** (`W003`), per run; `commit-lint` does not report unused ones | 74 | **9** (2 path, 7 C23 commit) |
| 2 | `conformance/baselines/C71.json` | C71 | **warning**; growth against the merge base is an error | 561 sites | **1** |
| 3 | `docs/gates.toml` `[[ratchet]]` `not_yet`, `clippy_not_yet` (C13 adoption) | C13 (JS), C100 (growth only) | **silent** | 46 + 2 | **1** (`crates/debrief-ai-worker`) |
| 4 | `ADOPTING` in `conformance/src/checks/C79-orphan-guard-hooks.mjs` | C79 | **mixed**: error when a listed main already calls the guardian, **silent** when the main is gone | 47 | **3** (gone mains) |
| 5 | `docs/gates.toml` `[[not_gated]]`, `[[planned]]`, `[[ignored]]`, `[[defect]]`, `[[level]]`, `[[report_only]]`, ... | C100 (`gates/evaluate.rs`) | **error** | 106 tables (the ratchet included) | 0 (`C100: 388 checks, 0 findings`) |
| 6 | `docs/ipc-endpoints.toml` `[[out_of_scope]]` | C102 (`ipc_endpoints.rs`) | **error** | 14 | 0 |
| 7 | `docs/ipc-endpoint-ratchet.toml` (with per-entry `os`) | the endings harness; C102 checks its shape | **error** in the ending tests | 31 | not measured (needs the ending tests on each OS) |
| 8 | `apps/pitwall/app/e2e/a11y.baseline.json`, `layout.baseline.json` | Pitwall's a11y and layout e2e specs (`expect(found).toEqual(baseline)`) | **error** in the e2e run (exact equality) | 0 and 0 (`{}`) | 0 |
| 9 | `paddock.conformance.json` `ownTestBinaries` | C37 | **warning** | 22 | 0 |
| 10 | `paddock.conformance.json` `untimedCommands` | C35 | **warning** | 0 | 0 |
| 11 | `paddock.conformance.json` `limitSkip`, `limitWaivers`; `parserSkip` | C62; C36 | **silent** | 1 + 0; 0 | 0 |
| 12 | Inline `conformance: background-qos` waivers | C54 | **warning** | n/a | 0 |
| 13 | `docs/screenshots/REVIEWED.md`, `hashes.json` | C24 | **error** | per app | 0 |
| 14 | `apps/debrief/deny.toml` `[bans] skip`, `skip-tree` | `cargo deny check bans` | cargo-deny's `unmatched-skip` / `unmatched-skip-root` diagnostics; their default severity is not stated on the [bans diagnostics page](https://embarkstudios.github.io/cargo-deny/checks/bans/diags.html) (read 2026-11-10, **unverified**) | 42 + 3 | not measured |
| 15 | `[[disallowed-methods]]` / `-types` entries with `allow-invalid = true` in the 10 `clippy.toml` files (about 95 per file) | Clippy, per workspace | **silent by design**: a shared preset whose paths need not resolve in every workspace | about 860 | not measured (stale only if a path resolves in **no** workspace) |
| 16 | `docs/known-copies.toml` (members) | C58, **not built** on any branch (ADR 0075 §Pattern) | **unchecked** (rule 55 (c) says an entry leaves with its files) | 53 mechanisms, 139 path lists | 0 (every path list matches a tracked file, except 3 `planned` canonicals, which may not) |
| 17 | `docs/known-copies.d/*.toml` (ADR 0075 records, not created yet) | C58 (planned) | **notice** by decision (ADR 0075 D4, D5): a drift log | 0 | 0 |
| 18 | Coverage register (memory `coverage-per-crate`), `docs/liveness.toml` (ADR 0081), `docs/test-paths.toml` (ADR 0086), `docs/wintest-crates.toml` (ADR 0087) | planned | not built | – | – |

**Today: 14 stale entries in four lists** (9 + 1 + 1 + 3), none of which fails a run. By stale case, counting each
list once: 5 are errors (5, 6, 7, 8, 13), 5 warn (1, 2, 9, 10, 12), 2 are silent (3, 11), 1 is mixed (4: error when a
listed main already calls the guardian, silent when it is gone), 1 is silent by design (15), 1 is unchecked (16), 1
depends on cargo-deny's level (14), and ADR 0075 chose a notice for its drift log (17). The lists implement "is this entry still used?" about ten times with three severities
(rule 55); one writer, `paddock-timing-baseline.mjs --write`, already removes stale C71 entries.

## Decision

### 1. Three kinds of list

| Kind | Meaning | Stale entry | Growth |
|---|---|---|---|
| **Ratchet** (baselines, adoption lists: 2, 3, 4, 7, 8, coverage, `wintest-crates`) | known debt that may only shrink | stale (§3) | error against the merge base, except a seed (§5) |
| **Register** (waivers and exemptions with reasons: 1, 5, 6, 9 to 16) | a deliberate exception with a reason | stale (§3) | allowed with its reason (and date where the register has one) |
| **Drift log** (ADR 0075 records: 17) | history kept on purpose | **notice** always, as ADR 0075 D4/D5 decided; pruned by `copies --prune` | allowed |

The drift log is the one exception, because its entries describe landed units and are meant to outlive the code they
describe; this ADR does not reopen ADR 0075.

### 2. Readers and the run record

Each list declares its **readers** (`readers = ["conformance:C23", "commit-lint"]`): every program that consults the
list. Each reader writes a **run record** per run: which check ran, whether it **completed**, its **precondition**
(history depth, `--paddock` present, base ref fetched, the OS), its **scope** (root or `apps/<app>`, `--files`, the
commit window), a **probe count** (sites or commits it looked at), and the keys it matched. An entry is **covered** by a
run when the record says the reader completed with its precondition met, a probe count above zero, and the entry's place
inside the scope (for an entry with `os`, on that OS). An entry is:

- **live** when some covering reader matched it;
- **stale** when **every** declared reader has a covering run in this invocation and none matched it;
- **not measured** otherwise (a reader did not run, did not complete, or its scope did not reach the entry).

Specific readers: a C23 commit waiver is covered by `commit-lint` only when its commit is an **ancestor of
`origin/main`** (no later branch range can contain it) and by C23 only when the commit is inside its window; so a C23
waiver is stale only when its commit is on main and outside the window. A clippy `allow-invalid` entry (15) is stale only
when it resolves in no workspace of the repository. An `ipc-endpoint-ratchet` entry with `os = "windows"` is covered only
by a Windows run of the endings harness.

### 3. Where stale is an error (the time bomb)

Staleness has two causes: the change under test removed the site (the author can and should remove the entry in the
same change), or **time and other commits** did (a window moved, another branch fixed the site, a release branch is
older than main). Treating the second as an error in every run would turn an app's release run red because of a commit
it does not contain.

- **Error:** in the root run on `main` (and on trains, whose integrator lands into main), and in a branch run for
  entries whose stale state the branch itself caused (the entry's place is in the branch's diff against its merge
  base).
- **Notice:** in app-scoped runs and release runs (tags, `release/**`, `<app>-release.yml`) for every other stale entry;
  the notice names the entry and "removed on main by `./dev check ratchets --write`".
- **On main** a stale entry fails the root run; the fix is the `--write` commit (D3 offers a bot that opens it).

### 4. The engine and `./dev check ratchets`

`tools/paddock-dev/src/check/ratchet.rs`, a module of RT-5's `./dev check` host (ADR 0022; Rust only):

```rust
/// One list of exceptions that checks read.
pub trait Ratchet {
    fn id(&self) -> &str;                                            // "C71-baseline"
    fn kind(&self) -> Kind;                                          // Ratchet | Register | DriftLog
    fn readers(&self) -> &[ReaderId];
    fn entries(&self, text: &str) -> Result<Vec<Entry>, ParseError>; // key, byte span, optional os and rule
    fn covers(&self, entry: &Entry, record: &RunRecord) -> bool;     // §2, from the reader's run record
}
```

The engine takes the entries and the readers' run records and returns live, stale (error or notice per §3) and not
measured; it owns the growth rule (the multiset difference against the list at the merge base, today in C71's
`baselineGrowth` and C100's `check_ratchets`) and the report lines (`::error file=<list>,line=<n>::` with the entry and
"remove it: ./dev check ratchets --write --only <id>"). Its first code comes from C100's Rust (`gates/evaluate.rs` stale
entries, `gates/register.rs` `check_ratchets`).

`./dev check ratchets [--only <id>] [--write]`:

- Without `--write`: runs every declared list's readers (or reads their run records, during RT-5's shadow period) and
  reports per list: entries, live, stale, not measured.
- `--write` **only ever removes**, and only when, **in the same invocation**, the list's positive control (§6) passed
  and every reader of the list covered every entry. It deletes the stale entries' byte spans (with a comment block that
  belongs to the entry only), keeps JSON valid (the separating comma of a removed array element goes with it; the file
  is re-parsed), keeps the rest byte for byte, asserts the new entry set is a subset of the old one and every remaining
  entry is still live, and otherwise writes nothing and fails. It never touches a drift log (use `copies --prune`).
- **Restore.** `--write` commits carry a data record in `docs/ratchets.toml` (`[[removed]] list, keys, commit`); a
  revert of that commit, or re-adding exactly those keys while the record is present, is not growth. A key re-added
  later without the record is growth and is judged as such. The record is pruned with the next `--write` once its
  commit is on main for 30 days.
- Lists in code (C79's `ADOPTING`) move to data first (`docs/ratchets/C79-adopting.toml`), so `--write` never edits
  source. The new file is added to `ci-inputs` (the planner's inputs of C79, so a change to it runs C79) and to
  `CODEOWNERS` beside the check.

### 5. The list of lists: `docs/ratchets.toml`, and seeds

Every list is declared once: `id`, `file`, `kind`, `readers`, `key`, `control` (its fixture folder). A meta-check in the
same host fails when a file matching the list patterns (`*.waivers.toml`, `conformance/baselines/**`,
`docs/*-ratchet.toml`, `docs/known-copies*.toml`, `docs/ratchets/**`, `**/e2e/*.baseline.json`, `deny.toml`
`[bans] skip`/`skip-tree`, the keys of `paddock.conformance.json` that checks read as lists) is not declared, or a
declared list has no control or its control did not run in the module's tests.

**Seeds** (growth of a ratchet on purpose, such as ADR 0082 WP-0 adding its new rows to C71). A `[[seed]]` record names
the ratchet, the ADR, the rule ids added, `count` and `entries_sha256` (the hash of the sorted added keys). The growth
rule accepts exactly that set when:
- the named ADR's status line **on `origin/main`** reads `Accepted` (the check reads the file there, not on the branch);
- every added entry carries a `rule` field naming one of the seed's rule ids (C71's entries gain `rule`; today they have
  `file`, `line`, `hash`);
- `count` and `entries_sha256` match the entries the branch added;
- the seed is on the branch that adds the entries: a seed is valid only in the range of that branch; **the integrator
  removes the seed record at landing** (in the landing merge), so it can never cover a later growth.

### 6. Positive controls, one per list (ER-33)

Each list's control fixture holds a tiny tree with **one live entry, one planted stale entry, one entry a reader cannot
cover** (out of scope, other OS, a commit not on main) and, for registers, **a planted site under a planted waiver**. The
control asserts: the stale entry is stale (error or notice by §3; notice for the drift log), the live one passes, the
uncovered one is `not measured`; `--write` removes exactly the planted stale entry and the file otherwise stays
byte-equal (and valid JSON); once a waiver is removed its planted site is reported (the reader is not blind); restoring
the removed entry with its `[[removed]]` record is not growth. `--write` runs a list's control first and refuses without
it (§4).

| List | Planted stale entry | Planted uncovered entry |
|---|---|---|
| waivers | a C53 path waiver for a file without a kill; a C23 waiver for an ancestor of main outside the window | a C23 waiver for a commit not on main |
| C71 baseline | a site hash whose line was rewritten | a site in an app the scoped run did not read |
| C13 `not_yet` | a member folder that does not exist; a member that already opts in | – |
| C79 adopting | a main that no longer exists; a main that calls the guardian | – |
| C100 tables, C102 `out_of_scope`, C24 rows, Pitwall e2e baselines | as their existing tests (kept; re-pointed at the engine) | – |
| endings ratchet | a fault entry for a scenario that now ends correctly | an `os = "windows"` entry in a macOS run |
| C37, C35, C62, C36 lists | a path or name that does not exist | – |
| cargo-deny skips | a `skip` for a crate version not in the lockfile | – |
| clippy `allow-invalid` | a path that resolves in no workspace | a path into a crate only one other workspace has |
| known-copies | a copy whose paths match no tracked file (rule 55 (c)) | – |
| drift log | a record whose member left the register (notice) | – |

### 7. Migration

1. **Now, before any code (WP-0):** remove the 14 stale entries in four commits, one per list, each after checking that
   its reader still sees a planted site in that place (the control by hand): the 9 waivers (C53 for `launch.rs`, C54 for
   the gone `install.rs`, 7 C23 commit waivers on main outside the window; the 11 C23 waivers for commits not yet on main
   stay), the C71 entry (`paddock-timing-baseline.mjs --write`), `crates/debrief-ai-worker` from Debrief's C13 `not_yet`
   (a shrink, which C100 allows), and the 3 gone mains from `ADOPTING`.
2. **Interim severities in the JS (WP-1), with a failing test each, coordinated with RT-5's shadow oracle.** The JS
   checks are the oracle RT-5's Rust host is compared with in shadow (plan §3); a severity changed on one side only is a
   shadow difference. So each flip lands in the JS check **and** is listed as an intended difference in RT-5's shadow
   table in the same change, and RT-5's ports implement the new severity. The flips: `W003` becomes an error in the
   root run on main and a notice elsewhere (§3), with C23 commit waivers judged by §2 (commit on main and outside the
   window); C71's, C37's and C35's stale warnings the same; C13 reports a missing or opted-in `not_yet` member; C79
   reports a listed main it cannot find. These change existing checks' severities; they are not new Node tooling.
3. **The engine, the run records and the list of lists** extend RT-5 (§9's net estimates); C100 and C102 adopt them
   first (already Rust).
4. **Ports onto the engine:** C71 with RT-9 (its entries gain `rule`), the waivers, C13, C79, C37, C35, C62, C36 with
   RT-17's batches; C58 (ADR 0075 WP-2) is built on it. Every JS copy of the stale logic, **and `paddock-timing-baseline.mjs
   --write`**, is registered in `docs/known-copies.toml` as a copy of a new mechanism `ratchet-entries` (canonical
   `tools/paddock-dev/src/check/ratchet.rs`, status `planned` until it is built) and leaves with its check's retirement
   (rule 55).

### 8. Relation to other ADRs

- **ADR 0082 (C71 over counts).** RT-9 ports C71 onto this engine; ADR 0082 WP-0's regeneration with the new rows is a
  seed (§5) naming ADR 0082 (accepted), with C71 entries gaining `rule`. C71's shrink-only rule (ADR 0041 §4) is
  unchanged; only its stale severity changes.
- **ADR 0075 (copy waivers as data).** Unchanged: its records are the drift log, stale is a notice (D4), a waiver that
  waives nothing a warning (D5), `copies --prune` its removal tool. What this ADR adds is for the register
  `docs/known-copies.toml`: a copy whose paths match nothing is stale (rule 55 (c)), checked by C58 through the engine.
- **ADR 0034.** The stale check is itself a mechanism with about ten implementations and one writer; this ADR names the
  canonical one and registers the others as copies to delete (§7.4).
- **ADR 0060 (C100).** Its stale-entry logic seeds the engine; its `[[ratchet]]` gains the stale half it lacks.
- **ADR 0072 (C102).** Already errors on a stale `out_of_scope`; adopts the engine, and the endings ratchet's `os` field
  becomes a coverage rule.
- **ADR 0087.** Its `docs/wintest-crates.toml` is a ratchet of this engine.

### 9. Release impact (ER-31)

- No shipped code changes. Conformance and `tools/paddock-dev` are outside every app's dependency set.
- **App and release runs never turn red because of time or other commits** (§3): those stale entries are notices there.
  A stale entry that a release branch itself caused is an error like any finding of that branch.
- The root run on main turns red on a stale entry; under ER-31 (b) it blocks an app's tag only when the list is one the
  app's gates read (its scoped lists: its waivers, its C71 sites, its ratchet members) and the entry is in the app's
  scope; a stale entry of another app's list does not block it (ER-31 (a)).
- WP-0 removes today's 14 entries before any severity changes, so no release in flight meets a new red check: Debrief
  has 1 C71 entry, 1 C53 waiver, 1 C13 member and 3 C79 mains; Dyno 1 C54 waiver; the 7 C23 waivers are root-scoped.

## Parallelism and cost (rule 51)

The engine is bookkeeping over findings the readers already compute: one set lookup per entry and one parse per list
(the largest, C71's baseline, is 2,810 lines). The run records add a few fields per check. The one extra reader run is
`commit-lint`'s ancestry test per C23 commit waiver (`git merge-base --is-ancestor`, 20 calls today). No timing is quoted
as evidence (rule 48); WP-3 records the engine's overhead on the root run in `docs/benchmarks.md`. Nothing to
parallelise beyond what the readers already do.

## Alternatives

- **Keep warnings and remove by hand in each batch.** Rejected: that is today, and it produced the 14 dormant
  exceptions and hand removals like `6bf1b44c4`.
- **Stale is an error in every run.** Rejected (review P1-2): a moved window or another branch's fix would turn app and
  release runs red for commits they do not contain.
- **Stale per reader** (an entry is stale when the run's own reader does not use it). Rejected (review P1-1): it would
  delete the 11 C23 waivers `commit-lint` still needs.
- **Expiry dates on every entry instead.** Rejected for ratchets: a date says nothing about whether the entry still
  matches. Registers keep their dates as well (`W001`).
- **Fix each check's severity and stop there (WP-1 only).** Rejected as the end state: the ten copies would drift again.
- **Make the drift log stale-is-error too.** Rejected: it would reopen ADR 0075 D4/D5.

## Consequences

- A change that fixes the last site of an entry must remove the entry in the same change; `--write` makes that one
  command, gated on the list's control.
- Stale entries caused by time or other branches are removed on main, by the root run's failure or a bot's commit, not
  by whoever happens to run an app check.
- A reader that goes blind shows up: its controls fail, and its run record shows a probe count of zero.
- Lists in code move to data files with planner inputs and owners.

## Rollback

Set a list's kind to report-only in `docs/ratchets.toml` (one line per list, visible in review) or revert WP-1's
severity changes; entries removed by `--write` can be restored through their `[[removed]]` record (§4).

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Severity of a stale entry | (a) error in the root run on main and for staleness a branch caused; notice in app and release runs for staleness from time or other commits; drift log always a notice; (b) error everywhere; (c) warning everywhere, removed in batches | **(a)** |
| D2 | When an entry is stale | (a) only when every declared reader covered it (run records: completed, precondition, probe count, scope, OS) and none matched; (b) per reader | **(a)** |
| D3 | Stale entries on main | (a) the root run on main fails; the next landing's integrator runs `--write`; (b) a bot opens the `--write` commit on main after each landing; (c) (a) now, (b) once the engine exists | **(c)** |
| D4 | Where the engine lives | (a) an extension of RT-5's `./dev check` host (Rust), the JS checks as its shadow oracle until ported; (b) the JS conformance host | **(a)** |
| D5 | Interim severity flips in the JS checks (WP-1) | (a) now, each with a failing test and an entry in RT-5's shadow difference table; (b) wait for the Rust ports | **(a)** |
| D6 | Remove today's 14 stale entries | (a) now (WP-0), one commit per list, each after a planted-site check; the 11 C23 waivers for commits not on main stay; (b) with WP-1 | **(a)** |
| D7 | What `--write` may do | (a) remove only, after the list's control passed in the same invocation and every entry was covered; JSON-safe; a `[[removed]]` record so a revert is not growth; (b) also re-key moved entries | **(a)** |
| D8 | Growth on purpose (ADR 0082's new rows) | (a) a `[[seed]]` with `count`, `entries_sha256` and rule ids, valid only for an ADR accepted on main and only on its branch, removed by the integrator at landing; (b) a commit trailer; (c) never | **(a)** |
| D9 | Lists in code (C79's `ADOPTING`) | (a) move to `docs/ratchets/<id>.toml`, added to the planner inputs and CODEOWNERS; (b) the engine edits source | **(a)** |
| D10 | Tag gating | (a) none beyond the normal gates, judged per app under ER-31 (§9); (b) a tag also needs zero stale entries family-wide | **(a)** |
| D11 | Engineering rule | (a) extend ER-33: "... and a list of known exceptions counts only if each entry is shown to match something: an entry no reader matches is removed, and an entry no reader could see is not measured"; (b) a new ER-37 | **(a)**: it is ER-33's rule (a check must show it can see the thing) applied to the lists checks read |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Remove the 14 stale entries (§7.1), one commit per list, each with its planted-site check recorded in the commit | 0.5 |
| WP-1 | JS severity flips with failing tests first (§7.2), the §2 rule for C23 commit waivers, the §3 split by run kind, and the shadow difference entries for RT-5 | 1.5 |
| WP-2 | **Extension of RT-5:** `check/ratchet.rs` (trait, coverage from run records, judge, growth, §3 severities, report) extracted from C100's code; the run record written by the RT-5 host for every check it runs; property test that `--write` never adds. Net of RT-5's own "shrink-only baselines by text" (RT-5 already budgets that part) | 2 net |
| WP-3 | `docs/ratchets.toml`, the meta-check, `[[seed]]` and `[[removed]]`, `./dev check ratchets [--write]` with JSON-safe removal and the control gate; control fixtures for every list in §6 (about 15 lists, two to four cases each) | 3 |
| WP-4 | C100 (with the `not_yet` stale half) and C102 (with `os` coverage) on the engine; C79's `ADOPTING` to data with planner inputs and CODEOWNERS | 1 |
| WP-5 | Register `ratchet-entries` and its copies (including `paddock-timing-baseline.mjs --write`) in `docs/known-copies.toml`; the ER-33 extension and the Paddock rule line; lessons entry | 0.5 |
| | **Total (new work, net of RT-5)** | **8.5** |
| | Inside existing packages: C71 on the engine with RT-9 (+0.5, including the `rule` field), the JS lists with RT-17's batches (+1), C58 on it with ADR 0075 WP-2 (no extra) | (+1.5) |

First cut: WP-0 and WP-1 (2 days) close the four lists that are stale or silent today.

## Review answers (revision 1, `fe314225c`)

| Finding | Answer in revision 2 |
|---|---|
| P1-1 13 of the 20 C23 waivers are live | Recounted with `git merge-base --is-ancestor` against `origin/main`: 9 waivers are stale (2 path, 7 C23), 11 C23 waivers name commits not yet on main and stay; readers per list (§2), stale only when stale for every reader; a C23 waiver is stale only when its commit is on main and outside the window. Total stale now 14, not 25 |
| P1-2 time bomb | §3: error in the root run on main and for staleness a branch caused, notice in app and release runs; D3; ER-31 section §9 |
| P1-3 coverage, control before write, per-OS, restore | §2 run records (ran, completed, precondition, probe count, scope, OS), `--write` refuses unless the list's control passed in the same invocation (§4, §6), `os` entries covered only on that OS, `[[removed]]` restore records |
| P1-4 seeds | §5: `count`, `entries_sha256`, rule ids, C71 entries gain `rule`, the ADR's status read on main, valid only on its branch, removed by the integrator at landing |
| P2 RT-5 net estimates, shadow oracle | WP-2 is an extension of RT-5 with a net estimate; §7.2 coordinates the JS flips with RT-5's shadow difference table |
| P2 WP-3 size | 3 days |
| P2 C79 data file inputs and owners | §4, D9 |
| P2 JSON commas, the timing-baseline writer as a copy | §4, §7.4 |
| P2 ER-37 as an extension of ER-33 | D11 |
| P2 inventory | `install.rs` is gone (stated); C79 counted once as "mixed"; Pitwall's a11y and layout baselines, the cargo-deny skips and the clippy `allow-invalid` entries added (rows 8, 14, 15) |
