# 49. Small merges behind feature switches, a drift check, wire schemas and notes, and conflict-free shared logs

- Status: Proposed (2026-11-04), revision 3. The user approved the direction on 2026-11-04 and accepted decisions D1 to D6
  as recommended in revision 1; revision 2 answered the independent review (verdict "needs rework") and adjusted D1, D2,
  D3 and D5 where the fixes required it; revision 3 answers the re-review ("needs another round": the wire schema, the
  helper and plugin wires, the freeze's enforcement). The user accepted D7 (the release freeze, with dormant wire changes
  bumping the shipped protocol) on 2026-11-04; no other decision changed in revision 3.
- Date: 2026-11-04
- Related: ER-1 / rule 1 (one release at a time), ER-4 (Conventional Commits), ER-8 / rule 3 (the full suite once on the
  combined result), ER-14 / rule 7 and rule 49 (tests never read the user's environment; no leaked state), ER-21 / rule
  40, rule 50 (CI economy), rule 54 (the release path builds installers), rule 55 and ADR 0034 (one implementation per
  mechanism), ADR 0037 (the app composition contract: one `compose`), ADR 0038 (CI parity before push: `./dev check
  --pre-push`), ADR 0039 (release gates inspect the artifacts), ADR 0042 (one owner per harness, batch caps, the
  `commit-msg` hook, CI once per tree; it rejected trunk-based landing by every agent, which this ADR keeps rejected; §3 is
  amended below), ADR 0022 G.7 (waves and ordering; on `origin/docs/adr-0022-generic-tools`, not yet on `main`), the draft
  ADR 0047 on `origin/docs/redesign-adrs-7-9` (fast-forward landings reuse green runs). Memory note "no backward
  compatibility before v1".
- Number: 0049. On `origin/main` (`f8dcd4440`) the highest is 0044; 0036 is reserved on `origin/docs/adr-logging-root-cause`
  (and two branches that carry it), 0045 to 0048 on `origin/docs/redesign-adrs-7-9` (`git ls-tree -r --name-only
  origin/<branch> docs/adr/` over all remote branches, 2026-11-04). Checks C79 to C82: the highest check file on any
  branch is `conformance/src/checks/C78-dialog-sizing.mjs` (`origin/steward/land-f`); C74 to C76 are named in the 0045-0048
  drafts; C77 is the "unknown check" in `conformance/test/core.test.mjs`.

## Evidence

All read in git at `origin/main` `f8dcd4440` and on the named remote branches, and in the coordinator log
`/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as "log" with its line), 2026-11-04. The git
experiments ran on synthetic repositories in a scratch folder (git 2.56.0).

### Drift and late failures (the Dyno 1.1.0 landings, 2026-11-03 to 11-04)

| What | Instance | Source |
|---|---|---|
| A landing red after an hour on checks that need no build | `steward/land-bc` CI 7461 ran about 1 h, then failed `commits` on 15 subjects over 100 characters written before the hook existed **and** C24 (11 icon images without REVIEWED rows); the steward then squashed each source branch into a landing copy. `main` has 13 "land … as one commit" copies since 11-03 (`git log origin/main --since=2026-11-03`) | log 727, 731 |
| Over-long subjects on the feature branches | 24 distinct subjects of 101 to 115 characters, committed since 11-02 and not on `main`, on 12 remote `feat/`, `perf/`, `fix/` and `test/` `dyno-*` branches (command: `git log --no-merges --since=2026-11-02 --format=%s origin/main..<branch>`, length > 100, de-duplicated by subject): `feat/dyno-cached-views` 4, `feat/dyno-tray-per-metric` 4, `fix/dyno-disk-startup` 3, `fix/dyno-windows-shell` 3, `feat/dyno-benchmark` 2, `feat/dyno-notifications-expand` 2, and one each on `feat/dyno-helper-requests`, `feat/dyno-helper-service`, `feat/dyno-network-features`, `feat/dyno-tray-per-metric-2`, `fix/dyno-agent-via-launchd-2`, `fix/dyno-tray-notch` | git |
| The hook arrived late, and does not cover every commit | the shared `.git/hooks/commit-msg` was installed by hand on 2026-11-03 18:35 (log 695; file time); `lefthook.yml` has had a `commit-msg` command since `ee0723ad8` (10-25), but lefthook is installed per checkout. A synthetic test shows `git cherry-pick` commits without running `commit-msg` | log 695; experiment |
| One landing, five CI runs; one failure the push check would catch | `steward/land-d2`: 7470 red (conformance release scripts, dyno ubuntu, dyno e2e-1); 7473 superseded by the fix head `95a408147`; 7475 red on `cargo fmt` of a Linux-only test; 7477 red on paddock-dev Windows; the Dyno release run 7478 red on the Windows runner's `stable-workspace` lock; 7479 still running at 05:25. **Only 7475's failure needs no build**, so only that one is claimed for the push check | log 762, 764, 770, 774, 777, 792 |
| fmt found only on the Linux job | `57ac35939` formats `apps/dyno/crates/dyno-agent/tests/bench_units.rs` (`#![cfg(target_os = "linux")]`), which the Mac's clippy and nextest skip; lesson "A fix to a cfg-gated test file went out unformatted" (2026-11-04). `838564a5e` formats 35 landed files at once | git; lessons.md |
| e2e on the combined tree only | `95a408147`: the menu-bar e2e looked for "Add Number" after a merged change renamed it "Add another Number". `e3f1e61ac`: "close the api types and Settings sections the merges left open" | git |
| Breadth: ten lines edit the same registration points | per-branch drift at each head (table below) | git |
| Hand-merged tray menus | the WoL and power squashes (`6142a3258`, `d37b03dc5`) each add tray menu entries; the coordinator reports they were merged by hand. Not found in a commit message (unverified) | brief |

| Branch | Unlanded | Behind | Files changed on both sides |
|---|---|---|---|
| `feat/dyno-benchmark` | 5 | 27 | 5 |
| `feat/dyno-cached-views` | 10 | 50 | 16 |
| `feat/dyno-helper-ipc-1b` | 13 | 50 | 10 |
| `perf/dyno-cpu-2` | 17 | 27 | 8 |
| `feat/dyno-helper-settings` | 47 | 27 | 7 |
| `feat/dyno-network-features` | 52 | 27 | 14 |
| `feat/widgets-grid` | 34 | 50 | 30 |

(`git merge-base`, `git rev-list --count --no-merges` both ways, and the intersection of the two `git diff --name-only`
lists.) Every base was a day old or less: age was not the problem, breadth was. The same files recur:
`dyno-core/src/protocol.rs`, `app/src/app/core/{api.types,backend,mock-backend,tauri-backend}.ts`,
`app/src-tauri/src/commands.rs`, `dyno-agent/src/{agent,handlers,paths}.rs`, `CHANGELOG.md`.

### Wire protocol churn (Dyno's app-agent protocol)

- `dyno-v1.0.0` ships `PROTOCOL_VERSION = 3`; `main` is at 4 since `17c575b08` (11-03, the learning fix), with its own "4:"
  note. Four other branches each bumped 3 to 4 with their own "4:" note (`feat/dyno-cached-views`, `feat/dyno-benchmark`
  `2739fe6a8`, `perf/dyno-cpu-2`, `feat/dyno-tray-per-metric-2` `f2d5f2b01`); `feat/dyno-helper-ipc-1b` and
  `feat/dyno-network-features` stayed at 3 while adding variants. The coordinator set "one bump 3→4 for 1.1" by hand (log
  683); the landing copy `151ae9a75` wrote "one combined note" by hand; `protocol.rs` conflicted in two merges (`4eb6cdd11`,
  `d68e27a19`). On `steward/land-f` the comment above the constant is 25 lines of hand-merged history.
- **The wire is not one file.** `protocol.rs` imports 16 modules of `dyno-core` (`use crate::…`); `DiskScanStatus` is in
  `dyno-core/src/disk/scan.rs`; `land-f` adds wire types under `power/`, `wol/`, `helper_*.rs`, `intel*`, `tray/` and the
  widget layouts. postcard is positional and not self-describing, so a new field anywhere in that graph changes the
  encoding.
- New variants reached the round-trip guard only after landing: `wire_roundtrip.rs` (landed as `6c34bd289`) says "a branch
  that adds variants adds its samples when it merges"; then `eaa2489fe`, `552ad09ce` and `f41dd6157` added samples on the
  landing branch.
- Pitcrew: the coordinator reports a local integration at "PROTOCOL 7" from main's 4 (log 791; unverified). Pitcrew's and
  Debrief's wires are JSON (`serde_json` in `pitcrew-core`, `debrief-ai`).

### Conflicts in additive files

Merge commits on remote branches since 2026-11-02 (`git log --remotes --merges --since=2026-11-02`, the `# Conflicts:`
lists): **35 conflicted files in 21 merges, 28 of them append-only documents**: `CHANGELOG.md` 7, `apps/dyno/CHANGELOG.md`
6, `docs/lessons.md` 5, `apps/dyno/docs/adr/README.md` 5, `docs/adr/README.md` 3, `apps/dyno/docs/performance.md` 2. The
other 7: `protocol.rs` 2, `agent.rs`, `lib.rs`, `paths.rs`, `link/mod.rs`, `Cargo.lock`. Follow-ups: `cbe1e6bc1` (ADR index
order), `f41dd6157` (Dyno's ADR 0032 row twice). Picking this ADR's number needed a scan of every remote branch.

## Context

**Pattern.** Work stays on branches until a whole feature is done, because unfinished work cannot be on `main` without
being in the next release. So many lines run in parallel, all edit the same registration points, the same version
constant and the same logs, and the failures appear only on the steward's combined tree. Checks that need no build also
run there first, because no CI runs on a branch push (`ci.yml` triggers on `push` to `main`, `pull_request` and
`workflow_dispatch`) and local hooks are not installed everywhere and are skipped by cherry-picks.

ADR 0042 caps a landing batch (60 commits, 200 files, 72 hours) and asks for a daily merge of `main`. It does not let
unfinished work land, and it measures nothing on the branch itself.

**Invariant.** (a) Unfinished work reaches `main` in small reviewed parts, unreachable in a released build until it is
finished. (b) A branch sees its drift from `main` on every push. (c) A wire version is a function of the code's wire schema
and the last release, so no two branches can disagree about it and no note can mislabel it. (d) Concurrent additions to a
shared log never conflict where a blind merge is safe, and stay visible where it is not. (e) Checks that need no build run
on every push.

## Decision

### 1. Feature switches: declared, default off, injected at the composition, with a removal tag or date

Each app declares its switches in `apps/<app>/switches.toml`:

```toml
version = 1

[[switch]]
id = "saved_disk_maps"            # snake_case; the generated Id variant is SavedDiskMaps
title = "Saved disk maps (ADR 0029)"
state = "off"                     # "off" or "on"; a new switch starts "off"
owner = "feat/dyno-cached-views"  # the branch (or ADR 0042 lease) that finishes it
remove_by = "dyno-v1.2.0"         # an app tag or a date: at most the next minor tag or 30 days after the switch landed
adr = "apps/dyno/docs/adr/0029-saved-views.md"
```

- **Generated table, injected.** `paddock-build` (shared, ER-22) gains `Build::switches("switches.toml")`: it parses the
  file once (typed, with limits, never panics) and writes `OUT_DIR/switches.rs` with an `Id` enum, a `Switches` table
  type, `Switches::SHIPPED` (a `const` from the file) and `Switches::current()`. In a release build `current()` is
  `SHIPPED`. In a debug build (`cfg(debug_assertions)`) it reads `PADDOCK_SWITCHES` once through a `OnceLock`
  (`all-on`, or `<id>,<id>` turned on over the shipped values); an unknown id fails the start with its name.
  `current()` is called once, in `main` of each composing binary. **`compose` takes the table as a parameter** (ADR
  0037's one `compose`), so tests pass `Switches::all_on()` or a chosen table and never read the environment (ER-14, rule
  49).
- **The UI asks the backend.** The table reaches the UI through the backend port (a `switches()` command on the injected
  backend). `./dev gen switches` writes the `SwitchId` string-literal union into `app/src/app/generated/switches.ts` and the
  shipped table into a spec-support file, `app/src/testing/switches.shipped.ts`; a test checks both against the TOML. There
  is no compiled TS constant to override.
- **The mock's default is the shipped table.** The mock backend returns `switches.shipped.ts` unless a spec says otherwise;
  all-on is always explicit: a spec calls the mock's `useSwitches('all-on')` (or a chosen table), and the browser e2e has a
  named all-on project in the nightly.
- **Where the all-on runs come from.** The nightly's all-on legs are that browser e2e project (mock backend) and a debug
  build of the real app started with `PADDOCK_SWITCHES=all-on`, running the real-app smoke and e2e (ADR 0037) through the
  backend port. Branch runs and release builds use the shipped table; release builds ignore the variable.
- **Gated at the edges.** A switch is read only in `compose` and in the UI's routes and menus (from the backend's table).
  Pure core code never branches on a switch. Off means: the code compiles, its unit tests run, its commands, handlers,
  tray and menu entries and settings section are not registered (a call answers "not available"; `paddock-settings` drops
  unknown keys when it reads values, `crates/paddock-settings/src/values.rs`).
- **No migration for settings that never shipped on.** Before v1, settings of a feature that never shipped with its switch
  on are deleted with the feature, or kept as they are when it turns on; nothing reads old keys. A finished feature turns
  `on` and its switch (the `Id` and every check of it) is deleted in the same release.
- **The shipped table is verified.** The binaries that call `compose` print their table with `--switches` (JSON, exit 0,
  before any window or socket): for Dyno the app binary and `dyno-agent` (neither has `--self-test` today:
  `.gitea/workflows/dyno-release.yml` line 98). ADR 0039's verify starts both from the built installer and compares the
  output with `switches.toml` at the tag.
- **Deadlines.** `remove_by` is **reached** by a tag of the same app whose version is greater than or equal to it
  (`dyno-v1.2.0` is reached by `dyno-v1.2.0`, `dyno-v1.2.1` and `dyno-v1.3.0`, not by `dyno-v1.1.3`), or for a date at
  00:00 UTC that day.
- **Shared code is not exempt from rule 1**; a switch hides a feature, it does not make a change to `crates/` or `ui/`
  safe for a release that depends on them.

**Check C79** (`paddock-conformance switches`): the file parses; every id is used and only in `compose` and the declared UI
route and menu files (`[uses] files = [...]`); the generated `SwitchId` matches; a tag is refused while its app has a
switch whose `remove_by` it reaches (rule 55 (d)'s shape); a date moves only with `Switch-date: <id>: <old> -> <new>:
approved: <coordinator|user>: <reason>` (user for a later release line).

### 2. Small merges: a line lands its parts, not its end

- A feature line's first part lands within 24 hours of its first commit: its switch (off), its skeleton and its first
  tests. Each further reviewed part lands when green. A line holds at most **25 non-merge commits and 48 hours** of
  unlanded work (`[line] max_commits`, `max_age` in `docs/harnesses.toml`, beside ADR 0042's batch caps; data, not code).
- The steward still lands, in ADR 0042's windows and cadence (at most every 8 hours); a landing batch is several small
  parts, under the batch caps. Agents never push to `main`.
- **Hot files** (registration points every line edits) are listed in `docs/harnesses.toml` `[drift] hot = [...]`, seeded
  with Dyno's `api.types.ts`, the three backends, `commands.rs`, `handlers.rs`, `agent.rs` and `paths.rs`. Rule: **a
  reviewed part that touches a hot file is in the next landing batch** (it may not wait for a later window), and within a
  batch the steward merges hot-file parts first, so every other open line merges them from `main` at its next daily merge.
- Making the hot files additive (one file per feature, registered by a generated list) is WP-8 (below).

### 3. Release freeze (D7, accepted)

**Opening and closing.** The coordinator opens an app's freeze when it cuts the release batch. The steward's merge
commit on `main` that lands that batch (or its first part) carries the trailer `Freeze: <app> <version>`. The tag
`<app>-v<version>` closes the freeze; a freeze given up is closed by `Freeze-closed: <app> <version>: <reason>` on a
later landing merge. A freeze is open while `main` has a `Freeze:` trailer with neither.

**What may land.** While a freeze is open, only that release's parts land in `apps/<app>/**` and in the shared code it
depends on (rule 1 already covers the shared code). Other lines of the same app wait, switch or not: dormant code still
changes the release tree and its tests.

**Enforced at landing.** The steward marks each landing merge that carries a release part with `Release-part: <app>
<version>`. C81's landing mode (`./dev drift --landing <range>`) refuses a range that changes `apps/<app>/**` while a
freeze of `<app>` is open and the merge has no matching `Release-part:` (WP-2).

**The caps yield to the freeze.** While a freeze of `<app>` is open, C81 only warns (never fails) for that app's other
lines, and their 24-hour first-part rule (§2) is suspended; the warnings still show their size, so the steward can plan
the first landings after the tag.

**Dormant wire changes bump the shipped protocol.** postcard is positional: a field added behind an off switch is still
encoded, so the shipped wire changes and §4 derives a new version from the schema whatever the switch says. That is
correct and accepted: §4 bumps at most once per release line, and says what a bump costs for each wire (the helper's
needs a helper release). A line that does not want
to bump a released protocol keeps its wire change out of the freeze (it lands after the tag).

### 4. Wire schemas, golden encodings and notes, the version derived from them: C80

Each app with a versioned wire declares every wire separately in `apps/<app>/wire.toml`. Dyno has three:

```toml
version = 1

[[protocol]]
id = "app-agent"                          # the app (and the dyno command) <-> the agent
format = "postcard"
roots = ["Message", "Request", "Reply", "ErrorCode", "LiveDetail", "JobSpec", "JobOutput"]
json_roots = ["JsonOnLink<Vec<PopoverState>>", "JsonOnLink<serde_json::Value>"]   # seeded by the test below
constant = { file = "crates/dyno-core/src/protocol.rs", name = "PROTOCOL_VERSION" }
tags = "dyno-v*"
dir = "wire-schema/app-agent"             # schema.json, samples.json, samples.postcard.hex, notes/

[[protocol]]
id = "agent-helper"                       # the agent <-> the privileged helper (installed separately, Dyno ADR 0027)
format = "postcard"
roots = ["HelperRequest", "HelperReply", "HelperMessage"]
constant = { file = "crates/dyno-core/src/helper_wire.rs", name = "HELPER_API" }
tags = "dyno-v*"
dir = "wire-schema/agent-helper"

[[protocol]]
id = "host-plugin"                        # the agent <-> sensor plugins
format = "postcard"
roots = ["PluginMessage", "PluginRequest", "PluginReply"]
constant = { file = "crates/dyno-plugin-api/src/protocol.rs", name = "WIRE_MAJOR" }
tags = "dyno-v*"
dir = "wire-schema/host-plugin"
```

**The schema: postcard-schema, not serde-reflection (decided).** Dyno's wire types are generic: `Reading<T>`
(`dyno-core/src/metrics.rs`) is used with `bool`, `f64`, `u32`, `f32`, `Vec<String>`, `Vec<Temperature>` and `Vec<Fan>`;
`Measured<T>`, `Total<T>` and `ListOrReason<T>` (`process.rs`) likewise. serde-reflection records containers by their
serde name, which erases type parameters, so these instantiations collide under one name. Three ways out were weighed:
- *monomorphic newtypes per instantiation*: changes dozens of domain types for a test's sake; rejected;
- *golden encodings only*: they prove that old bytes still decode, but not that a new field was appended rather than
  inserted where no sample reaches; kept as the second check, not the only one;
- **[postcard-schema](https://crates.io/crates/postcard-schema)** (MIT OR Apache-2.0, 0.2.5 of 2025-08-23, from the
  postcard repository; checked on crates.io and docs.rs on 2026-11-04): `Schema` has an associated
  `const SCHEMA: &'static NamedType` per type, so every instantiation of a generic gets its own structural schema, in
  postcard's own data model. It needs `#[derive(Schema)]` on every type reachable from a postcard root (about 515
  `Serialize` derives in `dyno-core` and 34 in `paddock-widgets` on `land-f`; most are reachable), behind a `schema`
  feature in shared crates, and hand-written impls for types with custom serialisation (`JsonOnLink<T>`, `Timestamp`).
  The derives are mechanical and cheap next to the alternatives; chosen.

A test in each owning crate builds the `OwnedNamedType` of every root and writes it, as pretty JSON in a stable order, to
`<dir>/schema.json`; it fails when the checked-in file differs ("run `./dev wire sync`").

**JSON payloads inside the binary wire.** `JsonOnLink<T>` (`dyno-core/src/widgets/wire.rs`) travels as one JSON string on
the postcard link, so the postcard schema sees a string. Its hand-written `Schema` impl names its `T` in that node; the
test lists every such node and fails unless it is in `json_roots`. Each JSON root is checked like a `json` protocol
(below), in human-readable mode.

**The serde lint.** postcard cannot carry `#[serde(tag = "...")]`, `untagged`, `flatten` or `skip_serializing_if`, and the
derive does not see serde attributes. C80 reads the type names in `schema.json` and refuses those four attributes on any of
them in the source (JSON roots are exempt: `PopoverSurfaceValues` has `#[serde(flatten)]` at
`widgets/popover_values/mod.rs:182` and travels inside `JsonOnLink`).

**Golden encodings.** `./dev wire sync` also writes the round-trip samples (the `*_samples()` functions of the guard,
moved to `paddock-testkit::wire` for the shared part) as `<dir>/samples.json` (human-readable) and, for postcard, each
sample's bytes as one hex line in `<dir>/samples.postcard.hex`. They are in every tag's tree. `./dev wire check` extracts
the last tag's goldens to a temporary folder and runs the crate's golden test against them (path passed as an explicit
test input): every old postcard sample must decode with the new types and re-encode to the same bytes; every old JSON
sample must decode.

**Breaking is decided by the code, never by the note.**
- `postcard`: the schema diff against the last tag. The only additive change is a variant appended at the end of an enum;
  anything else (a field added, removed, reordered or retyped; a variant inserted or removed) is breaking. The goldens
  must pass either way.
- `json` (Pitcrew's protocol, Debrief's `debrief-ai` worker, the `json_roots`): breaking when an old golden no longer
  decodes; an added field with a serde default, or a new variant, decodes and is additive.
- Debrief's store-client wire (`WIRE_VERSION` 3) is positional by the row order of its operation table
  (`debrief-store-client/src/port_table.rs`, `wire_names`): WP-4 writes a manual emitter that lists the rows in order with
  their DTO types as `schema.json`; appended rows are additive, anything else breaking; the DTOs get JSON goldens.

**The version.** `released` is the constant in the tree of the **last tag**: the highest semver among the tags matching
`tags`, reachable from `HEAD` or not. `version = released + 1` when the code is breaking against that tag, else
`released`. **No matching tag**, or a tag whose tree has no `<dir>` yet: nothing is bumped and no diff is required; the
first tag with a schema sets the baseline. `./dev wire sync` writes the schema, the goldens and the constant; two branches
that both need the bump write the same line, which git merges cleanly. The integrator runs `./dev wire sync --check` at
landing.

**What a bump means per wire.**
- `app-agent`: app, command and agent ship in one installer, so a bump costs one "update the older one" refusal between a
  new app and an old running agent, once per release line.
- `agent-helper`: the helper is installed separately (Dyno ADR 0027). A `HELPER_API` bump makes the agent refuse every
  installed helper with "update the helper", so it needs a helper release in the same app release and
  `MIN_HELPER_VERSION` (`helper_wire.rs`, `dyno-helper-service`) raised to it; the release checklist names both. The
  frame major `WIRE_MAJOR` of that link (1) stays for frame-level changes only.
- `host-plugin`: a bump refuses every plugin built for the old major; plugins are rebuilt and shipped with the release.

**Notes stay, for the reader.** A change of a schema adds one note (`<dir>/notes/<yyyy-mm-dd>-<slug>.toml`: `summary`,
`changed`, `added`); its `kind` must agree with the code or C80 fails. Notes are compared **by path**: a note absent from
the last tag's tree is new; a note present in it is shipped and **immutable** (C80 fails on its edit or removal). Every
`added` entry appears as a sample. `./dev wire log` prints the history by version; the comment above each constant is one
line pointing at its folder.

**Two unreleased wires with one number.** Between tags two builds can both say version 4 with different schemas. In
non-release builds each side puts the low 16 bits of the SHA-256 of its `schema.json` (embedded with `include_bytes!`) in
the existing `minor` of `paddock_ipc::ProtocolVersion`; release builds keep `minor` 0. `ProtocolVersion::accepts` on
`land-f` takes any child minor up to the host's (`frame.rs:74`), so it would not refuse a different hash: in non-release
builds the app's and the agent's link code compare the minors for equality right after the handshake and refuse a
mismatch with "rebuild both: the wire schemas differ". No new field in the shared handshake; if the peer's version is not
yet readable after `connect`, a read-only accessor is the one change to `paddock-ipc` (WP-4).

**CI fetches tags.** C80 needs the tags and their trees: every job that runs it checks out with `fetch-tags: true` (§6).

**Which parts run where.** C80's note, immutability, constant and serde-lint parts need no build and run in the push
check. The schema and golden tests need a build and run in `./dev check --pre-push` and CI.

### 5. Additive files: union where a blind merge is safe, conflicts kept where it is not: C82

- **`merge=union` for the ADR indexes, `docs/lessons.md`, the `performance.md` files and `docs/benchmarks.md`.** These only
  grow by rows or sections whose position does not change their meaning. A synthetic test (git 2.56.0) shows the driver
  keeps both sides' rows and sections, and also what it gets wrong silently: two rows with the same number, both versions
  of a row both sides edited, and a missing blank line between appended sections.
- **CHANGELOGs stay off union** (chosen over a placement rule). A second synthetic test shows union moving a branch's new
  bullet under a `## [1.1.0]` heading that `main` inserted below `## [Unreleased]`, with no conflict. A wrong section in a
  changelog is a wrong release note, and a rule "lines added since the last tag sit only under Unreleased" would need the
  tags in every check and an exception for the release commit itself. The 13 changelog conflicts (of 35) stay visible and
  are resolved by hand; per-entry fragments are the fallback if WP-7 shows they still cost a landing (D5).
- **C82** (`paddock-conformance logs`) checks every union file: index rows unique by number, sorted, one per ADR file and
  pointing at an existing file; no heading twice in a lessons file; no exact repeated line within a section, and a
  **near-duplicate warning** for two lines of at least 20 characters in one section whose edit distance is at most 10 %
  of the longer line's length, which is what a row edited on both sides leaves; a blank line before every heading. `paddock-conformance logs --fix` sorts rows and
  restores blank lines; duplicates are left to the author. WP-3's fixtures include each union blind spot and the
  changelog case (to prove the CHANGELOGs are not union files).
- **`./dev adr next [--app <app>]`** prints the next free ADR number over `origin/main` and every remote branch.
- Assumption: the steward merges locally (log 722: "steward merging locally"). Gitea's server-side merge button may not
  apply the repository's `.gitattributes` merge drivers (not checked); landings keep merging locally.

### 6. The push check on every branch push: `push-check.yml`

A new workflow, `.gitea/workflows/push-check.yml`, on `push` with `branches-ignore: [main, 'wip/**']`, one job in the
`paddock-ci` container, with no build and no tests:
- checkout as a partial clone with tags (`filter: blob:none`, `fetch-depth: 0`, `fetch-tags: true`);
- `commit-lint` over the pushed range (the commits not on `origin/main`), the code of the `commits` job;
- formatting: for each changed `.rs` file, its owning package from `cargo metadata --no-deps` of the owning workspace
  (no dependency resolution), then `cargo fmt --check -p <package>` in that workspace. That uses each workspace's own
  edition (2024 for `crates/`, `tools/paddock-dev`, the demos and the fuzz crates; 11 manifests on `main`) and its own
  `rustfmt.toml`. A `.rs` file no workspace package owns (a script, a spike) is checked with `rustfmt --check --edition
  <edition>` from the nearest `Cargo.toml` above it (2021 when there is none) and the nearest `rustfmt.toml`. The toolchain
  is the one in the `paddock-ci` image, the same as the Linux fmt job's; its version is printed in the job summary and
  recorded with the measured run;
- **every conformance check that needs no build**, for the apps and shared paths the range touches: WP-1 adds a
  `needs: "none" | "build" | "bundle"` field to each check's metadata, and the push check runs the `none` ones (C24, the
  image review rows, among them), plus C72, C79, C80's parts that need no build (notes, immutability, the constant, the serde lint; the schema and
  golden tests need a build and run in `--pre-push` and CI), C81 and C82.

It runs on its own runner label (`push-check`, one slot on the Linux host) so it never queues behind or ahead of the gate
runs, and cancels an older run of the same branch. It writes a one-line job summary per check and prints an `::error::`
with the cause and the next step (ER-27). Local hooks and `./dev check --pre-push` stay the first line; this job is the
backstop for cherry-picks, uninstalled hooks and other machines.

### 7. The drift check: C81 and `./dev drift`

`./dev drift [--branch <b>] [--json]` compares a branch head with `origin/main`: **behind** (non-merge commits on `main`
since the merge base, and hours since the branch last merged `main`), **unlanded** (non-merge commits not on `main`, and the
age of the oldest) and **overlap** (files changed on both sides since the merge base, hot files marked). Thresholds are
data in `docs/harnesses.toml` `[drift]`: **warn** when behind is over 30 commits or 24 hours, unlanded is over the line caps,
or a hot file overlaps; **fail** when unlanded is over twice the line caps (50 commits or 96 hours) or `main` was last
merged more than 72 hours ago. C81 is a thin conformance entry that runs `./dev drift --json` (one implementation, rule 55).
It runs in the push check, in `--pre-push` and in the steward's landing step.

**Amendment to ADR 0042 §3:** "a feature branch's own pushes are not landings and are not checked against the caps"
becomes "a feature branch's pushes are checked by C81 against the line caps; the batch caps (C73) still apply only at
landing."

### Where the checks live

C79, C82 and the `needs` field are Node checks in `conformance/src/checks/` (where every check lives until ADR 0022's
conformance wave ports them to Rust). `./dev drift`, `./dev wire sync|check|log`, `./dev gen switches` and `./dev adr next`
are Rust in `tools/paddock-dev`; C80 and C81 are thin Node entries that call `./dev wire check --json` and `./dev drift
--json`, so each mechanism has one implementation.

## How this fits the rules

- **Rule 50 (CI economy), amendment proposed.** The push check runs on every branch push, which (a) and (b) would forbid
  as written. Proposed (f): "The push check (`push-check.yml`: commit messages, formatting of changed packages and the
  conformance checks that need no build; no build, no tests, on its own runner slot) runs on every branch push except
  `wip/**` and is not a CI run under (a) to (c)." **Before (f) takes effect, one run is measured** (container start,
  partial clone, each step) and recorded (rule 48); the budget is set from it, with 2 minutes as the target. The
  `.gitattributes` change marks everything in the affected plan, so WP-3 costs one full run, once. Smaller landings mean
  more main runs; ADR 0042's cadence bounds them at three a day, and the draft ADR 0047 would let a **fast-forward** landing
  reuse the branch's green runs (a merge commit still runs in full).
- **Rule 54 (release path).** A part that touches the release path still dispatches `<app>-release.yml` with gates and
  bundles before it lands. `switches.toml` is on the release path, so a change of a switch's `state` builds the installers
  before it lands.
- **Rule 55 (one mechanism).** Switches are one mechanism in `paddock-build`, wire schemas and versions one in `./dev`
  with C80 as a caller, drift one in `./dev`, the round-trip helper one in `paddock-testkit`. New copies would be listed in
  `docs/known-copies.toml` like any other (C58 checks that register; it does not find copies by itself).
- **ER-1 / rule 1.** §3's freeze: during an app's release only that release's parts land for the app; other apps' lines
  land only where they leave the release's shared code alone.
- **ER-8 / rule 3.** The full suite runs once on the combined result with the shipped switches before a local build or
  release; the nightly adds the all-on leg (§1).

**Proposed rule 65** (the next number after those named by ADRs 0041, 0042 and the drafts 0045 to 0048, up to 64;
renumbered at acceptance if they take fewer): "Small merges behind switches. Unfinished work lands in reviewed parts behind
a declared switch (`apps/<app>/switches.toml`, default off, a removal tag or date, injected into `compose`, no override in
release builds); a line holds at most 25 commits and 48 hours unlanded; during an app's release freeze only that release's
parts land for it. A wire version is derived from its schema and goldens against the last tag, never chosen by hand; a wire
change adds its note and its round-trip samples. The ADR indexes, lessons and benchmark logs merge with `merge=union` and
are checked by C82; changelogs do not. Commit messages, formatting and the conformance checks that need no build run on
every push. Checked by C79 to C82. *Why:* the Dyno 1.1 landings found five independent "4:" protocol notes, 28 of 35
conflicts in append-only files, and message, review-row and formatting failures found only on the landing branch (ADR
0049)."

Proposed one-clause amendment to rule 61: "a line lands its parts behind a switch (rule 65) instead of holding them for
its end."

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C79** switches | every switch, its uses, its `remove_by`; tags against reached deadlines | nothing yet; the ad-hoc "disk test off until WP-B3" in `e5acb772b` becomes a switch |
| **C80** wire | per wire: the postcard schema against the last tag's, the last tag's goldens decoded, the constant against the derived version, notes (new, immutable, kind agrees), samples, the serde lint over the schema's types | the Dyno lines above (no notes; two at version 3 with new fields); `land-f`'s hand-merged history |
| **C81** drift | each branch head against `origin/main` | warns on all seven lines in the drift table (hot files overlap); **fails** on `feat/dyno-network-features` (52 unlanded, over 50) |
| **C82** logs | every row, heading and line of the union files | the duplicate ADR 0032 row (`f41dd6157`), the order (`cbe1e6bc1`) |
| push check | every branch push | `land-bc`'s over-long subjects and C24 rows, `bench_units.rs` formatting, at push time |

## Work packages (agent-days, estimates)

| WP | What | Days |
|---|---|---|
| WP-1 | `push-check.yml` (partial clone with tags, own runner label, commit-lint, per-package `cargo fmt --check`, summary, `::error::`), the `needs` field on every check, its ci-actions test; one measured run; rule 50 (f) text | 1.5 |
| WP-2 | `./dev drift`, `[drift]` and `[line]` in `docs/harnesses.toml`, C81 with the freeze rules (`Freeze:`, `Freeze-closed:`, `Release-part:`, warn-only during a freeze), fixtures; the ADR 0042 §3 amendment text | 1.25 |
| WP-3 | `merge=union` lines in `.gitattributes`; C82 with fixtures for each union blind spot, the near-duplicate warning and the changelog case; `logs --fix`; `./dev adr next` | 1.5 |
| WP-4 | `wire.toml` with Dyno's three wires, Pitcrew's and Debrief's two; `#[derive(Schema)]` on every type reachable from a postcard root (about 550 in `dyno-core` and `paddock-widgets`, a `schema` feature in shared crates), hand impls for `JsonOnLink<T>` and `Timestamp`; the schema tests; golden samples and postcard bytes with the golden test; the JSON rules and `json_roots`; Debrief's store-client emitter; the serde lint; `./dev wire sync`, `--check`, `log`; notes for the 1.1 changes; C80; the non-release minor hash and the equality check in the link code (and a read-only `paddock-ipc` accessor if needed); `paddock-testkit::wire` | 4.5 |
| WP-5 | `paddock-build::Build::switches` (parser, `Id`, `Switches`, `SHIPPED`, `current()` with the debug `OnceLock`), `compose` taking the table in every app, the backend `switches()` command and the mock's table, `./dev gen switches` with its test, `--switches` on the composing binaries, ADR 0039's probe, C79 with tag refusal, the nightly all-on leg | 4.5 |
| WP-6 | rule 65, the rule 61 clause, the freeze in the `integrate` skill and AGENTS.md "Working in the monorepo", the landing step (`wire sync --check`, `drift`, `logs`) | 0.5 |
| WP-7 | after two weeks: push-check minutes per day, conflicts per merge, CI runs per landing, **reviews per landing**, unlanded age; recorded in `docs/benchmarks.md` (rule 48); tune caps and thresholds; decide on changelog fragments | 0.5 |
| **Total (this ADR)** | | **14.25** |
| WP-8 (follow-up) | Additive registration for Dyno's hot files: per-feature modules for the API types and the three backends, the command and handler lists generated from per-feature files; as an amendment to ADR 0037. Owner: a designer the coordinator assigns; the amendment is due 2026-11-21, after the Dyno 1.1.0 tag. Estimate to be made from that amendment's work packages (memory note "estimate after design") | (open) |

Order: WP-1 and WP-3 first (no product code), then WP-4 before the next wire change, WP-2, WP-5 after the Dyno 1.1.0 tag,
WP-6, WP-7. WP-1, WP-2 and WP-4's `./dev` parts are CI kit and `paddock-dev` harness work (ADR 0042): made on those homes
and landed outside the Dyno release window (ADR 0022 G.7, on `origin/docs/adr-0022-generic-tools`). WP-5 changes
`crates/paddock-build` and every app's `compose`: after the current release, all apps in one change (rules 39, 42).

## Parallelism and cost (rule 51)

The push check is one short job: git queries on a partial clone, rustfmt on a few packages, TOML and Markdown parsing. It
parallelises nothing; its cost is the container start and the clone. On 11-02 and 11-03 the remote branches gained 277
non-merge commits across 52 heads, so it runs tens of times a day; the first measured run (WP-1) sets the budget. What it
saves: a red landing run is about 1 to 1.5 hours of the shared runners (log 727; memory `ci-economy`). The schema and golden
tests run with the crate's other tests, with no extra build (their time is measured in WP-7).

## Consequences

- Feature work reaches `main` daily; conflicts arrive one part at a time, and e2e sees the combined menus and labels a day
  earlier.
- Dormant code ships while a switch is off: unregistered, checked by C79 and ADR 0039's probe, with a deadline. Dormant
  wire fields bump the protocol (§3).
- No branch writes a combined protocol note again; a breaking change is found from the code, not from a note.
- Union merges never stop a merge, so a semantic clash in an index or the lessons is found only by C82; WP-3 lands both
  together. Changelog conflicts remain.
- During a release freeze the app's other lines wait, as they effectively did this week.
- One more Linux job per push on its own slot, and two more files per app (`switches.toml`, `wire.toml`).

## Alternatives considered

- **Runtime feature flags (a hidden setting or a remote list).** They can be flipped without a build, but they are user
  state, a support surface, and a released build would not be fixed by its tag. Rejected before v1.
- **Cargo features as switches.** They change the dependency graph per build and multiply the clippy and test matrix.
  Rejected.
- **A compiled TS constant per switch.** The debug override could not reach it; the backend port carries the table instead.
- **Bump `PROTOCOL_VERSION` only by the integrator at landing.** One person reads every branch's notes, and a branch build
  carries the old number with a breaking change. Rejected (D1).
- **Wire change detected by a list of type files.** It misses most wire types (16 imported modules, types in subfolders)
  and trusts the note's `kind`. Replaced by the schema.
- **serde-reflection for the schema** (revision 2). It records containers by serde name and so erases type parameters:
  `Reading<T>`, `Measured<T>`, `Total<T>` and `ListOrReason<T>` collide. Rejected for postcard-schema (§4).
- **Monomorphic newtypes per generic instantiation.** They would make serde-reflection work, at the cost of dozens of
  domain types changed for a test. Rejected.
- **A new handshake field for the schema hash.** It changes the shared `paddock-ipc` frame for a debug-only need; the
  existing `minor` carries it in non-release builds. Rejected.
- **`merge=union` for the CHANGELOGs plus a placement rule.** Rejected: it needs the tags in every check and still misplaces
  silently between checks (§5).
- **Changelog and lessons fragments.** No conflicts at all, but an assembler, broken lessons anchors, and a change to what
  the first-release step reads. Kept as the fallback for changelogs (D5).
- **A Gitea server-side `pre-receive` hook.** Gitea disables custom git hooks by default (`DISABLE_GIT_HOOKS = true`)
  because they run arbitrary code on the host ([config cheat sheet](https://docs.gitea.com/administration/config-cheat-sheet),
  read 2026-11-04), and rustfmt would need a toolchain there. Rejected.
- **Only local hooks.** Commits bypass them (cherry-picks, commits before installation). Not enough alone.
- **Long-lived integration branches per release line.** They move the drift, they do not remove it. Rejected.

## Decisions

Accepted by the user on 2026-11-04 (revision 1), adjusted by revision 2 as marked:
- **D1:** the wire version is derived, on any branch, by `./dev wire sync`. *Adjusted:* derived from the code (the
  postcard schema and the last tag's goldens), not from the notes' `kind`; notes stay required; one derivation per wire
  (app-agent, agent-helper, host-plugin for Dyno).
- **D2:** the push check on every branch push and rule 50 (f). *Adjusted:* not `wip/**`, its own runner slot, and the
  budget set from one measured run before (f) takes effect.
- **D3:** switches fixed in release builds, `remove_by` at most the next minor tag or 30 days. *Adjusted:* a generated
  table injected into `compose`, a debug-only override, the UI reads the table through the backend.
- **D4:** line caps of 25 commits and 48 hours; warn at 30 commits, 24 hours or a hot file; fail at twice the caps or 72
  hours without a `main` merge (unchanged; this amends ADR 0042 §3).
- **D5:** union plus C82 now, fragments only if WP-7 shows it fails. *Adjusted:* CHANGELOGs are not union files.
- **D6:** squash landing copies end once the push check is in (ADR 0042 §4).

Accepted by the user on 2026-11-04 (revision 3):
- **D7:** the release freeze of §3: from `Freeze: <app> <version>` until the tag, only that release's parts land for the
  app, enforced at landing by C81 with `Release-part:`; dormant wire changes bump the shipped protocol (postcard encodes
  a field behind an off switch anyway). The alternative, unfinished parts of the same app landing during its release,
  would put untested dormant code and possibly a protocol bump into the release tree after its gates ran.

## Rollback

Switch files, caps and thresholds are data; C79 to C82 can be warnings by one line each; the push check is one workflow
file; the union lines are removed from `.gitattributes`.
