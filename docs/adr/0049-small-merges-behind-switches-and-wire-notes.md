# 49. Small merges behind feature switches, a drift check, wire changes as notes, and conflict-free shared logs

- Status: Proposed (2026-11-04). The user approved the direction on 2026-11-04 (small merges behind switches, a drift
  check, wire notes with the version derived from them, conflict-free additive files); the details below are open for
  review and for the user's decisions (Decisions for the user).
- Date: 2026-11-04
- Related: ER-1 / rule 1 (one release at a time), ER-4 (Conventional Commits), ER-8 / rule 3 (the full suite once on the
  combined result), ER-21 / rule 40, rule 50 (CI economy), rule 54 (the release path builds installers), rule 55 and ADR
  0034 (one implementation per mechanism), ADR 0037 (the app composition contract: one `compose`), ADR 0038 (CI parity
  before push: `./dev check --pre-push`), ADR 0039 (release gates inspect the artifacts), ADR 0042 (one owner per harness,
  batch caps, the `commit-msg` hook, CI once per tree; it rejected trunk-based landing by every agent, which this ADR
  keeps rejected), the draft ADR 0047 on `origin/docs/redesign-adrs-7-9` (reuse green runs on `main`). Memory note "no
  backward compatibility before v1" (settings of unreleased features are deleted outright).
- Number: 0049. On `origin/main` (`f8dcd4440`) the highest is 0044; 0036 is reserved on `origin/docs/adr-logging-root-cause`
  (and two branches that carry it), 0045 to 0048 on `origin/docs/redesign-adrs-7-9` (checked with
  `git ls-tree -r --name-only origin/<branch> docs/adr/` over all remote branches, 2026-11-04). Check numbers C79 to C82:
  C74 to C76 are named in the 0045-0048 drafts, C77 is the "unknown check" in `conformance/test/core.test.mjs`, C78 is
  named in `docs/plans/debrief-1-2-api-dialogs.md` on `feat/dialog-size-to-content`.

## Evidence

All read in git at `origin/main` `f8dcd4440` and on the named remote branches, and in the coordinator log
`/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as "log" with its line), 2026-11-04. Measurements are
git queries; the two git experiments below ran on synthetic repositories in a scratch folder (git 2.56.0).

### Drift and late failures (the Dyno 1.1.0 landings, 2026-11-03 to 11-04)

| What | Instance | Source |
|---|---|---|
| A landing red after an hour on messages only | `steward/land-bc` CI 7461 ran about 1 h, then failed `commits` on 15 subjects over 100 characters written before the hook existed; the steward then squashed each source branch into a landing copy (`d636528da`, `330a30028`, `cf9d1002c` and 7 more on `main`) | log 727, 731 |
| Over-long subjects on the feature branches | 18 subjects of 101 to 115 characters on `feat/dyno-cached-views` (4), `feat/dyno-benchmark` (2), `perf/dyno-cpu-2` (6), `feat/dyno-network-features` (3) and others (`git log --format=%s origin/main..<branch>`, length > 100) | git |
| The hook arrived late, and does not cover every commit | the shared `.git/hooks/commit-msg` was installed by hand on 2026-11-03 18:35 (log 695; file time); `lefthook.yml` has had a `commit-msg` command since `ee0723ad8` (10-25), but lefthook is installed per checkout. A synthetic test shows `git cherry-pick` commits without running `commit-msg` (git 2.56.0): landing copies and cherry-picks skip the hook | log 695; experiment |
| One landing, five CI runs | `steward/land-d2`: run 7470 red (conformance release scripts, dyno ubuntu, dyno e2e-1), 7473 cancelled for a fold-in, 7475 red on `cargo fmt` of a Linux-only test, 7477 red on Windows (runner lock, paddock-dev), 7479 still running at 05:25 | log 762, 764, 770, 774, 777, 792 |
| fmt found only on the Linux job | `57ac35939` formats `apps/dyno/crates/dyno-agent/tests/bench_units.rs` (`#![cfg(target_os = "linux")]`), which the Mac's clippy and nextest skip; lesson "A fix to a cfg-gated test file went out unformatted" (2026-11-04). `838564a5e` formats 35 landed files at once | git; lessons.md |
| e2e on the combined tree only | `95a408147`: the menu-bar e2e looked for "Add Number" after a merged change renamed it "Add another Number". `e3f1e61ac`: "close the api types and Settings sections the merges left open" | git |
| Merges leave registration points open | per-branch drift at each head (`git merge-base`, then `git rev-list --count` and the intersection of the two `git diff --name-only` lists): every Dyno 1.1 line was based on 11-03 and was already 27 to 50 main commits behind; the files changed on both sides were 5 (`feat/dyno-benchmark`) to 30 (`feat/widgets-grid`); the same files recur: `dyno-core/src/protocol.rs`, `app/src/app/core/{api.types,backend,mock-backend,tauri-backend}.ts`, `app/src-tauri/src/commands.rs`, `dyno-agent/src/{agent,handlers,paths}.rs`, `CHANGELOG.md` | git (table below) |
| Hand-merged tray menus | the WoL and power squashes (`6142a3258`, `d37b03dc5`) each add tray menu entries; the coordinator reports the tray menus were merged by hand. I did not find the resolution in a commit message (unverified) | brief |

| Branch | Ahead | Behind | Files changed on both sides |
|---|---|---|---|
| `feat/dyno-benchmark` | 5 | 27 | 5 |
| `feat/dyno-cached-views` | 10 | 50 | 16 |
| `feat/dyno-helper-ipc-1b` | 13 | 50 | 10 |
| `perf/dyno-cpu-2` | 17 | 27 | 8 |
| `feat/dyno-helper-settings` | 47 | 27 | 7 |
| `feat/dyno-network-features` | 52 | 27 | 14 |
| `feat/widgets-grid` | 34 | 50 | 30 |

Age was not the problem (every base was a day old or less); breadth was. Ten lines changed the same registration points in
parallel, and each found the others only on the steward's combined tree.

### Wire protocol churn (Dyno's app-agent protocol)

- `dyno-v1.0.0` ships `PROTOCOL_VERSION = 3`; `main` is at 4 since `17c575b08` (11-03 15:40, the learning fix), with its own
  "4:" note.
- Four other branches each bumped 3 to 4 with their own "4:" note: `feat/dyno-cached-views` (saved disk maps),
  `feat/dyno-benchmark` (benchmarks, `2739fe6a8`), `perf/dyno-cpu-2` and `feat/dyno-tray-per-metric-2` (`tray_alert_percent`,
  `f2d5f2b01`); `feat/dyno-helper-ipc-1b` and `feat/dyno-network-features` stayed at 3 while adding variants. The
  coordinator set "one bump 3→4 for 1.1" by hand (log 683), and the landing copy `151ae9a75` wrote "one combined note" by
  hand. `protocol.rs` conflicted in two merges (`4eb6cdd11`, `d68e27a19`). On `steward/land-f` the doc comment above
  `PROTOCOL_VERSION` is now 25 lines of hand-merged history.
- New variants reached the round-trip guard only after landing: `apps/dyno/crates/dyno-core/tests/wire_roundtrip.rs`
  (from `test/dyno-wire-roundtrip`, landed as `6c34bd289`) says itself "a branch that adds variants adds its samples when it
  merges (the guard fails the merge result until it does)". Then `eaa2489fe` (samples for saved maps, benchmark consent,
  tray detail), `552ad09ce` (the helper variants) and `f41dd6157` (the intel types) followed, each on the landing branch.
- Pitcrew: the coordinator reports a local integration at "PROTOCOL 7" from main's 4 (log 791; not in a pushed commit,
  unverified). Debrief has `PROTOCOL_VERSION` 2 (`debrief-ai`) and `WIRE_VERSION` 3 (`debrief-store-client`).

### Conflicts in additive files

Every merge commit on any remote branch since 2026-11-02 (`git log --all --merges --since=2026-11-02`, the `# Conflicts:`
lists): **41 conflicted files in 27 merges; 34 of the 41 are append-only documents**: `apps/dyno/docs/adr/README.md` 8,
`CHANGELOG.md` 7, `docs/adr/README.md` 6, `apps/dyno/CHANGELOG.md` 6, `docs/lessons.md` 5, `apps/dyno/docs/performance.md` 2.
The other 7: `protocol.rs` 2, `agent.rs`, `lib.rs`, `paths.rs`, `link/mod.rs`, `Cargo.lock`. Follow-ups: `cbe1e6bc1` ("keep
the ADR index rows in number order after the merges"), `f41dd6157` (Dyno's ADR 0032 row twice). Picking this ADR's number
needed a scan of every remote branch.

## Context

**Pattern.** Work stays on branches until a whole feature is done, because unfinished work cannot be on `main` without
being in the next release. So many lines run in parallel for a day or more, all edit the same registration points, the
same version constant and the same logs, and the failures appear only on the steward's combined tree: as conflicts, as a
red run an hour later, and as hand-written combined notes. Checks that are cheap (commit messages, rustfmt) also run
there first, because no CI runs on a branch push (`ci.yml` triggers on `push` to `main`, `pull_request` and
`workflow_dispatch`) and local hooks are not installed everywhere and are skipped by cherry-picks.

ADR 0042 caps the size of a landing batch (60 commits, 200 files, 72 hours) and asks for a daily merge of `main`. It does
not let unfinished work land, so a feature line still holds its parts back until it is finished; and it has no measure of
drift on the branch itself.

**Invariant.** (a) Unfinished work reaches `main` in small reviewed parts, unreachable in a released build until it is
finished. (b) A branch sees its drift from `main` on every push. (c) A wire version is a function of the change notes and
the last release, so no two branches can disagree about it. (d) Concurrent additions to a shared log never conflict, and a
check catches what a blind merge gets wrong. (e) Checks that need no build run on every push.

## Decision

### 1. Feature switches: build-time, declared, default off, with a removal date

Each app declares its switches in `apps/<app>/switches.toml`:

```toml
version = 1

[[switch]]
id = "saved_disk_maps"            # snake_case; the Rust const is SAVED_DISK_MAPS, the TS name savedDiskMaps
title = "Saved disk maps (ADR 0029)"
state = "off"                     # "off" or "on"; a new switch starts "off"
owner = "feat/dyno-cached-views"  # the branch (or lease, ADR 0042) that finishes it
remove_by = "dyno-v1.2.0"         # an app tag or a date; at most the next minor tag or 30 days after the switch landed
adr = "apps/dyno/docs/adr/0029-saved-views.md"
```

- **Build-time only.** `paddock-build` (shared, ER-22) gains `Build::switches("switches.toml")`: it parses the file once
  (typed, with limits, never panics), writes `OUT_DIR/switches.rs` with one `pub const <ID>: bool` per switch, and
  `./dev gen switches` writes the app's `app/src/app/generated/switches.ts` from the same parser; a test checks that the
  checked-in TypeScript matches (one declared wire contract). There is no runtime setting, no hidden menu and no user
  override, so a released build behaves exactly as its file says and there is nothing to migrate when a switch goes.
- **Debug builds only** read `PADDOCK_SWITCHES=all-on` or `=<id>,<id>` (under `cfg(debug_assertions)`), so branch e2e and
  the nightly can run unfinished features. Release builds ignore it; ADR 0039's verify reads the switch table the binary
  prints with `--self-test` and compares it with `switches.toml` at the tag.
- **Gated at the edges.** A switch is read only where a feature becomes reachable: the composition (ADR 0037's `compose`:
  command registration, menus and tray entries, settings sections, routes, agent handlers). Pure core code never branches
  on a switch. Off means: the code compiles, its unit tests run, its commands and handlers are not registered (a call
  answers "not available"), and its settings section is not registered (`paddock-settings` drops unknown keys when it
  reads values, `crates/paddock-settings/src/values.rs`).
- **No compatibility shims.** Before v1 a finished feature turns `on` and its switch is deleted in the same release; an
  abandoned one is deleted with its code and settings. Neither keeps old keys or old paths.
- **Shared code is not exempt from rule 1.** A switch hides a feature from users; it does not make a change to `crates/`
  or `ui/` safe for a release that depends on them.

**Check C79** (`paddock-conformance switches`): the file parses; every id is used and every use is in a declared
composition file (`[uses] files = [...]` in the same file, default the app's `compose` module and its UI routes); a switch
past `remove_by` fails, and a tag is refused while its app has a switch whose `remove_by` tag it has reached (the same
shape as rule 55 (d)); a date moves only with `Switch-date: <id>: <old> -> <new>: approved: <coordinator|user>: <reason>`
(user for a later release line).

### 2. Small merges: a line lands its parts, not its end

- A feature line's first part lands within 24 hours of its first commit: its switch (off), its skeleton and its first
  tests. Each further reviewed part lands when green. A line holds at most **25 non-merge commits and 48 hours** of
  unlanded work (`[line] max_commits`, `max_age` in `docs/harnesses.toml`, beside ADR 0042's batch caps; data, not code).
- The steward still lands, in ADR 0042's windows and cadence (at most every 8 hours); a landing batch is several small
  parts, under the batch caps. Agents never push to `main` (ADR 0042's rejection of trunk-based landing stands).
- Registration points that every line edits (`api.types.ts`, the backends, `commands.rs`, `handlers.rs`) are named as
  **hot files** in `docs/harnesses.toml` (`[drift] hot = [...]`); a part that touches one lands in the next window, so the
  next line merges it a day earlier. Making those points additive (one file per feature, registered by a list) is a
  follow-up for ADR 0037, not part of this ADR.
- The release line lands untagged parts as before (ADR 0042 §3); with switches, its unfinished parts can land too.

### 3. The drift check: C81 and `./dev drift`

`./dev drift [--branch <b>]` compares a branch head with `origin/main`:
- **behind**: non-merge commits on `main` since the merge base, and hours since the branch last merged `main`;
- **unlanded**: non-merge commits on the branch not on `main`, and the age of the oldest;
- **overlap**: files changed both on `main` since the merge base and on the branch, with the hot files marked.

Thresholds are data in `docs/harnesses.toml` `[drift]`: **warn** when behind is over 30 commits or 24 hours, unlanded is
over the line caps, or any hot file overlaps; **fail** (C81) when unlanded is past twice the line caps or `main` was last
merged more than 72 hours ago. It runs in the push check (§6) as a warning annotation with the numbers and the next step
("merge origin/main; land part N"), in `./dev check --pre-push` (ADR 0038), and in the steward's landing step, where a
failure needs `Batch-size-waived:` (ADR 0042) like a cap.

### 4. Wire changes as notes, the version derived from them: C80

Each app with a versioned wire declares it in `apps/<app>/wire.toml`:

```toml
version = 1

[[protocol]]
id = "app-agent"
constant = { file = "crates/dyno-core/src/protocol.rs", name = "PROTOCOL_VERSION" }
types = ["crates/dyno-core/src/protocol.rs", "crates/dyno-core/src/helper_wire.rs", "crates/dyno-core/src/jobs.rs"]  # seeded in WP-4
roundtrip = "crates/dyno-core/tests/wire_roundtrip.rs"
tags = "dyno-v*"
notes = "wire-changes/app-agent"
```

A wire change adds one note file, `apps/<app>/wire-changes/<protocol>/<yyyy-mm-dd>-<slug>.toml`:

```toml
kind = "breaking"         # "breaking" (positional field, removed or reordered variant) or "additive" (variant at the end)
summary = "saved disk maps (ADR 0029)"
changed = ["DiskScanStatus.origin", "StartDiskScan.replaces"]
added = ["Request::ForgetSavedDiskMap", "Request::ForgetSavedDiskMaps"]  # each needs a round-trip sample
```

- **The version is computed, never chosen.** `version = released + 1` if any `breaking` note is absent from the last tag's
  tree, else `released`, where `released` is the constant in that tag's tree (`git show <tag>:<file>`). `./dev wire sync`
  writes that value into the constant. Any branch may run it; two branches that both need the bump write the same line,
  which git merges without a conflict. The integrator's landing step runs `./dev wire sync --check` instead of writing a
  combined note by hand. One bump per release line follows by construction (the coordinator's 1.1 policy, log 683).
- **The history lives in the notes.** The comment above the constant shrinks to one line pointing at the notes folder;
  `./dev wire log --app dyno` prints the history by version. The release notes of a tag may include it.
- **C80** (`paddock-conformance wire --range <base>..HEAD`): a range that changes a declared `types` file adds or edits a
  note, or carries `Wire-none: <protocol>: <reason>` (for a comment or a rename that does not change the encoding); the
  constant equals the computed version; every `added` entry of a note appears as a sample in the `roundtrip` file
  (textual; the full guard, `every_variant_has_a_sample`, runs as the crate's test in `./dev check --pre-push` and CI). A
  version line changed by hand to anything else fails.
- Seeded for Dyno's `app-agent` and helper wire, Pitcrew's protocol, and Debrief's `debrief-ai` worker and store-client
  wires. The round-trip helper (`try_round` in Dyno's guard) moves to `paddock-testkit::wire` so the other apps' guards
  use one implementation (rule 55).

### 5. Additive files merge without conflicts, and a check catches the blind spots: C82

- `.gitattributes` marks the append-only logs `merge=union` (git's built-in driver): `CHANGELOG.md`, `apps/*/CHANGELOG.md`,
  `docs/lessons.md`, `docs/adr/README.md`, `apps/*/docs/adr/README.md`, `docs/performance.md` files, `docs/benchmarks.md`.
  A synthetic test (git 2.56.0) shows the driver keeps both sides' appended rows and sections with no conflict, and also
  what it gets wrong, silently: two rows with the same number, both versions of a row both sides edited, and a missing
  blank line between two appended sections.
- **C82** (`paddock-conformance logs`) checks every such file on the tree: index rows are unique by number, sorted, one
  per ADR file and pointing at an existing file; no heading appears twice in a lessons file or in a changelog's
  `Unreleased` section; no line is repeated within a section; a blank line precedes every heading. It runs in the push
  check, at landing and in `--pre-push`. Its fix is mechanical (`./dev logs fix` sorts rows and restores blank lines);
  duplicates are left to the author.
- **`./dev adr next [--app <app>]`** prints the next free ADR number over `origin/main` and every remote branch (the scan
  done by hand for this ADR), so two branches do not pick one number.

### 6. Checks that need no build run on every push: `push-check.yml`

A new workflow, `.gitea/workflows/push-check.yml`, on `push` to every branch except `main`, one job in the `paddock-ci`
container on a Linux runner, with no build and no tests:
- `commit-lint` over the pushed range (the commits not on `origin/main`), the same code as the `commits` job;
- `rustfmt --check --edition 2021` on the changed `.rs` files (rustfmt parses cfg-gated files, so a Linux-only test is
  checked on any host);
- C79, C80, C81 (warning or failure as in §3), C82, and ADR 0042's C72 (harness homes).

It cancels an older run of the same branch, writes a one-line job summary per check, and prints an `::error::` with the
cause and the next step (ER-27). Budget: 2 minutes per push, measured in WP-7. Local hooks and `./dev check --pre-push`
stay the first line; this job is the backstop for what hooks miss (cherry-picks, uninstalled hooks, other machines).

## How this fits the rules

- **Rule 50 (CI economy), amendment proposed.** The push check is a run on every branch push, which (a) and (b) would
  forbid as written. Proposed (f): "The push check (`push-check.yml`: commit messages, rustfmt of changed files and the
  register checks; no build, no tests, at most 2 minutes on one Linux container) runs on every branch push and is not a CI
  run under (a) to (c)." The gate runs keep (a) to (e) unchanged. Smaller landings mean more main runs; ADR 0042's
  cadence bounds them at three a day, and the draft ADR 0047 would let a fast-forward landing reuse the branch's green
  runs. `land-d2`'s five runs and `land-bc`'s hour lost on messages are what this should remove.
- **Rule 54 (release path).** A part that touches the release path still dispatches `<app>-release.yml` with gates and
  bundles before it lands; a switch does not exempt it. `switches.toml` itself is on the release path (it decides what the
  installer contains), so a change of a switch's `state` builds the installers before it lands.
- **Rule 55 (one mechanism).** Switches are one mechanism in `paddock-build`, wire notes one mechanism in `./dev` and
  conformance, the round-trip helper one in `paddock-testkit`; no app writes its own. C58 then treats a second switch
  parser or wire-version reader as a copy.
- **ER-1 / rule 1.** The release's parts take the landing windows first. Other lines land in the remaining windows, and
  only if they do not change shared code the release depends on; a switch does not change that.
- **ER-8 / rule 3.** The full suite runs once on the combined result with the shipped switch values, before a local build
  or release. The nightly adds one run with every switch on (debug build, `PADDOCK_SWITCHES=all-on`), so unfinished
  features are tested combined too.
- **Rule 61 (ADR 0042).** Its batch caps, harness homes and daily `main` merge stay. This ADR adds per-line caps and the
  drift numbers; the proposed rule 61 text gains one clause (below).

**Proposed rule 65** (the next number after those named by ADRs 0041, 0042 and the drafts 0045 to 0048, up to 64;
renumbered at acceptance if they take fewer): "Small merges behind switches. Unfinished work lands in reviewed parts behind
a build-time switch (`apps/<app>/switches.toml`, default off, a removal tag or date, no runtime override in release
builds, no compatibility shims), read only at the composition; a line holds at most 25 commits and 48 hours unlanded. A
wire version is derived from note files (`apps/<app>/wire-changes/`) and the last tag, never chosen by hand; a wire change
adds its note and its round-trip samples in the same change. Append-only logs merge with `merge=union` and are checked by
C82. Commit messages, rustfmt and the register checks run on every push. Checked by C79 to C82. *Why:* the Dyno 1.1
landings found five independent "4:" protocol notes, 34 of 41 conflicts in append-only files, and messages and formatting
failures an hour into a landing run (ADR 0049)."

Proposed one-clause amendment to rule 61: "a line lands its parts behind a switch (rule 65) instead of holding them for
its end."

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C79** switches | every switch, its uses and its `remove_by`; tags against reached deadlines | (none: no switches yet) the ad-hoc "disk test off until WP-B3" in `e5acb772b` becomes a switch |
| **C80** wire notes | every range touching a declared wire file; the constant against the computed version; note samples | the six Dyno lines above (no notes; two at version 3 with new variants); `steward/land-f`'s hand-merged history |
| **C81** drift | each branch head against `origin/main` | warns on all seven lines in the drift table (hot files overlap) |
| **C82** logs | every row and heading of the append-only logs | the duplicate ADR 0032 row fixed in `f41dd6157`, the order fixed in `cbe1e6bc1` |
| push check | every branch push | `land-bc`'s 15 over-long subjects, `bench_units.rs` formatting, at push time instead of after an hour |

## Work packages (agent-days, estimates)

| WP | What | Days |
|---|---|---|
| WP-1 | `push-check.yml` (commit-lint over the pushed range, rustfmt on changed files, summary, `::error::`), its ci-actions test; rule 50 (f) text | 1 |
| WP-2 | `./dev drift` in `tools/paddock-dev`, `[drift]` and `[line]` in `docs/harnesses.toml`, C81, fixtures | 1 |
| WP-3 | `merge=union` lines in `.gitattributes`; C82 with fixtures for each blind spot of the union driver; `./dev logs fix`; `./dev adr next` | 1 |
| WP-4 | `wire.toml` and notes for Dyno (the 1.1 notes split from the combined comment), Pitcrew and Debrief; `./dev wire sync`, `--check`, `log`; C80; `paddock-testkit::wire` | 1.75 |
| WP-5 | `paddock-build::Build::switches`, the Rust and TS generation with the agreement test, the debug-only override, C79 with tag refusal, the `--self-test` switch table and ADR 0039's probe, the nightly all-on leg | 2.5 |
| WP-6 | rule 65 and the rule 61 clause, AGENTS.md "Working in the monorepo", the `integrate` skill's landing step (`wire sync --check`, `drift`, `logs`) | 0.5 |
| WP-7 | after two weeks: push-check minutes per day, conflicts per merge, runs per landing, unlanded age; recorded in `docs/benchmarks.md` (rule 48); tune the caps and thresholds | 0.5 |
| **Total** | | **8.25** |

Order: WP-1 and WP-3 first (they remove the cheapest failures and most conflicts, and need no product code), then WP-4
(before the next wire change), WP-2, WP-5 (first used by the next feature line after the Dyno 1.1.0 tag), WP-6, WP-7. WP-1,
WP-2 and WP-4's `./dev` parts are CI kit and `paddock-dev` harness work (ADR 0042): they are made on those homes and land
outside the Dyno release window (ADR 0022 G.7). WP-5 changes `crates/paddock-build` (shared code on every app's release
path): after the current release, with every app's `build.rs` updated in the same change (rule 39, 42).

## Parallelism and cost (rule 51)

The push check is a single short job: git queries, rustfmt on a handful of files and TOML parsing; it parallelises nothing
and needs no cache. Its cost is the container start plus seconds of work per push; on 11-02 and 11-03 the remote branches
gained 277 non-merge commits across 52 heads, so the job runs tens of times a day (pushes are fewer than commits; WP-7
measures it). The cheaper operation it buys: a red landing run is about 1 to 1.5 hours of the shared runners (log 727;
memory `ci-economy`).

## Consequences

- Feature work reaches `main` daily, so conflicts arrive one part at a time and e2e sees the combined menus and labels a
  day earlier, not at the end.
- Dormant code ships in releases while a switch is off. It is unreachable (not registered), checked by C79 and ADR 0039's
  probe, and has a deadline.
- Branches never write a combined protocol note again; the version cannot disagree between branches. A breaking change
  in a release line bumps once.
- Union merges never stop a merge, so a semantic clash in a log is found only by C82. C82 must exist before the attributes
  go in (WP-3 lands both together).
- One more Linux job per push, and one more config file per app (`switches.toml`, `wire.toml`).

## Alternatives considered

- **Runtime feature flags (a hidden setting or a remote list).** They can be flipped without a build, but they are user
  state that must be migrated or deleted, a support surface, and a released build would not be fixed by its tag. Rejected
  before v1; build-time switches have none of that.
- **Cargo features as switches.** They change the dependency graph per build and multiply the clippy and test matrix;
  generated consts keep one graph and let both paths compile. Rejected.
- **Bump `PROTOCOL_VERSION` only by the integrator at landing** (the brief's first form). It still needs one person to read
  every branch's notes and write the comment, and a branch build carries the old number with a breaking change, so it can
  talk to an installed agent of the released version. The derived version removes both; the integrator only checks it.
  Kept as decision D1.
- **Changelog and lessons fragments** (one file per entry, assembled at release, as towncrier or changesets do). They
  remove conflicts completely, but need an assembler, break the lessons anchors that rules.md links, and change what the
  first-release step reads (`first-release-changelog` reads a section of `CHANGELOG.md`). Kept as the fallback if WP-7
  finds the union driver plus C82 wrong in practice.
- **A Gitea server-side `pre-receive` hook for commit messages.** No runner at all, but Gitea disables custom git hooks by
  default (`DISABLE_GIT_HOOKS = true`) because they run arbitrary code on the host
  ([config cheat sheet](https://docs.gitea.com/administration/config-cheat-sheet), read 2026-11-04), and rustfmt would
  need a toolchain on the server. Rejected; the push check runs the same code in the CI container.
- **Only local hooks** (ADR 0038 `--pre-push`, ADR 0042 `commit-msg`). They stay, but the evidence shows commits that
  never ran them (cherry-picks, commits before the hook was installed). Not enough alone.
- **Long-lived integration branches per release line.** That is what the steward branches are now; it moves the drift,
  it does not remove it. Rejected.

## Decisions for the user

- **D1:** the wire version **derived from the notes** by `./dev wire sync` on any branch (recommended), or bumped only by
  the integrator at landing.
- **D2:** the **push check on every branch push** and the rule 50 (f) amendment, with a 2-minute budget.
- **D3:** switches **build-time only** (recommended), `remove_by` at most the next minor tag or 30 days.
- **D4:** per-line caps of **25 commits and 48 hours**, and the drift thresholds (warn 30 commits or 24 hours or a hot
  file; fail at twice the line caps or 72 hours without a `main` merge).
- **D5:** the **union driver plus C82** for the logs now, fragments only if WP-7 shows it fails (recommended), or
  fragments now.
- **D6:** squash landing copies (log 731; `151ae9a75`) end once the push check is in, as ADR 0042 §4 already says; confirm.

## Rollback

Switch files, caps and thresholds are data; C79 to C82 can be warnings by one line each; the push check is one workflow
file; the union lines are removed from `.gitattributes`.
