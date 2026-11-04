# 52. Wired features, visible outcomes and real layouts: every feature reaches the real engine, every job shows how it ended, every control stays in its box

- Status: Proposed (2026-11-04), revision 2. Written on the user's go-ahead of 2026-11-04 (ER-21 redesign after the
  Dyno 1.1.0 release-candidate findings on the user's Windows PC). The user accepted D1 to D6 as recommended in
  revision 1 (2026-11-04). The independent review (round 1, on `e494b2f78`) asked for another round; revision 2 answers
  it. Its fixes change the details of **D2 and D3**, which go back to the user, and add **D7**. Docs only: nothing here
  is built yet.
- Date: 2026-11-04
- Related: ER-14 / rule 7 (tests never touch the user's machine), ER-17 to ER-19 / rules 10 to 12 (long work off the UI
  thread, live progress, no UI timeouts), ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42
  and 43 (share first; shared behaviour tested once; one real-app smoke per release), ER-23 / rule 22 (screenshots get an
  independent review), ER-27 (logs explain every failure; rule 56 on `origin/ci/publish-tools`), rule 47 (Material
  first), rule 50 (CI economy), rule 54 (the release path builds installers; (c) proposed by ADR 0040), rule 55 / ADR 0034
  (one implementation per mechanism). ADR 0008 (long operations, `paddock-jobs`, `pd-job`), ADR 0010 (notifications),
  ADR 0016 and ADR 0043 (dialog sizing at several scales), ADR 0018 (virtualised lists), ADR 0021 (scaled fixtures),
  ADR 0026 (dialog contracts), ADR 0027 (`pd-table`), **ADR 0037 (the app composition contract; §1 extends it)**,
  ADR 0039 (release gates inspect the artifacts), ADR 0042 (one owner per harness; its §1 release-window hold on
  `crates/` and `ui/` harness homes decides what can land before Spotter), ADR 0040 (the Windows GUI test session), ADR 0041 (deterministic timing:
  the test clock that drives §3's live updates), ADR 0049 (`wire.toml`, the per-app wire register §1 reads; the no-build
  push check that runs §1 to §3's source checks), ADR 0050 on `origin/ci/per-crate-coverage` (its `[[exclude]]`
  entries name the `job` that covers a file unit tests cannot run; for platform code that job is a `features.toml` VM
  step of §1.4), ADR 0051 on `origin/docs/paddock-tools-design` (one manager for external
  tools; it fixes the smartmontools instances B1 and B2 below **for tools**; this ADR closes the class for every wire,
  port and feature and does not repeat 0051's design). Drafts on `origin/docs/redesign-adrs-7-9`: 0046 (every check proves
  it can fail: each check here ships with a failing fixture), 0048 (test in the user's launch context: §1.4's VM tests run
  in those contexts once 0048 lands).
- Numbers (scanned with `git ls-tree` and `git grep` over every remote branch after `git fetch`, 2026-11-04):
  - **ADR 0052.** `origin/main` has up to 0049; 0045 to 0048 are on `origin/docs/redesign-adrs-7-9`, 0050 on
    `origin/ci/per-crate-coverage`, 0051 on `origin/docs/paddock-tools-design` (838aad1ea).
  - **Checks C83 to C90.** The highest check number named on any branch is C82 (ADR 0049); C74 to C76 are named in the
    0045 to 0048 drafts, C77 is the "unknown check" fixture, C78 is the dialog-sizing check.
  - **Rule 67.** Rules 56 to 59 are on branches, 60 and 61 are proposed by ADRs 0041 and 0042, 64 by the 0045 draft, 65 by
    ADR 0049, 66 on `origin/docs/rules-2026-11-04`. Note for those drafts: 0046 and 0048 still name rules 65 and 66
    provisionally, and both numbers are now taken; they need new numbers (68 and up) when they are written out.

## Evidence

Read in git on 2026-11-04 at the commits named. "Verified" means read in the code at that commit; "measured" means a
`git grep` count whose command is given; "in progress" means read in an agent's uncommitted worktree, not yet in git.

### Pattern 1: features built but never wired end to end

| Instance | Source | How checked |
|---|---|---|
| `HelperLink::smart` sends `HelperRequest::Smart` but has no caller outside its own `#[cfg(test)]` module (line 229 on) | `apps/dyno/crates/dyno-agent/src/helper_link.rs:171` at `origin/steward/dyno-1.1.0` (1589c46bb); callers at lines 340 and 351 are tests | verified |
| The helper answers `Info` with an empty tool list for every caller: `CallerView { .., tools: vec![] }` | `apps/dyno/crates/dyno-helper/src/server.rs:88`; returned by `HelperRequest::Info => Ok(HelperReply::About(self.info.clone(), ctx.view.clone()))`, `dispatch.rs:200` | verified |
| Effect on the user's PC: Settings showed smartmontools "Not installed" right after a successful install through the helper, and the Lifespan page never read SMART through the helper's `smartctl` | user report 2026-11-04; the fix is in progress on `fix/dyno-windows-smart` (22 files, +737/−90, uncommitted when read) | user report; in progress |
| The same shape, wider: one more helper sender with no production caller, `HelperLink::log_tail` (line 198). (`HelperLink::firewall`, line 163, has none either, but `HelperRequest::Firewall` is also sent by the `FirewallReader` impl for `HelperLink` at line 223, so that kind is reachable through a trait object; corrected in revision 2.) Eight of the helper's 19 request kinds (`Processes`, `Traffic`, `SocketOwners`, `FileHolders`, `Counters`, `MemoryModules`, `Extras`, `WolState`) are built by no agent code outside tests, only by the helper's own authorisation table and its tests | `git grep -nw <name>` over `apps/dyno/crates/*/src` and `apps/dyno/app/src-tauri/src` at 1589c46bb, excluding `tests` paths and the helper's own crates | measured (grep level, not a call graph). Some may be deliberately later (Dyno ADR 0027 scopes the helper's 1.1.0 reads); nothing in the code says which, and that is the gap |
| Earlier: three Dyno 1.0 tray and Settings features complete in the browser build and unwired in the app (popover spec, dialog contract, `BroadcastChannel`) | lessons.md "What the browser e2e cannot see" (2026-11-01); ADR 0037 | read |
| Earlier: a setting nothing in the product filled in (`utcOffsetMinutes`, default 0) made every "local" day a UTC day | lessons.md, 2026-10-30 (Dyno) | read |

ADR 0037 made unmanaged state, undeclared dialog commands and unregistered popovers detectable. It does not see a request
kind that the app never sends, a port method nobody calls, or a reply that is always empty: those compile, pass every
unit test (each side is tested against a fake of the other) and pass the browser e2e (the mock backend answers what the
real engine never would).

### Pattern 2: silent failures, and stale state shown as current

| Instance | Source | How checked |
|---|---|---|
| The tool install job's result is read only for `null` (cancelled); a done job's typed output is dropped and the card reloads, so "installed, but the helper does not list it" became a silent "Not installed" | `apps/dyno/app/src/app/state/tool.store.ts:43-56` at 1589c46bb (`const out = await run.result; if (out === null) …` then `await load()` in `finally`) | verified |
| Four more stores narrow a job's output with `'x' in out` and have no final `else`, so an output of another kind is ignored without a word: `backup.store.ts:66-67` (`if (out && 'backup' in out)`); `extras.store.ts:178-181`, the finder path (`null` → cancelled, `else if ('finder' in out)`, nothing after; the path at 148-149 maps through `pick` instead); `helper.store.ts:58-60` (`null` → cancelled, `else if ('helper' in out)`); `wol.store.ts:121-122` (`if (out && 'wol' in out)`, so a cancelled run and an output of another kind both fall through silently) | at 1589c46bb | verified |
| A cancelled job and a page closed while its job ran both arrive as `null` (`JobFollow.follow` resolves `null` when the owner is destroyed), and `tool.store.ts` and `helper.store.ts` then show "cancelled" | `ui/projects/paddock-ui/shell/jobs/src/job-follow.ts:14-33` (origin/main); Dyno `core/job-starter.ts` | verified |
| `JobState::Done` carries no outcome at all; "done" cannot say "done, but I could not confirm it" | `crates/paddock-jobs/src/model.rs:99-112` (origin/main) | verified |
| Benchmarks showed a stored Dyno 1.0 run's "runner missing; reinstall Dyno" in the present tense next to a new run that works | fixed by `33682d905` on `fix/dyno-bench-stale-error` ("a past benchmark failure reads in the past") | verified (commit) |
| `smartctl -a` failures per drive are dropped: `let Ok(out) = self.run(program, &["-a", "-j", &d]) else { continue };` | `apps/dyno/crates/dyno-platform/src/hardware.rs:263` at 1589c46bb; that file has no `log::` line, and 107 of `dyno-platform`'s 116 source files have none (the crate depends on `log`) | verified; measured |
| Footprint of the "drop the error" shape | at 1589c46bb, Dyno's non-test Rust has 111 `let Ok(..)`/`let Some(..) = .. else { continue \| break \| return }` sites, 10 of them `let Ok(..) else { continue }`; `origin/main` has 268 `let Ok(..) = .. else {` sites in non-test Rust across the family | measured (`git grep -nE`, `tests` paths excluded); not every site is a defect |
| Earlier: "endless silent waits" were one of five rounds of agent-link fixes | lessons.md "Five fixes in two days…" (2026-10-30) | read |

### Pattern 3: layout bugs seen only on the real Windows machine

| Instance | Source | How checked |
|---|---|---|
| Settings › AI access: checkboxes clipped at the content's left edge. The shared rule pulls every inline checkbox box left by half its touch-target overhang, the MCP page pulled it again, so the box sat past the scroll container's edge | shared rule `ui/projects/paddock-ui/scss/_material.scss:779-781` (origin/main); fix `370eac6c6` on `fix/settings-checkbox-clip` (drops the page copies, adds an e2e assertion) | verified |
| The same class in the notification pane: `.pd-pick { margin-block: -10px; margin-inline-start: -10px; }` on top of the shared rule | `ui/projects/paddock-ui/shell/notify/src/notifications-panel.ts:111` (origin/main); fix in progress on `fix/notification-pane-layout`, which also adds a `checkboxLayoutProblems` helper to `paddock-e2e/src/layout.ts` (uncommitted when read) | verified; in progress |
| `pd-table`'s virtual scroll left a blank area when its body grew after the first measure (Dyno's Processes page, 458 rows) | fix `3c28a9371` on `fix/table-virtual-blank` and its lesson | verified |
| Footprint: negative margins in shared and app styles | 38 declarations of a negative `margin*` on origin/main, in the pathspecs `ui/projects/*.ts`, `ui/projects/*.scss`, `apps/*/app/src/*.ts`, `apps/*/app/src/*.scss`, excluding `*.spec.ts` (Debrief's timeline 9, `paddock-ui/src/lib` 4, the rest one or two per folder). Dyno's styles on its release branch are not in this count | measured (`git grep -nE 'margin(-inline-start\|-left\|-block\|-top\|-inline)?: *(calc\(.*-\|-[0-9])' origin/main -- <pathspecs>`); some are legitimate |
| Earlier: a detached chart host resized to 0 stayed at height 0 | lessons.md, 2026-11-03 (Dyno) | read |

What our UI tests use: the mock backend, Playwright's default viewport at device scale 1, and short mock lists. The
existing layout helpers (`paddock-e2e/src/layout.ts`: horizontal scroll and cut-off checks at fixed widths;
`dialog-fits.ts`: dialogs at 1, 1.25 and 1.5) and the shared screenshot check `expectScreenshotClean` (`b726d56de` on
`feat/test-timing-and-screenshot-checks`: cut text, raw identifiers, placeholders, colour-only chips) do not look for a
control clipped by an overflow ancestor, two siblings overlapping, a blank virtual area or misaligned rows, and nothing
runs them over every page.

## Context: the three patterns and their invariants

**1. Wired.** A feature has parts on several sides of a wire or port (UI, app command, agent request, helper request,
platform reader). Each part is built and unit-tested against a fake of the next. Nothing checks that the real parts are
joined in the shipped composition, so the last join can be missing while every test is green. *Invariant:* every request
kind on every declared wire, and every port method, has a production caller in the composition `run` builds; no reply is
a placeholder without a dated waiver; every user-facing feature has one test that goes through the real engine, on the
platform it depends on.

**2. Outcome.** A job's end is an event the UI may or may not handle, and a stored past result is just data. So a UI can
drop a done job's result, conflate "cancelled" with "the page closed", and show an old failure as if it were happening
now. *Invariant:* every job ends with a visible outcome (success; failure with its reason and next step; or done but not
confirmed, with why), and every stored outcome carries its time and reads in the past.

**3. Real layout.** The design system lets a control draw outside its own box (a negative margin) and lets a page restyle a
shared control, and our tests never run at the scale, data size or update rate of a real PC. *Invariant:* a control draws
inside its box, and every surface (pages, dialogs, panes, Settings, popovers, widgets) is checked for clipping, overlap,
wrapped labels, blank virtual areas and alignment at 100 % and
150 %, with large lists and live updates, and once on the Windows VM before each release.

## Decision

### 1. The wired contract (extends ADR 0037)

**1.1 Every request kind has a production sender: C83 (static).** For each wire in `apps/<app>/wire.toml` (ADR 0049 §4),
the check enumerates the variants of the request roots (`Request`, `HelperRequest`, `PluginRequest` for Dyno: 109, 19 and
2 kinds at 1589c46bb). For each kind it finds the non-test functions that construct it, and requires one of:
- the function is called from non-test code outside its own impl block; or
- the function is a method of a trait impl for a **named concrete type**, and the composition builds that concrete type
  as the trait's object. The exemption names the type, not the trait (`FirewallReader for HelperLink<_>`, built in
  Dyno's composition), so a second impl of the same trait does not lend it its reachability.

Test code is `#[cfg(test)]` modules, `tests/` folders, `*_tests.rs`, and debug-only test commands. The check is built in
Rust in `./dev check` (ADR 0022), parsing with `syn`. It is a name-level call check, not a call graph; its fix text says
so, and the run-time counts of §1.5 are the backstop for everything it cannot see (closures, macros, trait objects).
At 1589c46bb it would have failed on `Smart` and `LogTail` and on the eight kinds no agent code builds. `Firewall` passes
through its trait impl.

C83 reads `wire.toml`, which ADR 0049's WP-4 creates for Dyno's three wires and Pitcrew's and Debrief's. It therefore
lands after that WP. An app outside 0049's list (Spotter, Pitwall) gets a roots-only `[[protocol]]` entry (id, format,
`roots`, no schema directory) from WP-A1, in the format 0049 defines and coordinated with that file's owner.

A kind that is deliberately not wired yet is declared in `apps/<app>/wiring.toml`:

```toml
[[unwired]]
wire = "agent-helper"
kind = "Processes"
why = "read for other accounts' processes is scoped to 1.2 (Dyno ADR 0027, C1)"
until = "dyno-v1.2.0"        # the app's next release-line tag, or a date at most 60 days ahead
```

**Waivers are bounded** (the same rules for `[[unwired]]`, `placeholder!` in §1.3 and every baseline here):
- `until` names at most the app's next release-line tag, or a date at most 60 days after the entry's commit;
- an expired entry fails its check, and `./dev release tag` (ADR 0039's tag gate) refuses a tag while one is expired;
- moving an `until` needs a `Wiring-date: <entry> <old> -> <new> approved: <coordinator|user>` trailer, as ADR 0034
  requires for copies (`user` for a later release line);
- every CI job that runs one of these checks writes the waiver counts per app (unwired kinds, placeholders, baseline
  sizes, the nearest expiry) to its job summary (ER-27), so growth is visible.

**1.2 Every port has a production adapter and a caller.** A port trait carries the marker supertrait
`paddock_shell::Port` (zero cost; a crate that is not a shell part reaches it through `paddock-commands`). C83 applies
the sender rule to each port method. ADR 0037's `composition()` report gains, for each port the composition manages,
the adapter's concrete type name in `Place::Installed`. It fails when that adapter is a test double: a type under
`#[cfg(test)]`, or one whose name the family preset lists (`Fake*`, `Mock*`, `Null*`, `Stub*`). A port deliberately
left on a null adapter is declared in `wiring.toml` (`[[unwired]] port = "…"`), with the same bounds.

**1.3 No placeholder replies: C84.** In non-test code of the crates that serve a wire (a new `servers = [..]` key per
`[[protocol]]` in `wire.toml`) and of every port adapter:
- `todo!` and `unimplemented!` fail outside doc comments and tests;
- a struct literal of a type reachable from a reply root (the schema of ADR 0049 §4 names the types) that sets a
  **collection field** to a constant empty value (`vec![]`, `Vec::new()`, `BTreeMap::new()`, `HashMap::new()`,
  `Default::default()` of a collection type) fails, unless the value is wrapped in
  `paddock_commands::placeholder!(value, why = "…", until = "…")`. The macro expands to the value, and C84 reads its
  `until` with the bounds of §1.1.

`server.rs:88`'s `tools: vec![]` is exactly such a field. `Option` and `String` fields are **not** in scope (D2, changed
in revision 2): in non-test code of `dyno-helper/src` and `dyno-agent/src` at 1589c46bb there are 128 `: None,`, 24
`: vec![],` or `: Vec::new(),` and 20 `: String::new(),` field initialisers (`git grep -nE`, `tests` paths excluded; the
review counted 132, 25 and 21 in its own scope). Most `None`s are honest ("not measured"), so a check on them would be
mostly waivers; the collection fields are the shape of the instance and few enough to review one by one.

Existing sites go into a shrink-only baseline keyed by the hash of the line's text, as C71 does (lesson "A rule that
counts sites needs a baseline by text, not by count", 2026-11-04, on `feat/test-timing-and-screenshot-checks`,
`b726d56de`). The baselines of C84 (and of C87 and C88) are cut **only after WP-0's fixes have landed**, so that the instances this ADR
starts from are not written into a baseline as accepted.

**1.4 One real-engine end-to-end test per user-facing feature: C85.** Each app keeps `apps/<app>/features.toml`:

```toml
[[feature]]
id = "lifespan.ssd-wear"
title = "SSD wear on the Lifespan page"
surfaces = ["route:/lifespan", "job:InstallTool", "setting:tools"]
wires = ["app-agent:Hardware", "agent-helper:Smart", "agent-helper:InstallTool", "agent-helper:Info"]
platforms = ["macos", "windows", "linux"]
tests = { tauri = "e2e-tauri/lifespan.e2e.ts#reads wear through the helper", vm = ["windows:helper-smart", "macos:helper-smart"] }
```

C85 (source check, no build) fails when:
- a surface of §3.3's list (routes, dialogs, shell panes, Settings pages, tray popovers, widget kinds), a job kind
  (`JobSpec` variants; Dyno has 14) or a Settings section maps to no feature (or to `[[unlisted]]` with a reason);
- a request kind that is not `[[unwired]]` belongs to no feature;
- a feature names a test that does not exist (the spec title or vm-smoke step id is looked up);
- a feature that depends on a platform (its `platforms` are not all three, or it uses a privileged wire such as
  `agent-helper`) has no vm-smoke step for each OS it lists, **Windows included**.

The tests run against the real engine, never the mock: the `tauri` test in ADR 0037 §8's shared real-app kit (Linux and
Windows CI legs), the `vm` steps in `vm-smoke` (the Windows GUI session of ADR 0040; macOS through ADR 0037's `--smoke`
mode). The Dyno instance's step: install smartmontools through the helper; Settings then says "Installed" with the
helper's version, and Lifespan shows a wear reading or a named reason that came from the helper's `smartctl`. ADR 0050's
coverage exclusions for platform code name these VM steps as the `job` that covers them.

**1.5 Wire coverage at run time (the dynamic half of C83).** Each wire server keeps a counter per request kind (an array
indexed by the variant, from the schema; no user content). The counts are available **on demand**: in the reply to the
wire's own info or status request (for the helper, `HelperReply::About`; for the agent, its status reply), and in the
diagnostic bundle. At shutdown the server also logs one line, `served: kind=count …` (ER-27).

**Which runs read them.** The vm-smoke run reads the counts of every wire server after its steps, on each OS (it is the
only run with the privileged helper installed). The CI real-app legs (ADR 0037 §8, Linux and Windows) read the
app-agent and host-plugin counts. A kind that is neither `[[unwired]]` nor served at least once, on an OS where its
feature is declared, fails that run. This catches a sender behind a switch that is off (ADR 0049), a branch never taken,
and anything C83's name-level check exempted.

### 2. The outcome contract (ER-17 to ER-19, ER-27)

**2.1 The engine says how a job ended.** In `paddock-jobs` (breaking change, allowed before v1, no shim):
- `JobState::Done` becomes `Done { outcome: Outcome }` with `Outcome::Confirmed { summary }` or
  `Outcome::Unconfirmed { why, next_step }`;
- `JobState::Failed { message }` becomes `Failed { reason, next_step }`;
- `NextStep` (and `Why`) are closed enums, defined **once** for the family in `paddock-jobs` (`Retry`,
  `OpenSettings(path)`, `Repair(action)`, `UpdateHelper`, `UpdateApp`, `ShowLog`, `None { why }`, and the variants ADR
  0051 lists). ADR 0051's `ToolState::Unusable { next_step }` and `ToolState::Unconfirmed { .., why, next_step }` use this
  same type, so a tool's state and a job's end speak one vocabulary; **ADR 0051 must follow** (its §"Types" changes to
  import the type rather than define it);
- a job body returns `Result<Outcome, JobFailure>`; `Outcome` is `#[must_use]`.

**`Confirmed` needs proof.** `Outcome::Confirmed` can be built only by `Outcome::confirmed(proof: ReadBack, summary)`.
`ReadBack` has no public constructor: it comes only from `paddock_jobs::read_back(expected, observed)`, which returns
`Ok(ReadBack)` when the state the job changed was read again through the path the feature uses and agrees, and
`Err(Mismatch)` otherwise. A mismatch ends the job `Unconfirmed { why, next_step }`. For an install, that path is the
helper's `Info` for Dyno's helper tools, or ADR 0051's state report for managed tools: "smartmontools was installed, but
the helper does not list it" ends unconfirmed, never confirmed. Every job kind has one engine test in which the read-back
disagrees and the job ends `Unconfirmed`; C86 (below) enumerates the job kinds and requires that test too.

The executor logs every end with the job id, kind, phase durations, outcome kind, reason and cause chain, and no user
content (ER-27).

**2.2 A stored outcome carries its time: C90.** Every outcome the engine stores (last benchmark, last scan, last install)
is `Stamped<T> { at: UtcInstant, app_version: String, value: T }`. `wire.toml` gains a `stored_roots = [..]` key per
protocol, naming the types that are persisted and sent back later (the values that go through ADR 0049's
`stored_wire` path are among them). **C90** fails when a stored root, or a field of a reply root that the schema marks as
coming from storage, is not a `Stamped<_>`. The engine words a stored reason as a fact about that run ("the 1.0 install
had no runner"), as `33682d905` does for benchmarks.

**2.3 One shared way to show it.** `@decryp7/paddock-ui/shell/jobs` (the existing jobs UI, extended) gets:
- `JobRunner`, whose `run` resolves a `JobEnd<T>`. Dyno's `core/job-starter.ts` moves here (ER-22). `JobEnd<T>` is a
  closed union: `confirmed` (with the typed value and summary), `unconfirmed`, `failed`, `cancelled` and `detached` (the
  page closed; the job goes on, and ADR 0010's notice reports its end). `null` is no longer an answer;
- `OutcomeStore.record(key, end)`, a signal store of the latest end per job key, filled from the engine's stamped
  outcomes (not browser storage);
- `pd-outcome`, a component (Material first, rule 47). It renders a live end in the present ("Installed. smartctl 7.5
  found.") and a stamped one with a dated lead-in ("Last run 3 Nov, 14:02: failed. The 1.0 install had no runner.").
  While a newer job of the same key runs, the stamped one is labelled "Previous run" and is never shown as a current
  error. Status is never colour alone.

**2.4 Checks.**
- ESLint `paddock/job-end-handled` (type-aware, family preset): a `JobEnd<T>` (or a promise of one) must be passed to
  `OutcomeStore.record` or `pd-outcome`, or be switched exhaustively (a `default` that `satisfies never`). An `await`
  whose value is unused, compared only with `null`, or narrowed with `'x' in end` without a final `else` fails. It fails
  on `tool.store.ts:48` and on the four stores above.
- ESLint `paddock/stamped-only-in-outcome`: a template binding or interpolation whose type is `Stamped<…>` (or a field
  read from one) outside `pd-outcome` fails, so a stored value cannot be rendered without its time.
- **C86:** every job kind of every app (the `JobSpec` root in `wire.toml`) has an outcome spec through the shared
  `outcomeHarness(kind)`, which drives each of the five ends through the mock and asserts that the owning page shows a
  visible outcome (for `confirmed`, the value rather than a reload). C86 also requires the engine test of §2.1 in which the
  read-back disagrees.
- **C87 (Rust, silent drops):** in non-test code, `let Ok(..) = .. else { continue | break | return .. }`, `if let Ok(..)`
  without an `else`, and `.ok()` on a `Result` feeding `?`, `flatten` or `unwrap_or*` must log the error (a `log::` or
  `tracing::` call in that branch), carry it into the value (`Reading::unavailable(e)`, a typed error), or carry a
  `// silent: <why>` comment. Clippy's `let_underscore_must_use` joins the family preset for the `let _ =` half. C87
  starts in **report-only** mode (it prints and writes the job summary, it does not fail) and is enforced once its
  baseline is cut after WP-0. The upper bound of that baseline is 268 `let Ok(..) = .. else {` sites on origin/main.
- **Spawns log once, at the spawn point.** Every external program runs through `paddock_proc`'s run helper (rule 53
  already routes kills there). The helper logs the program, masked arguments, exit code, duration and the tail of stderr:
  **success at `debug`, failure at `info`** with the cause and next step. Clippy's `disallowed-methods` in
  `lint/clippy.toml` lists `std::process::Command::{spawn, output, status}` (and `tokio::process::Command`'s), with
  `paddock_proc`'s own module the one allowed site. That covers the 107 silent `dyno-platform` files without a log call
  in each.

### 3. The real-UI layout contract

**3.1 Design-system rules: a control draws inside its own box, and a control's label stays on one line.**
- No negative margin, negative inset or translate moves the visible part of a control (box, thumb, icon, label) outside
  its host element. The shared rule at `_material.scss:779-781` is removed. Inline forms align with Material's density
  tokens, which shrink the checkbox state layer so the overhang fits in the row's padding (D4, accepted). The touch target
  (the invisible 48 px hit area) stays; removing it would be an accessibility regression.
- A page never restyles a shared control's geometry: selectors on `.mat-mdc-*`, `.mdc-*` or `pd-*` internals that set
  margin, inset, translate, width or height live only in `ui/projects/paddock-ui/scss/`. A page restyle is what made the
  shift double.
- Shared buttons, tabs, chips and menu items are `white-space: nowrap` with a minimum inline size (a token,
  `--pd-control-min-inline`), and truncate with the full text in a tooltip when their container is narrower; a label never
  wraps to a second line.
- Widgets scroll inside (the user's Grafana-style decision): a widget whose content is taller than its frame scrolls in
  its body; it never clips.

**C88** checks the first two in `.scss` files and in component `styles` strings: a negative margin, inset or translate, or
a geometry rule on a shared control outside the shared SCSS, fails unless it carries `/* pd-layout-ok: <why> */`. Its
"page restyles a shared control" half starts in **report-only** mode; both halves are enforced once the baseline is cut
after WP-0 (the 38 negative-margin sites above are the starting point). The design guide gets the rules and their why.

**3.2 Shared layout checks in `@decryp7/paddock-e2e`.** `layoutProblems(root)` gathers facts in the page and decides in
Node (as `screenshot-check.ts` does). It fails on:
1. **Clipped.** For each visible control or text box and each ancestor whose overflow is not `visible`, or the viewport:
   - overflow past the ancestor's **start** edge (inline or block start) is always clipped, scrollable ancestor or not,
     because a scroll container cannot scroll to negative offsets;
   - overflow past its **end** edge counts when the ancestor's overflow on that axis is `hidden` or `clip`, or when the
     overflow forces a horizontal scroll the ancestor does not declare (`data-pd-scroll-x` is the declaration).
   The failing fixture is the shell demo's AI access (MCP) page as it was at `370eac6c6^` (the box past the scroll
   container's left edge); the check must fail there and pass at `370eac6c6`.
2. **Self-clipped vertically.** An element with `overflow-y: hidden` or `clip` and `scrollHeight > clientHeight + 1` that
   is not a declared line clamp (`-webkit-line-clamp`/`line-clamp` set, or `data-pd-clamp`). A widget body found this way
   fails with "widgets scroll inside".
3. **Wrapped control label.** The label of a button, tab, chip or menu item (`button`, `[role=tab]`, `mat-chip`,
   `[role=menuitem]`) takes more than one line box (its text's client rects span more than one line).
4. **Overlap.** Two in-flow siblings whose boxes intersect by more than 1 px, or a control whose corners `elementFromPoint`
   finds covered by something outside it. Exempt: elements that are `position: sticky` or `fixed` (sticky table headers,
   ADR 0027), absolute overlays marked `[data-pd-overlay]`, and any element with `data-pd-overlap-ok="<why>"`.
5. **Blank virtual area.** A `cdk-virtual-scroll-viewport` whose content is taller than itself and whose last drawn row
   ends more than one row height above its bottom edge. Checked at start, after a window resize, after a **container or
   splitter resize** (a docked pane dragged wider or taller, which does not fire a window resize), after scrolling to the
   end, and after the data grew (the `3c28a9371` spec, generalised).
6. **Rows aligned.** In repeated rows (`[role=listitem]`, `mat-row`, `.pd-form-row`), the start x of the leading control and
   of the text agree within 1 px across rows, and a checkbox's centre lies within 3 px of its first text line's centre.

`expectLayoutClean(page)` runs these together with `expectScreenshotClean` (`b726d56de`), so an app calls one function.
The page-side fact gatherer is one self-contained function, so the same code also runs through WebDriver's `execute` in
the real WebView2 (§3.4).

**3.3 Every surface, at two scales and two densities, with large lists and live updates: C89.** Each app has one tour
spec that visits every surface and calls `expectLayoutClean` at each stop. **C89** enumerates the surfaces and fails when
the tour misses one:
- the routes of the app's router;
- the dialogs of its manifest (as ADR 0037's C68 does);
- the shell panes (notifications, jobs, channels, and any pane a shell part contributes);
- every Settings page, from the settings section registry (rule 44's registry);
- every tray popover (ADR 0037 §4's `Tray` carries them);
- every widget kind of the app's catalogue (ADR 0035, ADR 0044), at its minimum and its default size.

The tour runs over this matrix:

| Axis | Values |
|---|---|
| Device scale | 1 and 1.5, with the viewport in CSS px = the physical window / scale (the default 1280 × 800 window is 853 × 533 CSS px at 150 %), plus the app's minimum window size at 1 |
| Density | the default and the compact `data-density` |
| Data | `default`; `large` (the mock's scaled fixtures, ADR 0021: at least 5 000 table rows, 500 notifications, every name at its maximum length; D6, accepted); `live` (the mock emits updates under ADR 0041's test clock: rows added at the top, values that change width, a list that grows after its first measure) |
| When | after load, after 20 live ticks, after a window resize, after a container or splitter resize |

**Rule 50 scope.** A branch runs the tour only for the surfaces its change reaches: `scripts/affected.mjs` (and its Rust
port) map changed component files to the routes and surfaces that import them, and run those stops at `default` 1 and
`large` 1.5, Chromium only. A change to shared UI (`ui/projects/paddock-ui`) runs the gallery's stops instead of every
app's tour. Nightly runs every app's full matrix. Run times are measured in WP-C3 and recorded in `docs/benchmarks.md`
before anyone quotes them (rule 48).

**3.4 A Windows VM page pass before every release.** ADR 0040's Windows GUI session already opens every dialog at 100 %
and 150 %. This adds every surface of §3.3: the real app's tour through ADR 0037 §8's WebDriver kit (WebView2 and
msedgedriver) inside the `vm-smoke` Windows GUI session, at 100 % and 150 %, with the real engine and the VM's real data.
It runs `layoutProblems` in the real WebView2 and saves one screenshot per surface and scale. A person other than the
author reviews the screenshots (ER-23 / rule 22). Rule 54 (c) (ADR 0040) gains it: a tag needs a
`--os windows --gui --pages` result with no FAIL for the tagged head and the reviewer's sign-off (D5, accepted: any
release whose diff touches UI or a shared UI package; a Rust-only patch may skip it with its reason in the tag
checklist). macOS runs the same tour in the local macOS VM through ADR 0037's `--smoke` mode, which **widens ADR 0037's
WP-8**: the `--smoke` plan gains the tour's stops and the fact gatherer, not only the dialog and tray steps. VMs stay
local (the 2026-10-31 decision); CI keeps its Windows WebDriver leg.

**3.5 Harness ownership (ADR 0042).** The new shared harness parts get entries in `docs/harnesses.toml` with their home
branch and mode: the layout checks (`ui/projects/paddock-e2e/src/layout*.ts`, `release_path = false`, home in `ui/`, so
held in a release window), the tour kit and the surface enumerator, `outcomeHarness` (`ui/projects/paddock-ui/shell/jobs`
testing), and the `vm-smoke` pages step. Each WP that adds one adds its entry in the same change.

### 4. Proposed rule 67, and ER-33

> 67. **A feature is done when it is wired, shows how it ended, and fits on a real screen.** (a) Every request kind on an
> app's declared wires and every port method has a production caller in the composition `run` builds, or a dated,
> bounded entry in `apps/<app>/wiring.toml`; no reply collection is a constant placeholder without a dated
> `placeholder!`; every user-facing feature in `apps/<app>/features.toml` has one end-to-end test against the real engine,
> and a platform feature has one on the VM for each OS it supports, Windows included. (b) Every job ends with a visible
> outcome: confirmed by a read-back, failed with its reason and next step, or done but not confirmed with why; a job's end
> is never discarded; a stored outcome carries its time and reads in the past; an error is logged or carried, never
> dropped. (c) A control draws inside its own box and its label stays on one line; pages never restyle a shared
> control's geometry; widgets scroll inside; every surface passes the shared layout checks at 100 % and 150 % with large
> lists and live updates, and on the Windows VM before every release that touches UI. Checked by C83 to C90, ESLint
> `paddock/job-end-handled` and `paddock/stamped-only-in-outcome`, and the real-engine runs (ADR 0052). *Why:* the Dyno
> 1.1.0 release candidate on the user's Windows PC showed a helper feature with no caller and an always-empty reply, a
> done job read as "Not installed", a past failure shown as current, and controls clipped by a doubled negative margin,
> all with every test green.

The general part goes to the engineering rules as **ER-33** (proposed; ER-31 and ER-32 are taken on branches of
`engineering-rules`), with the same coordinator-owned sync as the other ER rules:

> ER-33. **Wired, outcome, real screen.** A feature is done only when (a) every request and port it adds has a production
> caller and one end-to-end test through the real backend on each platform it depends on, (b) every long operation ends
> with a visible outcome (confirmed by reading the result back, failed with reason and next step, or not confirmed with
> why) and stored results show their time, and (c) its UI passes automated layout checks (nothing clipped, overlapping,
> wrapped or blank) at the scales, data sizes and update rates of real machines. Placeholders and exceptions carry an
> expiry. *Why:* features passed every mock-based test and failed on the first real PC.

## Checks that make the classes impossible or detected

| Check | Kind | Would have failed on |
|---|---|---|
| C83 static senders (concrete-type trait exemption), bounded `wiring.toml` | source check over every declared wire and port | `HelperLink::smart`, `log_tail`; the eight unbuilt helper kinds (until declared) |
| `composition()` port adapters (ADR 0037 §7 extended) | enumerating test | a port left on a fake in `Place::Installed` |
| C84 collection placeholders, bounded `placeholder!` | source check over server crates and adapters | `server.rs:88` `tools: vec![]` |
| C85 `features.toml` | source check: every surface, job kind, setting and request kind maps to a feature with a real-engine test, VM steps per OS | a SMART feature with no Windows VM step |
| Served-kind counts read by the VM run and the CI real-app legs | real-engine run | `Smart` never served on Windows |
| `Outcome` with `ReadBack` proof in `paddock-jobs`, one shared `NextStep` | type (impossible by construction) plus one mismatch test per job kind | "done" with nothing to say; "installed" without a read-back |
| ESLint `paddock/job-end-handled`, C86 | lint plus enumerating spec | `tool.store.ts:48`; the four `'x' in out` stores |
| `Stamped<T>`, C90 over `stored_roots`, ESLint `paddock/stamped-only-in-outcome`, `pd-outcome` | type, source check, lint, shared component | the benchmark's present-tense 1.0 failure |
| C87 silent drops, Clippy `let_underscore_must_use`, spawn logging and `disallowed-methods` for `Command` | source check with baseline; lints; one mechanism | `hardware.rs:263` |
| C88 negative geometry, page restyles of shared controls | source check with baseline | the MCP page's and the notification pane's second shift |
| `expectLayoutClean` (clipped with the start-edge rule, self-clipped, wrapped labels, overlap, blank virtual area, alignment), C89 tour over every surface | shared e2e checks plus enumerating check | all three layout instances (`370eac6c6^` is the clipped check's fixture) |
| Windows VM page pass, rule 54 (c) | release gate, local VM | what only WebView2 at 150 % shows |

Each new check ships with a fixture it fails on (the 0046 draft's demand): the 1589c46bb shapes above, reduced to
synthetic files, and for the clipped check the real page at `370eac6c6^`.

## Work packages and order

ADR 0022 G.7 keeps larger shared changes out of a release window; ADR 0042 §1 holds harness homes in `crates/` and `ui/`
(`paddock-e2e` among them) during a release window; rule 1 puts release-critical work first. The current order is Dyno
1.1.0 → Spotter 1.0 → Pitwall 1.1 → Pitcrew 1.1 → Debrief 1.2. Sizes are agent-days (estimates).

| WP | What | Days | Depends on | Lands |
|---|---|---|---|---|
| WP-0 | The instance fixes: `fix/dyno-windows-smart`, `fix/dyno-bench-stale-error`, `fix/settings-checkbox-clip`, `fix/notification-pane-layout`, `fix/table-virtual-blank` | (outside this ADR) | | Dyno 1.1.0 |
| WP-S | **Audit sweep** of Spotter's and Pitwall's release branches for the three patterns with this ADR's measures (senders without callers at grep level, job ends discarded or narrowed without `else`, negative margins and page restyles of shared controls, virtual lists, wrapped labels); each finding becomes a fix with a failing test | 1 | | **before Spotter 1.0** |
| WP-C1a | C88's "page restyles a shared control" half, report-only | 0.5 | | **before Spotter 1.0** |
| WP-B4a | C87, report-only | 1 | | **before Spotter 1.0** |
| WP-C2 | `layoutProblems` and `expectLayoutClean` in `paddock-e2e` (six checks, the `370eac6c6^` fixture, synthetic fixtures for the rest), harness entry | 2.5 | ADR 0042's hold on `ui/` homes, unless waived (D7) | after Spotter 1.0, or before it if D7 waives the hold |
| WP-C1b | Remove the negative-margin rule (density tokens), nowrap and minimum size for shared buttons, C88 enforced (baseline after WP-0), design-guide rules, gallery screenshots reviewed | 1.5 | WP-0 | after Spotter 1.0, before Pitcrew 1.1 |
| WP-B4b | C87 enforced (baseline after WP-0); Clippy `let_underscore_must_use` and `disallowed-methods` for `Command`; spawn logging in `paddock_proc` | 1.5 | WP-0, WP-B4a | after Spotter 1.0 |
| WP-A1 | C83 with the bounded `wiring.toml`, fixtures; roots-only `wire.toml` entries for Spotter and Pitwall; registers for Dyno, Spotter, Pitwall | 2 | ADR 0049 WP-4 | after 0049 WP-4, before Pitcrew 1.1 |
| WP-A2 | C84 (collection fields), `placeholder!`, baseline after WP-0 | 1.5 | WP-A1, WP-0 | after WP-A1 |
| WP-A3 | `Port` marker, port adapters in `composition()`, served-kind counters with on-demand read-out, the VM and CI read-back | 2 | ADR 0037 WP-2 and WP-7 | before Pitcrew 1.1 |
| WP-A4 | `features.toml` and C85 (report-only first, then enforced); Dyno's and Spotter's registers with their real-engine and VM tests | 3 | WP-A1, ADR 0037 WP-7 | enforced before Pitcrew 1.1 |
| WP-B1 | `Outcome`, `ReadBack`, `NextStep`, `Why`, `Stamped<T>` in `paddock-jobs`; executor logging; every user of `paddock_jobs` updated (on origin/main 91 non-test files in 29 crates, of 156 files with tests; plus Spotter, Pitwall and Pitcrew on their branches); one read-back-mismatch test per job kind; ADR 0051 follows | 5 | | after Spotter 1.0, before Pitcrew 1.1 |
| WP-B2 | `JobRunner` (Dyno's copy moves), `JobEnd`, `OutcomeStore`, `pd-outcome`; Dyno's stores adopt it | 2.5 | WP-B1 | with WP-B1 |
| WP-B3 | ESLint `paddock/job-end-handled`, C86 and `outcomeHarness`; specs for every job kind; harness entry | 2 | WP-B2 | with WP-B2 |
| WP-B5 | `stored_roots` in `wire.toml`, C90, ESLint `paddock/stamped-only-in-outcome` | 1 | WP-B1, ADR 0049 WP-4 | with WP-B3 |
| WP-C3 | The tour spec per app, C89 over the six surface kinds, the `large` and `live` mock profiles, the scale and density matrix, the affected-surface scoping in `affected.mjs`; run times measured and recorded; harness entry | 3 + 0.5 per app | WP-C2 | after WP-C2; each app before its next tag |
| WP-C4 | Windows VM page pass (`vm-smoke --gui --pages`, screenshots, review), rule 54 (c) text; the macOS tour in `--smoke` | 2 | ADR 0037 WP-7 (and WP-8 for macOS), ADR 0040's session, WP-C3 | first release after those land |
| Adoption | Spotter, Pitwall, Pitcrew, the two demos (registers, outcome specs, tours), about 1.25 each; Debrief when it moves to the shell | 6 | | per release |
| **Total** | | **38** (with five apps' tours: 40.5) | | |

**Before the Spotter 1.0 tag:** WP-S, WP-C1a and WP-B4a: **2.5 days**, all report-only or audit, none touching
`crates/` or a held harness home. With D7's waiver, WP-C2 joins: **5 days**. (Revision 1 claimed about 8 days for a set
that could not be built before Spotter: WP-C4 needs ADR 0037's WP-7 and WP-C3, WP-A1 needs ADR 0049's WP-4, and WP-C2
sits under ADR 0042's hold.)

**Before the Pitcrew 1.1 tag:** WP-B1 to WP-B3, WP-B5, WP-C1b, WP-B4b, WP-A1 to WP-A4 (as their dependencies land),
WP-C2 and WP-C3 with Pitcrew's tour, because Pitcrew moves onto the shell in 1.1 and runs long jobs. WP-C4 joins the
release gate of the first release after ADR 0037's WP-7 lands.

**Rollback.** Each check is its own module with its own baseline, and can be reverted alone. WP-B1 is the one breaking
change; it lands with its adoption in every app in one change (rules 39, 43), so there is no half state.

## Consequences

- **CI cost (rule 50).** C83 to C90 are source checks with no build and run in ADR 0049's no-build push check (rule 50
  (f)) and in `./dev check --pre-push`. Each declares its inputs (the wire, `wiring.toml`, `features.toml`, the styles,
  the server crates) and runs only when they changed. Their cost is **not measured yet**; WP-A1 measures C83, the
  heaviest (a `syn` parse of each app's crates), on Dyno, and records it in `docs/benchmarks.md` before it joins the
  push check. If it is too slow there, it runs in the affected plan instead. The real-engine feature tests and the
  served-kind read-back ride ADR 0037's real-app legs: Linux in the affected plan, Windows nightly and at release. The
  layout tour runs, on a branch, only the surfaces the change reaches (or the gallery for shared UI); the full matrix
  runs nightly. The Windows page pass adds to the local release VM run, not to CI (ADR 0040 estimates its GUI run at 20
  to 30 minutes; the pages are not measured yet).
- **Authoring cost.** Each app keeps two more registers (`wiring.toml`, `features.toml`); each job kind gets an outcome
  spec and a read-back-mismatch test; each feature that depends on a platform gets a VM step per OS. That cost is the
  point: nobody had to say which test proves the last join.
- **Breaking changes before v1 (no shims):** `JobState::Done` and `Failed` change shape; `NextStep` moves into
  `paddock-jobs` and ADR 0051 imports it; `JobFollow` returns `JobEnd`; Dyno's `JobStarter` moves into `paddock-ui`; the
  checkbox alignment changes for every app's forms; shared buttons stop wrapping.
- **Logs** gain a served-kinds line per wire server and one line per tool spawn (failures at `info`, successes at
  `debug`); neither carries user content (ER-27).
- **Limits.** C83 is a name-level check: a call through a closure or a macro can hide a sender, and the trait exemption
  trusts that the named type is really used; the run-time counts (§1.5) are the backstop. The layout checks see geometry,
  not meaning; the reviewed release screenshots stay.

## Alternatives considered

- **More browser e2e on the mock at default size.** That is what we have, and all three patterns passed it. Rejected as
  the answer; the mock stays for development.
- **Rely on `dead_code`.** `pub` functions in library crates are never dead to rustc, and merging the crates to make them
  private is not possible across the app, agent and helper. Rejected.
- **Coverage of an instrumented real-app run instead of per-kind counters.** Exact, but it needs instrumented builds on
  every OS and heavy runs. The counters give the per-request answer cheaply and also help a user's diagnostic bundle.
  Rejected for now.
- **C84 over `Option` and `String` fields too.** Measured at 128 and 20 sites in two Dyno crates alone, most of them
  honest; the check would be mostly waivers. Rejected (D2).
- **Pixel-diff screenshot regression in CI.** Brittle across fonts, OS builds and scales, high upkeep, and a diff does
  not say what is wrong. The fact-based checks plus a person's review of the release screenshots are used instead.
- **A toast for every job end.** A toast is gone before it is read and has no time. The outcome sits on the feature's own
  card; ADR 0010's notice covers a page that was closed.
- **Drop the checkbox touch target to stop the overhang.** An accessibility regression. Rejected.
- **A release checklist item "check the layout on Windows".** It is not measurable and gets skipped under pressure.
  Rejected in favour of the gated VM pass.
- **Fix the five instances only.** Each is fixed (WP-0); the earlier lessons show each class coming back. Rejected as
  the whole answer (ER-21).

## Decisions

Accepted by the user on 2026-11-04, as recommended in revision 1: D1 (one rule with three parts), D4 (Material density
tokens), D5 (the Windows page pass for every release that touches UI; a Rust-only patch may skip it with its reason), D6
(5 000 rows and 500 notifications; Debrief gets its own larger profile when it adopts the tour). D2 and D3 were accepted
too, but revision 2 changes their content, so they **need the user again**; D7 is new.

- **D2 (changed, needs the user):** C84 covers **collection fields only** (recommended). Revision 1 recommended every
  constant-empty `Option` or collection field. Measured in non-test code of `dyno-helper/src` and `dyno-agent/src` at
  1589c46bb: 128 `: None,`, 24 empty vecs, 20 `String::new()` (the review counted 132, 25 and 21 in its scope). The
  instance (`tools: vec![]`) is a collection field.
- **D3 (changed, needs the user):** the Spotter 1.0 gate is WP-S, WP-C1a and WP-B4a (**2.5 days**, audit and report-only
  checks), plus WP-C2 (2.5 days, **5 days** in all) only if D7 waives ADR 0042's hold. Revision 1's set of about 8 days
  could not be built before Spotter: WP-C4 waits for ADR 0037's WP-7 and WP-C3, and WP-A1 for ADR 0049's WP-4 (or roots-only
  `wire.toml` entries).
- **D7 (new):** waive ADR 0042 §1's release-window hold so that WP-C2 (the layout checks in `paddock-e2e`, test code only,
  additive, no product change) lands before the Spotter 1.0 tag and runs over Spotter's pages? Recommended: yes, as a
  one-off waiver recorded in the harness register, because WP-S's layout part is then a run of the checks instead of a
  hand review; no, if the Spotter window must stay closed to `ui/` changes.
