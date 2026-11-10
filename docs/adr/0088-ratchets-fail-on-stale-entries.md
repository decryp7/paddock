# 88. Ratchets fail on stale entries: one ratchet engine in `./dev check`, a `--write` that only removes, and a control per ratchet

- Status: Proposed (2026-11-10). Drafted at the user's request of 2026-11-10 (ER-21 redesigns). Draft only: nothing here is
  built. Needs the user's decisions below and an independent review before any work package starts.
- Date: 2026-11-10
- Related: ER-21 / rule 40, ER-22 / rules 42 and 55 (share first; one implementation per mechanism), ER-27, ER-33 /
  rule 71 (a check counts only if it can see the thing), ER-36; ADR 0022 (Rust tools in `./dev`; no new shell or Node
  tooling), ADR 0034 (copies register, C58), ADR 0041 (C71 and its shrink-only baseline), ADR 0060 (C100 and
  `docs/gates.toml`), ADR 0072 (C102, `docs/ipc-endpoints.toml` and `docs/ipc-endpoint-ratchet.toml`), ADR 0075 (copy
  waivers as data, accepted 2026-11-09), ADR 0076 amendment (C79's `ADOPTING`), ADR 0082 (C71 extended over counts,
  accepted 2026-11-10, on `origin/docs/adr-0081-0082`); the Rust tooling plan `docs/plans/rust-tooling-2026-11-10.md`
  (RT-5 `./dev check` host with "expiring waivers, shrink-only baselines by text", RT-9 C71 port, RT-17 the conformance
  remainder; on `origin/docs/rust-tooling-plan`); lesson "A rule that counts sites needs a baseline by text, not by count"
  (2026-11-04); memory `coverage-per-crate` (2026-11-04, a per-crate coverage register, not built yet).
- Number: 0088 was free on every `origin` branch on 2026-11-10 (same listing as ADR 0087: family ADRs end at 0086, and
  0087 is ADR 0087 on this branch).
- Evidence: read and run at `origin/train/2026-11-09-3` `67a633ef3` on 2026-11-10: `node
  conformance/bin/paddock-conformance.mjs --app paddock --json` (the whole root run, every check), `./dev gates check`
  (C100) and `./dev check ipc-endpoints` (C102) with the cached `paddock-dev` of that tree, and small read-only scripts in
  the researcher's scratch folder for the registers no check reads. No user data.

## Context: the incidents

- **C71 timing baseline.** The root run on 2026-11-10 warns: `conformance/baselines/C71.json lists a site that is gone
  (hash 064af9f8fe17ec13)` (Debrief, `apps/debrief/app/src/app/core/mock/mock-clock.spec.ts`). It is a warning, so it
  stays. The same day `6bf1b44c4` (Pitwall Playground fix) removed two dead entries by hand; nothing required it.
- **Waivers that waive nothing.** The same run reports **20** `W003` warnings for `paddock.waivers.toml`: two path
  waivers (line 68, C53 for `apps/debrief/crates/chromium-driver/src/launch.rs`; line 222, C54 for
  `apps/dyno/crates/dyno-helper/src/install.rs`; the brief saw them at lines 68 and 228 on an older tree) and 18 C23
  commit waivers whose commits are now about 2,000 commits behind the head, outside C23's 200-commit window (the file's
  own comment says "the lines can go" then). A `git grep` finds no C53 or C54 site left in either file.
- **Silent ones.** Some registers do not even warn. `docs/gates.toml`'s C13 ratchet for Debrief lists
  `crates/debrief-ai-worker` in `not_yet`, a folder that no longer exists; C13 skips the member and says nothing. C79's
  `ADOPTING` list (in the check's source) names 3 mains under `debrief-ai-worker` and `debrief-ai` that no longer exist;
  C79 only iterates the mains it finds, so they are invisible.

**Why it matters.** A stale entry is a dormant waiver, not just clutter. The C53 path waiver for `launch.rs` will waive the
next bare-pid kill written in that file until 2026-12-31; the gone `debrief-ai-worker` member would exempt a re-created
crate of that name from the lint preset; a C71 hash entry passes a new line that normalises to the same text in that
file. And a waiver that waives nothing may mean the check went blind, which only a control can tell (ER-33).

## Pattern

**Shrink-only lists keep entries that match nothing, because staleness is a warning or invisible.** Whenever a check
reads a list of known exceptions (a baseline, a ratchet, a waiver or exemption register), each check decides on its own
what a dead entry is and how loud to be, and most choose a warning or nothing. Dead entries pile up, nobody is made to
remove them, and each one re-arms its exception for whatever comes next at that place.

**Invariant.** Every entry of a ratchet, baseline or waiver register matches at least one live site or finding at the
head, in every run whose scope covers the entry; an entry that matches nothing is an **error** in that run; an entry the
run cannot see is reported `not measured`, never "stale" and never "fine". The only automatic edit is removal.

## Instances: every ratchet, baseline and register (2026-11-10, `67a633ef3`)

Found with `git grep -liE "baseline|ratchet|waiv|register"` over `conformance/` and `tools/paddock-dev/src`, the
`docs/*.toml` registers, and `paddock.conformance.json`'s lists.

| # | List (file) | Read by | A stale entry today is | Entries | Stale today | Removal tool |
|---|---|---|---|---|---|---|
| 1 | `paddock.waivers.toml` (path and C23 commit waivers) | runner, `util/waivers.mjs` (`W003`) | **warning** (`W003`); skipped for C99 waivers outside a scoped run | 74 | **20** (2 path, 18 C23 commit) | by hand |
| 2 | `conformance/baselines/C71.json` | C71 | **warning**; growth against the merge base is an error | 561 sites | **1** | `paddock-timing-baseline.mjs --write` (shrink only) |
| 3 | `docs/gates.toml` `[[ratchet]]` `not_yet`, `clippy_not_yet` (C13 adoption) | C13 (JS), C100 (growth only) | **silent** (C13 skips the member) | 46 + 2 | **1** (`crates/debrief-ai-worker`) | by hand |
| 4 | `ADOPTING` in `conformance/src/checks/C79-orphan-guard-hooks.mjs` | C79 | error when a listed main calls the guardian; **silent** when the main is gone | 47 | **3** (gone mains) | by hand (source edit) |
| 5 | `docs/gates.toml` `[[not_gated]]`, `[[planned]]`, `[[ignored]]`, `[[defect]]`, `[[level]]`, `[[report_only]]`, ... | C100 (`gates/evaluate.rs`) | **error** (`[[not_gated]] ... is stale`, `names a check that no longer exists`, `[[ignored]] lists ..., which is no longer ...`) | 106 tables (the ratchet included) | 0 (`C100: 388 checks, 0 findings`) | by hand |
| 6 | `docs/ipc-endpoints.toml` `[[out_of_scope]]` | C102 (`ipc_endpoints.rs`) | **error** (`matches no listen site any more. Remove the entry.`) | 14 | 0 | by hand |
| 7 | `docs/ipc-endpoint-ratchet.toml` | the endings harness (`PADDOCK_ENDINGS_NO_RATCHET`); C102 checks its shape | **error** in the ending tests ("a listed fault that no longer happens fails the run") | 31 | not measured (needs the ending tests) | by hand |
| 8 | `paddock.conformance.json` `ownTestBinaries` | C37 | **warning** (`which does not exist`) | 22 | 0 | by hand |
| 9 | `paddock.conformance.json` `untimedCommands` | C35 | **warning** (`Remove the stale entry`) | 0 | 0 | by hand |
| 10 | `paddock.conformance.json` `limitSkip`, `limitWaivers` | C62 | **silent** | 1 + 0 | 0 | by hand |
| 11 | `parserSkip` (config) | C36 | **silent** | 0 | 0 | by hand |
| 12 | Inline `conformance: background-qos` waivers | C54 | **warning** ("waives nothing") | n/a | 0 | by hand |
| 13 | `docs/screenshots/REVIEWED.md`, `hashes.json` | C24 | **error** (`reviews X, which does not exist`) | per app | 0 | by hand |
| 14 | `docs/known-copies.toml` (members) | C58, **not built** on any branch (ADR 0075 §Pattern) | **unchecked** (rule 55 (c) says an entry leaves with its files) | 53 mechanisms, 139 path lists | 0 (every path list matches a tracked file except 3 `planned` canonicals, which may) | by hand |
| 15 | `docs/known-copies.d/*.toml` (ADR 0075 records, not created yet) | C58 modes a-c (planned) | **notice** by decision (ADR 0075 D4, D5): a drift log | 0 | 0 | `copies --prune` (planned) |
| 16 | Coverage register (memory `coverage-per-crate`), `docs/liveness.toml` (ADR 0081), `docs/test-paths.toml` (ADR 0086) | planned | not built | – | – | – |

**Today: 25 stale entries in four lists** (20 + 1 + 1 + 3), none of which fails a run. Of the 16 lists, 5 already treat a
stale entry as an error (C24, C79's "hooked", C100, C102, the endings harness), 5 warn, 4 are silent, 1 is unchecked, and
ADR 0075 chose a notice for its drift log. Each implements "is this entry still used?" on its own: about ten copies of
one mechanism with three severities (rule 55).

## Decision

### 1. Three kinds of list, one rule each

| Kind | Meaning | Stale entry | Growth |
|---|---|---|---|
| **Ratchet** (baselines, adoption lists: 2, 3, 4, 7, coverage) | known debt that may only shrink | **error** | error against the merge base, except a declared seed (§4) |
| **Register** (waivers and exemptions with reasons: 1, 5, 6, 8-14) | a deliberate exception with a reason | **error** | allowed with its reason (and date where the register has one) |
| **Drift log** (ADR 0075 records: 15) | history kept on purpose | **notice**, as ADR 0075 D4/D5 decided; pruned by `copies --prune` | allowed |

The drift log is the one exception, because its entries describe landed units and are meant to outlive the code they
describe; this ADR does not reopen ADR 0075.

### 2. The engine: `tools/paddock-dev/src/check/ratchet.rs`

One module in the RT-5 `./dev check` host (ADR 0022; Rust only). Each list is an adapter of one small trait:

```rust
/// One list of exceptions that a check reads.
pub trait Ratchet {
    fn id(&self) -> &str;                        // "C71-baseline"
    fn kind(&self) -> Kind;                      // Ratchet | Register | DriftLog
    fn entries(&self, text: &str) -> Result<Vec<Entry>, ParseError>; // each with its key and byte span in the file
    fn covers(&self, entry: &Entry, run: &RunScope) -> bool;         // can this run see the entry's place?
}
```

The engine takes the entries, the keys the check matched in this run and the run's scope, and returns `live`, `stale`
(error for ratchets and registers, notice for drift logs) and `unmeasured` (the run did not cover the entry: a scoped
run, `push-gate --files`, a C23 commit that is not in this history). It also owns the growth rule (the multiset
difference against the list at the merge base, today in C71's `baselineGrowth` and C100's `check_ratchets`) and the
report lines (`::error file=<list>,line=<n>::` with the entry and "remove it: ./dev check ratchets --write --only <id>").

The first code comes from C100's Rust (`gates/evaluate.rs` stale entries, `gates/register.rs` `check_ratchets`), which
already has both halves; C71's text-keyed baseline (lesson 2026-11-04) supplies the key-by-normalised-text adapter.

### 3. `./dev check ratchets [--only <id>] [--write]`

- Without `--write`: runs every declared list's check (or reads the findings the conformance job produced, during the
  shadow period) and reports per list: entries, live, stale, unmeasured.
- `--write` **only ever removes**: it deletes the byte spans of stale entries (with their preceding comment block when
  that block belongs to the entry only), re-parses the result, and asserts that the new entry set is a subset of the old
  one and that every remaining entry is still live; otherwise it writes nothing and fails. It refuses when any entry of
  the list is `unmeasured` in this run ("run the whole scope"), and it never touches a drift log (use `copies --prune`).
  It prints each removed entry. Formatting of the rest of the file is kept byte for byte.
- Entries in code (C79's `ADOPTING`) move to data first (`docs/ratchets/C79-adopting.toml`), so `--write` never edits
  source (parse, don't validate).

### 4. The list of lists: `docs/ratchets.toml`

Every list is declared once: `id`, `file`, `check`, `kind`, `key` (what identifies an entry), and `control` (the fixture
folder of its positive control). A meta-check in the same host fails when:
- a file matching the list patterns (`*.waivers.toml`, `conformance/baselines/**`, `docs/*-ratchet.toml`,
  `docs/known-copies*.toml`, `docs/ratchets/**`, and the keys of `paddock.conformance.json` that the checks read as lists)
  is not declared;
- a declared list has no control fixture, or its control did not run in the module's tests.

**Seeding** (growth of a ratchet on purpose, such as ADR 0082 WP-0 adding its new rows to C71): a `[[seed]]` record in
`docs/ratchets.toml` names the ratchet, the accepted ADR, the rule ids being added and the date; the growth rule accepts
exactly the added entries whose rule id the seed names, in the commit that adds the seed record, and the record is
removed by the next `--write` run once the seed's commit is on main. Data, not a trailer (ADR 0075's reason: a commit
message cannot be corrected).

### 5. Positive controls, one per list (ER-33)

Each list's control fixture holds a tiny repository tree with **one live entry and one planted stale entry**, and, for
lists with a scope, **one entry outside the scope**. The control asserts: the stale entry is an error (a notice for the
drift log), the live one passes, the out-of-scope one is `unmeasured`; `--write` removes exactly the planted entry and
the file otherwise stays byte-equal; a planted **site** under a planted stale waiver is reported once the waiver is
removed (the check is not blind). The meta-check (§4) enumerates the lists, so a new list without its control fails.

| List | Planted stale entry |
|---|---|
| waivers | a C53 path waiver for a file without a kill; a C23 commit waiver for an ancestor outside the window |
| C71 baseline | a site hash whose line was rewritten |
| C13 `not_yet` | a member folder that does not exist; a member that already opts in |
| C79 adopting | a main that no longer exists; a main that calls the guardian |
| C100 tables, C102 `out_of_scope`, C24 rows | as their existing tests (kept; re-pointed at the engine) |
| endings ratchet | a fault entry for a scenario that now ends correctly |
| C37, C35, C62, C36 lists | a path or name that does not exist |
| known-copies | a copy whose paths match no tracked file (rule 55 (c)) |
| drift log | a record whose member left the register (notice, not error) |

### 6. Migration

1. **Now, before any code (WP-0):** remove the 25 stale entries in four commits, one per list, each after checking that
   its check still sees a planted site in that place (the control by hand): the 20 waivers (2 path waivers after `git
   grep` showed no C53/C54 site; 18 C23 commit waivers whose commits are ancestors of `origin/main` outside C23's
   window), the C71 entry (`paddock-timing-baseline.mjs --write`), `crates/debrief-ai-worker` from Debrief's C13
   `not_yet` (a shrink, which C100 allows), and the 3 gone mains from `ADOPTING`.
2. **Interim severities in the JS (WP-1), with a failing test each:** `W003` becomes an error, with `skipUnused`
   extended to C23 commit waivers whose commit is not in the run's history (unmeasured, not stale); C71's stale warning,
   C37's and C35's become errors; C13 reports a missing or opted-in `not_yet` member as an error; C79 reports a listed
   main it cannot find as an error. These are one-line severity changes in existing checks, not new Node tooling; they
   stop new dormant waivers while the Rust host is built.
3. **The engine and the list of lists (WP-2, WP-3)** in RT-5's host; C100 and C102 adopt it first (already Rust).
4. **Ports onto the engine:** C71 with RT-9, the waivers, C13, C79, C37, C35, C62, C36 with RT-17's batches; C58 (ADR
   0075 WP-2) is built on it. Each JS copy of the stale logic is registered in `docs/known-copies.toml` as a copy of a new
   mechanism `ratchet-entries` (canonical `tools/paddock-dev/src/check/ratchet.rs`, status `planned` until WP-2) and
   leaves with its check's retirement (rule 55).

### 7. Relation to other ADRs

- **ADR 0082 (C71 over counts).** RT-9 ports C71 onto this engine; ADR 0082 WP-0's regeneration with the new rows is a
  **seed** (§4) naming ADR 0082, so the growth is allowed once and every later growth is still an error. C71's
  shrink-only rule (ADR 0041 §4) is unchanged; only its stale severity changes.
- **ADR 0075 (copy waivers as data).** Unchanged: its records are this ADR's drift-log kind, stale is a notice (D4), a
  waiver that waives nothing a warning (D5), `copies --prune` its removal tool. What this ADR adds is for the register
  `docs/known-copies.toml` itself: a copy whose paths match nothing is an error (rule 55 (c)), checked by C58 through
  the engine.
- **ADR 0034 (one implementation per mechanism).** The stale check is itself a mechanism with about ten
  implementations; this ADR names the canonical one and registers the others as copies to delete (§6.4).
- **ADR 0060 (C100).** Its stale-entry logic is the seed of the engine; its `[[ratchet]]` gains the stale half it lacks
  (instance 3).
- **ADR 0072 (C102).** Already errors on a stale `out_of_scope`; adopts the engine for uniform reporting and `--write`.

## Parallelism and cost (rule 51)

The engine is bookkeeping over findings the checks already compute: one set lookup per entry and one parse per list
(the largest, C71's baseline, is 2,810 lines). Its cost is noise next to the checks (the whole root conformance run took
about 4 minutes at a load of 55 to 77 on 2026-11-10, a single unrepeated observation, not a benchmark). `--write` runs on
demand. Nothing to parallelise.

## Alternatives

- **Keep warnings and remove by hand in each batch.** Rejected: that is today, and it produced 25 dormant exceptions and
  hand removals like `6bf1b44c4`.
- **Expiry dates on every entry instead.** Rejected for ratchets: a date says nothing about whether the entry still
  matches, and moving dates is churn. Registers keep their dates as well (`W001`).
- **Fix each check's severity and stop there (WP-1 only).** Rejected as the end state: the ten copies would drift again;
  it is the interim step.
- **Make the drift log stale-is-error too.** Rejected: it would reopen ADR 0075 D4/D5 and recreate the cross-branch
  deadlock its P1-3 removed.

## Consequences

- A change that fixes the last site of an entry must remove the entry in the same change; `--write` makes that one
  command. A train that merges a fix with another branch's new waiver for the same place goes red until the integrator
  runs `--write`, a further ordinary commit.
- A check that goes blind shows up: its waivers turn stale and the control fails, instead of everything quietly passing.
- Lists in code move to data files.

## Rollback

Set a list's kind to report-only in `docs/ratchets.toml` (one line per list, visible in review) or revert WP-1's
severity changes; the removed stale entries are not restored (they matched nothing).

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D1 | Severity of a stale entry | (a) error for ratchets and registers, notice for ADR 0075's drift log; (b) error everywhere; (c) warning, removed in batches | **(a)** |
| D2 | Where the engine lives | (a) `./dev check` (RT-5's Rust host), the JS checks shimmed until ported; (b) the JS conformance host | **(a)**: ADR 0022 and the 2026-11-10 Rust-only decision |
| D3 | Interim severity flips in the JS checks (WP-1) before RT-5 | (a) yes, now, one failing test each; (b) wait for the Rust ports | **(a)**: RT-17 is weeks away and each stale waiver re-arms meanwhile |
| D4 | Remove today's 25 stale entries | (a) now (WP-0), one commit per list, each after a planted-site check; (b) with WP-1 | **(a)** |
| D5 | What `--write` may do | (a) remove stale entries only, refuse on unmeasured entries, never grow, never touch a drift log; (b) also re-key moved entries (C71 line moves) | **(a)**: re-keying is growth in disguise |
| D6 | Growth of a ratchet on purpose (ADR 0082's new rows) | (a) a `[[seed]]` record naming an accepted ADR and rule ids; (b) a commit trailer; (c) never | **(a)** |
| D7 | Lists in code (C79's `ADOPTING`) | (a) move to `docs/ratchets/<id>.toml`; (b) the engine edits source | **(a)** |
| D8 | Tag gating | (a) none beyond the normal gates: a stale entry in an app's scope already fails its conformance gate (ER-31 judges per app); (b) a tag also needs zero stale entries family-wide | **(a)** |
| D9 | Engineering rule | (a) a new ER-37: "A ratchet, baseline or waiver register fails on an entry that matches nothing; the only automatic edit is removal"; (b) a sentence in ER-21 | **(a)**: it applies to every project with baselines, not only to redesigns |

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
|---|---|---|
| WP-0 | Remove the 25 stale entries (§6.1), one commit per list, each with its planted-site check recorded in the commit | 0.5 |
| WP-1 | JS severity flips with failing tests first (§6.2): `W003` (and the C23 unmeasured case), C71, C37, C35, C13 `not_yet`, C79 gone mains | 1 |
| WP-2 | `check/ratchet.rs`: trait, judge, growth, scope, report; extracted from C100's code; property test that `--write` never adds | 2 |
| WP-3 | `docs/ratchets.toml`, the meta-check, `[[seed]]`, `./dev check ratchets [--write]`; control fixtures for every list in §5 | 1.5 |
| WP-4 | C100 (`not_yet` stale half included) and C102 on the engine; C79's `ADOPTING` to data | 1 |
| WP-5 | Register `ratchet-entries` in `docs/known-copies.toml`; the ER-37 text and the Paddock rule line; lessons entry | 0.5 |
| | **Total (new work)** | **6.5** |
| | Inside existing packages: C71 on the engine with RT-9 (+0.5), the JS lists with RT-17's batches (+1), C58 on it with ADR 0075 WP-2 (no extra) | (+1.5) |

First cut: WP-0 and WP-1 (1.5 days) close the four lists that are stale or silent today.

## Release impact (ER-31)

No shipped code changes. Conformance and `tools/paddock-dev` are outside every app's dependency set; a stale entry in an
app's scope fails that app's conformance gate like any other finding, so after WP-1 an app with stale entries in its
scope removes them before its tag (D8 (a)). Today that concerns Debrief (one C71 entry, one C53 waiver, the C13 member,
three C79 mains) and Dyno (one C54 waiver); WP-0 removes them first.
