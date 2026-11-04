# 52. Wired features, visible outcomes and real layouts: every feature reaches the real engine, every job shows how it ended, every control stays in its box

- Status: Proposed (2026-11-04). Written on the user's go-ahead of 2026-11-04 (ER-21 redesign after the Dyno 1.1.0
  release-candidate findings on the user's Windows PC). Awaits an independent review and the user's decisions (D1 to D6).
  Docs only: nothing here is built yet.
- Date: 2026-11-04
- Related: ER-14 / rule 7 (tests never touch the user's machine), ER-17 to ER-19 / rules 10 to 12 (long work off the UI
  thread, live progress, no UI timeouts), ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42
  and 43 (share first; shared behaviour tested once; one real-app smoke per release), ER-23 / rule 22 (screenshots get an
  independent review), ER-27 (logs explain every failure; rule 56 on `origin/ci/publish-tools`), rule 47 (Material
  first), rule 50 (CI economy), rule 54 (the release path builds installers; (c) proposed by ADR 0040), rule 55 / ADR 0034
  (one implementation per mechanism). ADR 0008 (long operations, `paddock-jobs`, `pd-job`), ADR 0010 (notifications),
  ADR 0016 and ADR 0043 (dialog sizing at several scales), ADR 0018 (virtualised lists), ADR 0021 (scaled fixtures),
  ADR 0026 (dialog contracts), ADR 0027 (`pd-table`), **ADR 0037 (the app composition contract; §1 extends it)**,
  ADR 0039 (release gates inspect the artifacts), ADR 0040 (the Windows GUI test session), ADR 0041 (deterministic timing:
  the test clock that drives §3's live updates), ADR 0049 (`wire.toml`, the per-app wire register §1 reads; the no-build
  push check that runs §1 to §3's source checks), ADR 0051 on `origin/docs/paddock-tools-design` (one manager for external
  tools; it fixes the smartmontools instances B1 and B2 below **for tools**; this ADR closes the class for every wire,
  port and feature and does not repeat 0051's design). Drafts on `origin/docs/redesign-adrs-7-9`: 0046 (every check proves
  it can fail: each check here ships with a failing fixture), 0048 (test in the user's launch context: §1.4's VM tests run
  in those contexts once 0048 lands).
- Numbers (scanned with `git ls-tree` and `git grep` over every remote branch after `git fetch`, 2026-11-04):
  - **ADR 0052.** `origin/main` has up to 0049; 0045 to 0048 are on `origin/docs/redesign-adrs-7-9`, 0050 on
    `origin/ci/per-crate-coverage`, 0051 on `origin/docs/paddock-tools-design` (838aad1ea).
  - **Checks C83 to C89.** The highest check number named on any branch is C82 (ADR 0049); C74 to C76 are named in the
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
| The same shape, wider: two more helper senders with no production caller, `HelperLink::firewall` (line 163) and `HelperLink::log_tail` (line 198); eight of the helper's 19 request kinds (`Processes`, `Traffic`, `SocketOwners`, `FileHolders`, `Counters`, `MemoryModules`, `Extras`, `WolState`) are built by no agent code outside tests, only by the helper's own authorisation table and its tests | `git grep -nw <name>` over `apps/dyno/crates/*/src` and `apps/dyno/app/src-tauri/src` at 1589c46bb, excluding `tests` paths and the helper's own crates | measured (grep level, not a call graph). Some may be deliberately later (Dyno ADR 0027 scopes the helper's 1.1.0 reads); nothing in the code says which, and that is the gap |
| Earlier: three Dyno 1.0 tray and Settings features complete in the browser build and unwired in the app (popover spec, dialog contract, `BroadcastChannel`) | lessons.md "What the browser e2e cannot see" (2026-11-01); ADR 0037 | read |
| Earlier: a setting nothing in the product filled in (`utcOffsetMinutes`, default 0) made every "local" day a UTC day | lessons.md, 2026-10-30 (Dyno) | read |
| Earlier: Pitwall 1.0.0 shipped a placeholder icon | lessons.md, line 63 | read |

ADR 0037 made unmanaged state, undeclared dialog commands and unregistered popovers detectable. It does not see a request
kind that the app never sends, a port method nobody calls, or a reply that is always empty: those compile, pass every
unit test (each side is tested against a fake of the other) and pass the browser e2e (the mock backend answers what the
real engine never would).

### Pattern 2: silent failures, and stale state shown as current

| Instance | Source | How checked |
|---|---|---|
| The tool install job's result is read only for `null` (cancelled); a done job's typed output is dropped and the card reloads, so "installed, but the helper does not list it" became a silent "Not installed" | `apps/dyno/app/src/app/state/tool.store.ts:43-56` at 1589c46bb (`const out = await run.result; if (out === null) …` then `await load()` in `finally`) | verified |
| Five more stores narrow a job's output with `'x' in out` and have no `else`: an output of another kind is ignored without a word | `backup.store.ts:66-67`, `extras.store.ts:148-181`, `helper.store.ts:58-60`, `wol.store.ts:121-122` at 1589c46bb | verified |
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
| Footprint: negative margins in shared and app styles | 38 declarations of a negative `margin*` in `ui/projects` and `apps/*/app/src` styles (`.ts` and `.scss`, specs excluded) on origin/main | measured (`git grep -nE 'margin(-inline-start\|-left\|-block\|-top\|-inline)?: *(calc\(.*-\|-[0-9])'`); some are legitimate |
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
inside its box, and every page and dialog is checked for clipping, overlap, blank virtual areas and alignment at 100 % and
150 %, with large lists and live updates, and once on the Windows VM before each release.

## Decision

### 1. The wired contract (extends ADR 0037)

**1.1 Every request kind has a production sender: C83 (static).** For each wire in `apps/<app>/wire.toml` (ADR 0049 §4),
the check enumerates the variants of the request roots (`Request`, `HelperRequest`, `PluginRequest` for Dyno: 109, 19 and
2 kinds at 1589c46bb). For each kind it finds the non-test functions that construct it, and requires that at least one of
them is called from non-test code outside its own impl block, or is the impl of a trait whose object the composition
builds (a port, §1.2). Test code is `#[cfg(test)]` modules, `tests/` folders, `*_tests.rs`, and code under
`cfg(debug_assertions)` test commands. Built in Rust in `./dev check` (ADR 0022), parsing with `syn`; it is a name-level
call check, not a full call graph, and that limit is stated in its fix text. It would have failed at 1589c46bb on `Smart`
(and on `Firewall` and `LogTail`, and on the eight kinds no agent code builds).

A kind that is deliberately not wired yet is declared in `apps/<app>/wiring.toml`:

```toml
[[unwired]]
wire = "agent-helper"
kind = "Processes"
why = "read for other accounts' processes is scoped to 1.2 (Dyno ADR 0027, C1)"
until = "dyno-v1.2.0"        # a tag pattern or a date; past it, C83 fails and the tag check refuses
```

An expired entry fails C83, and `./dev release tag` (ADR 0039's tag gate) refuses a tag while one is expired, as ADR 0034
does for copies.

**1.2 Every port has a production adapter and a caller.** A port trait carries the marker supertrait
`paddock_shell::Port` (a zero-cost trait; a port that is not a `paddock-shell` app's still uses it through
`paddock-commands`). C83 applies the sender rule to each port method. ADR 0037's `composition()` report gains, for each
port the composition manages, the adapter's type name in `Place::Installed`; it fails when that adapter is a test double
(a type under `#[cfg(test)]`, or one whose name the family preset lists: `Fake*`, `Mock*`, `Null*`, `Stub*`), unless
`wiring.toml` declares it (`[[unwired]] port = "…"`, same `until`).

**1.3 No placeholder replies: C84.** In non-test code of the crates that serve a wire (a new `servers = [..]` key per
`[[protocol]]` in `wire.toml`) and of every port adapter:
- `todo!`, `unimplemented!` and `unreachable!("not yet…")`-style stubs fail (one non-test site on origin/main today);
- a struct literal of a type reachable from a reply root (the schema of ADR 0049 §4 names them) that sets a collection or
  `Option` field to a constant empty value (`vec![]`, `Vec::new()`, `None`, `Default::default()`, `String::new()`) fails,
  unless the value is wrapped in `paddock_commands::placeholder!(value, why = "…", until = "…")`, which expands to the
  value and is read by C84 with the same expiry as `[[unwired]]`.

`server.rs:88`'s `tools: vec![]` is exactly such a field. The existing sites go into a shrink-only baseline keyed by the
hash of the line's text, as C71 does (lesson "A rule that counts sites needs a baseline by text", 2026-11-04). The rate of
legitimate constant empties is not measured yet; WP-A2 measures it on Dyno's three wires first, and if it is high, narrows
C84 to collection fields (D2).

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
- a route of the app's router, a dialog of `paddock-manifest.json`, a job kind (`JobSpec` variants; Dyno has 14) or a
  Settings section maps to no feature (or to `[[unlisted]]` with a reason);
- a request kind that is not `[[unwired]]` belongs to no feature;
- a feature names a test that does not exist (the spec title or vm-smoke step id is looked up);
- a feature that depends on a platform (its `platforms` are not all three, or it uses a privileged wire such as
  `agent-helper`) has no vm-smoke step for each OS it lists, **Windows included**.

The tests run against the real engine, never the mock: the `tauri` test in ADR 0037 §8's shared real-app kit (Linux and
Windows CI legs), the `vm` steps in `vm-smoke` (the Windows GUI session of ADR 0040; macOS through ADR 0037's `--smoke`
mode). The Dyno instance's step: install smartmontools through the helper, then Settings says "Installed" with the
helper's version, and Lifespan shows a wear reading or a named reason that came from the helper's `smartctl`.

**1.5 Wire coverage at run time (the dynamic half of C83).** Each wire server keeps a counter per request kind (an array
indexed by the variant, from the schema; no user content). At shutdown it logs one line, `served: kind=count …`, which
also lands in the diagnostic bundle (ER-27). After the real-engine run (§1.4) the kit reads that line and fails on any
kind that is neither `[[unwired]]` nor served at least once on an OS where its feature is declared. This catches what a
name-level static check cannot: a sender behind a switch that is off (ADR 0049), or a branch never taken.

### 2. The outcome contract (ER-17 to ER-19, ER-27)

**2.1 The engine says how a job ended.** In `paddock-jobs` (breaking change, allowed before v1, no shim):
- `JobState::Done` becomes `Done { outcome: Outcome }` with
  `Outcome::Confirmed { summary }` or `Outcome::Unconfirmed { why, next }`;
- `JobState::Failed { message }` becomes `Failed { reason, next }`, where `next: NextStep` is a typed enum (`Retry`,
  `OpenSettings(path)`, `Repair(action)`, `ShowLog`, `None { why }`), never free text alone;
- a job body returns `Result<Outcome, JobFailure>`; `Outcome` is `#[must_use]`.

"Confirmed" means the job read back the state it changed. An install re-probes the installed tool through the same path
the feature will use (the helper's `Info` for Dyno's helper tools); when the read-back disagrees, the job ends
`Unconfirmed { why: "smartmontools was installed, but the helper does not list it", next: Repair(…) }`, not `Confirmed`.
The executor logs every end with the job id, kind, phase durations, outcome kind, reason and cause chain, and no user
content (ER-27).

**2.2 A stored outcome carries its time.** Every outcome the engine stores (last benchmark, last scan, last install) is
`Stamped<T> { at: UtcInstant, app_version: String, value: T }`; there is no unstamped stored outcome on any wire. The engine
words a stored reason as a fact about that run ("the 1.0 install had no runner"), as `33682d905` does for benchmarks.

**2.3 One shared way to show it.** `@decryp7/paddock-ui/shell/jobs` (extending the existing jobs UI, not a new package)
gets:
- `JobRunner` (Dyno's `core/job-starter.ts` moves here; ER-22) whose `run` resolves a `JobEnd<T>`, a closed union:
  `confirmed` (with the typed value and summary), `unconfirmed`, `failed`, `cancelled` and `detached` (the page closed;
  the job goes on and ADR 0010's notice reports its end). `null` is no longer an answer;
- `OutcomeStore.record(key, end)`, a signal store of the latest end per job key, filled from the engine's stamped
  outcomes (not browser storage);
- `pd-outcome`, a component (Material first, rule 47) that renders a live end in the present ("Installed. smartctl 7.5
  found.") and a stamped one with a dated lead-in ("Last run 3 Nov, 14:02: failed. The 1.0 install had no runner."). When
  a newer job of the same key is running, the stamped one is labelled "Previous run" and never shown as a current error.
  Status is never colour alone.

**2.4 Checks.**
- ESLint `paddock/job-end-handled` (type-aware, family preset): a `JobEnd<T>` (or a promise of one) must be passed to
  `OutcomeStore.record` or `pd-outcome`, or be switched exhaustively (a `default` that `satisfies never`). An `await` whose
  value is unused, compared only with `null`, or narrowed with `'x' in end` without an `else` fails. It fails on
  `tool.store.ts:48` and on the four stores above.
- **C86:** every job kind of every app (the `JobSpec` root in `wire.toml`) has an outcome spec through the shared
  `outcomeHarness(kind)`, which drives each of the five ends through the mock and asserts that the owning page shows a
  visible outcome, and for `confirmed` that it shows the value rather than reloading.
- **C87 (Rust, silent drops):** in non-test code, `let Ok(..) = .. else { continue | break | return .. }`, `if let Ok(..)`
  without an `else`, and `.ok()` on a `Result` feeding `?`, `flatten` or `unwrap_or*` must log the error (a `log::` or
  `tracing::` call in that branch), carry it into the value (`Reading::unavailable(e)`, a typed error), or carry a
  `// silent: <why>` comment. Clippy's `let_underscore_must_use` joins the family preset for the `let _ =` half. Today's
  sites go into a text-hash baseline (268 `let Ok .. else` sites family-wide on main is the upper bound).
- **Spawns log once, at the spawn point.** Every external tool run goes through `paddock_proc`'s run helper (rule 53
  already routes kills there), which logs the program, masked arguments, exit code, duration and the tail of stderr. That
  covers the 107 silent `dyno-platform` files without a log call in each.

### 3. The real-UI layout contract

**3.1 Design-system rule: a control draws inside its own box.** No negative margin, negative inset or translate moves the
visible part of a control (box, thumb, icon, label) outside its host element. The shared rule at
`_material.scss:779-781` is removed. Inline forms align instead by one of two means, chosen in WP-C1 by the layout check
over the gallery at both scales (D4):
- (i) Material's density tokens shrink the checkbox state layer in inline forms, so the overhang fits in the row's
  padding; or
- (ii) the hover halo is drawn as an overlay that may be clipped without hiding the box, and the box sits at its host's
  start edge.

The touch target (the invisible 48 px hit area) stays: removing it would be an accessibility regression.

A page never restyles a shared control's geometry: selectors on `.mat-mdc-*`, `.mdc-*` or `pd-*` internals that set
margin, inset, translate, width or height live only in `ui/projects/paddock-ui/scss/`. That is what made the shift
double. **C88** checks both (in `.scss` files and in component `styles` strings): a negative margin, inset or translate,
or a geometry rule on a shared control outside the shared SCSS, fails unless it carries `/* pd-layout-ok: <why> */`.
Today's 38 negative-margin sites go into a text-hash baseline; the design guide gets the rule and its why.

**3.2 Shared layout checks in `@decryp7/paddock-e2e`.** `layoutProblems(root)` reports facts from the page and decides in
Node (as `screenshot-check.ts` does). It fails on:
1. **Clipped:** a visible control or text box partly outside an ancestor whose overflow is not `visible` on an axis that
   ancestor does not scroll, or partly outside the viewport. This generalises the in-progress `checkboxLayoutProblems` and
   the cut-off checks already in `layout.ts`.
2. **Overlap:** two in-flow siblings (not absolute, fixed or sticky, not `[data-pd-overlay]`) whose boxes intersect by more
   than 1 px, or a control whose corners `elementFromPoint` finds covered by something outside it.
3. **Blank virtual area:** a `cdk-virtual-scroll-viewport` whose content is taller than itself and whose last drawn row
   ends more than one row height above its bottom edge. Checked at start, after a resize, after scrolling to the end and
   after the data grew (the `3c28a9371` spec, generalised).
4. **Rows aligned:** in repeated rows (`[role=listitem]`, `mat-row`, `.pd-form-row`), the start x of the leading control and
   of the text agree within 1 px across rows, and a checkbox's centre lies within 3 px of its first text line's centre.

`expectLayoutClean(page)` runs these together with `expectScreenshotClean` (`b726d56de`), so an app calls one function.
The page-side fact gatherer is one self-contained function, so the same code also runs through WebDriver's `execute` in
the real WebView2 (§3.4).

**3.3 Every page and dialog, at two scales, with large lists and live updates: C89.** Each app has one tour spec that
visits every route of its router and opens every dialog of its manifest, and calls `expectLayoutClean` at each stop.
**C89** enumerates the routes and dialogs (dialogs as ADR 0037's C68 does) and fails when the tour misses one. The tour runs
over this matrix:

| Axis | Values |
|---|---|
| Device scale | 1 and 1.5, with the viewport in CSS px = the physical window / scale (the default 1280 × 800 window is 853 × 533 CSS px at 150 %), plus the app's minimum window size at 1 |
| Data | `default`; `large` (the mock's scaled fixtures, ADR 0021: at least 5 000 table rows, 500 notifications, every name at its maximum length); `live` (the mock emits updates under ADR 0041's test clock: rows added at the top, values that change width, a list that grows after its first measure) |
| When | after load, after 20 live ticks, after a window resize |

Branch runs (rule 50 (e), affected plan) run `default` at 1 and `large` at 1.5 for the apps a change touches, Chromium
only; nightly runs the full matrix. Run times are measured in WP-C3 and recorded in `docs/benchmarks.md` before anyone
quotes them (rule 48).

**3.4 A Windows VM page pass before every release.** ADR 0040's Windows GUI session already opens every dialog at 100 %
and 150 %. This adds every page: the real app's tour through ADR 0037 §8's WebDriver kit (WebView2 and msedgedriver)
inside the `vm-smoke` Windows GUI session, at 100 % and 150 %, with the real engine and the VM's real data. It runs
`layoutProblems` in the real WebView2 and saves one screenshot per page and scale. A person other than the author reviews
the screenshots (ER-23 / rule 22). Rule 54 (c) (ADR 0040) gains it: a tag needs a `--os windows --gui --pages` result with
no FAIL for the tagged head and the reviewer's sign-off. macOS runs the same tour in the local macOS VM through ADR 0037's
`--smoke` mode. VMs stay local (the 2026-10-31 decision); CI keeps its Windows WebDriver leg.

### 4. Proposed rule 67

> 67. **A feature is done when it is wired, shows how it ended, and fits on a real screen.** (a) Every request kind on an
> app's declared wires and every port method has a production caller in the composition `run` builds, or a dated entry in
> `apps/<app>/wiring.toml`; no reply field is a constant placeholder without a dated `placeholder!`; every user-facing
> feature in `apps/<app>/features.toml` has one end-to-end test against the real engine, and a platform feature has one on
> the VM for each OS it supports, Windows included. (b) Every job ends with a visible outcome: confirmed, failed with its
> reason and next step, or done but not confirmed with why; a job's end is never discarded; a stored outcome carries its
> time and reads in the past; an error is logged or carried, never dropped. (c) A control draws inside its own box; pages
> never restyle a shared control's geometry; every page and dialog passes the shared layout checks at 100 % and 150 % with
> large lists and live updates, and on the Windows VM before every release. Checked by C83 to C89, ESLint
> `paddock/job-end-handled` and the real-engine runs (ADR 0052). *Why:* the Dyno 1.1.0 release candidate on the user's
> Windows PC showed a helper feature with no caller and an always-empty reply, a done job read as "Not installed", a past
> failure shown as current, and controls clipped by a doubled negative margin, all with every test green.

## Checks that make the classes impossible or detected

| Check | Kind | Would have failed on |
|---|---|---|
| C83 static senders, `wiring.toml` with expiry | source check over every declared wire and port | `HelperLink::smart`, `firewall`, `log_tail`; the eight unbuilt helper kinds (until declared) |
| `composition()` port adapters (ADR 0037 §7 extended) | enumerating test | a port left on a fake in `Place::Installed` |
| C84 placeholders, `placeholder!` with expiry | source check over server crates and adapters | `server.rs:88` `tools: vec![]` |
| C85 `features.toml` | source check: every route, dialog, job kind, setting and request kind maps to a feature with a real-engine test, VM steps per OS | a SMART feature with no Windows VM step |
| Wire coverage at run time | real-engine run reads the served counts | `Smart` never served on Windows |
| `Outcome` in `paddock-jobs` | type (impossible by construction) | "done" with nothing to say |
| ESLint `paddock/job-end-handled`, C86 outcome harness per job kind | lint plus enumerating spec | `tool.store.ts:48`; the four `'x' in out` stores |
| `Stamped<T>`, `pd-outcome` | type plus shared component | the benchmark's present-tense 1.0 failure |
| C87 silent drops, Clippy `let_underscore_must_use`, spawn logging in `paddock_proc` | source check with baseline; lint; one mechanism | `hardware.rs:263` |
| C88 negative geometry, page restyles of shared controls | source check with baseline | the MCP page's and the notification pane's second shift |
| `expectLayoutClean`, C89 tour over every page and dialog, two scales, large and live | shared e2e checks plus enumerating check | all three layout instances |
| Windows VM page pass, rule 54 (c) | release gate, local VM | what only WebView2 at 150 % shows |

Each new check ships with a fixture it fails on (the 0046 draft's demand): the 1589c46bb shapes above, reduced to
synthetic files.

## Work packages and order

ADR 0022 G.7 keeps larger shared changes out of a release window, and rule 1 puts release-critical work first. The current
order is Dyno 1.1.0 → Spotter 1.0 → Pitwall 1.1 → Pitcrew 1.1 → Debrief 1.2. Sizes are agent-days (estimates).

| WP | What | Days | Lands |
|---|---|---|---|
| WP-0 | The instance fixes: `fix/dyno-windows-smart`, `fix/dyno-bench-stale-error`, `fix/settings-checkbox-clip`, `fix/notification-pane-layout`, `fix/table-virtual-blank` | (outside this ADR) | Dyno 1.1.0 |
| WP-S | **Audit sweep** of Spotter's and Pitwall's release branches for the three patterns with this ADR's measures: senders without callers (grep level), job results discarded or narrowed without `else`, negative margins and page restyles of shared controls, virtual lists; file each finding as a fix with a failing test | 1 | **before Spotter 1.0** |
| WP-C2 | `layoutProblems` and `expectLayoutClean` in `paddock-e2e` (test kit only, additive), each with failing fixtures; run once over Spotter's pages at 1 and 1.5 | 2 | **before Spotter 1.0** |
| WP-C4 | Windows VM page pass (the tour in `vm-smoke --gui --pages`, screenshots, review), rule 54 (c) text | 2 | **before Spotter 1.0** (Spotter is the first user; needs ADR 0040's session) |
| WP-A1 | C83 with `wiring.toml`, failing fixtures; Dyno's and Spotter's registers written (data only) | 2 | **before Spotter 1.0** (no product change) |
| WP-B4 | C87 with its baseline; Clippy `let_underscore_must_use`; spawn logging in `paddock_proc` | 1.5 | C87 before Spotter 1.0; the `paddock_proc` change after its tag |
| WP-C1 | Remove the negative-margin rule (choose (i) or (ii)), C88 with its baseline, the design-guide rule, gallery screenshots reviewed | 1.5 | after Spotter 1.0, **before Pitcrew 1.1** (C88's "no page restyles" half may land before Spotter) |
| WP-B1 | `Outcome`, `NextStep`, `Stamped<T>` in `paddock-jobs`; executor logging; every app's job bodies updated (Dyno 14 kinds, Spotter, Pitwall, demos) | 3 | after Spotter 1.0, **before Pitcrew 1.1** |
| WP-B2 | `JobRunner` (Dyno's copy moves), `JobEnd`, `OutcomeStore`, `pd-outcome`; Dyno's stores adopt it | 2.5 | with WP-B1 |
| WP-B3 | ESLint `paddock/job-end-handled`, C86 and `outcomeHarness`; specs for every job kind | 2 | with WP-B2 |
| WP-A2 | C84 with `placeholder!`, measured on Dyno's wires first (D2) | 1.5 | after Spotter 1.0 |
| WP-A3 | `Port` marker, port adapters in `composition()`, served-kind counters and the kit's read-back | 2 | after ADR 0037 WP-2 and WP-7; before Pitcrew 1.1 |
| WP-A4 | `features.toml` and C85; Dyno's and Spotter's registers with their real-engine and VM tests | 3 | C85 report-only before Spotter 1.0, enforced before Pitcrew 1.1 |
| WP-C3 | The tour spec per app, C89, the `large` and `live` mock profiles, the scale matrix; run times measured and recorded | 3 + 0.5 per app | after Spotter 1.0; Pitwall and Pitcrew before their tags |
| Adoption | Pitwall, Pitcrew, the demos (registers, outcome specs, tours), about 1.5 each; Debrief when it moves to the shell | 6 | per release |
| **Total** | | **33** (with five apps' tours: 35.5) | |

**Before the Spotter 1.0 tag:** WP-S, WP-C2, WP-C4, WP-A1, C87 and C88's restyle half, with C85 in report-only mode
(about 8 days; none changes product code in `crates/`). **Before the Pitcrew 1.1 tag:** WP-B1 to WP-B3, WP-C1, WP-A3,
C85 enforced and Pitcrew's adoption, because Pitcrew moves onto the shell in 1.1 and runs long jobs.

**Rollback.** Each check is its own module with its own baseline, and can be reverted alone. WP-B1 is the one breaking
change; it lands with its adoption in every app in one change (rules 39, 43), so there is no half state.

## Consequences

- **CI cost (rule 50).** C83 to C89 are source checks with no build; they run in ADR 0049's no-build push check (rule 50
  (f)) and in `./dev check --pre-push`, in seconds. The real-engine feature tests and the served-kind read-back ride ADR
  0037's real-app legs: Linux in the affected plan, Windows nightly and at release. The layout tour adds Playwright time:
  a slice on branches, the full matrix nightly, measured before it is quoted. The Windows page pass adds to the local
  release VM run, not to CI (ADR 0040 estimates its GUI run at 20 to 30 minutes; the pages are not measured yet).
- **Authoring cost.** Each app keeps two more registers (`wiring.toml`, `features.toml`); each job kind gets an outcome
  spec; each feature that depends on a platform gets a VM step per OS. That cost is the point: the gap was that nobody had
  to say which test proves the last join.
- **Breaking changes before v1 (no shims):** `JobState::Done` and `Failed` change shape; `JobFollow` returns `JobEnd`;
  Dyno's `JobStarter` moves into `paddock-ui`; the checkbox alignment rule changes for every app's forms.
- **Logs** gain a served-kinds line per wire server and one line per tool spawn; neither carries user content (ER-27).
- **Limits.** C83 is a name-level check: a call through a closure or a macro can hide a sender, so the run-time counts
  (§1.5) are the backstop. The layout checks see geometry, not meaning; the reviewed release screenshots stay.

## Alternatives considered

- **More browser e2e on the mock at default size.** That is what we have, and all three patterns passed it. Rejected as
  the answer; the mock stays for development.
- **Rely on `dead_code`.** `pub` functions in library crates are never dead to rustc, and merging the crates to make them
  private is not possible across the app, agent and helper. Rejected.
- **Coverage of an instrumented real-app run instead of per-kind counters.** Exact, but it needs instrumented builds on
  every OS and heavy runs. The counters give the per-request answer cheaply and also help a user's diagnostic bundle.
  Rejected for now.
- **Pixel-diff screenshot regression in CI.** Brittle across fonts, OS builds and scales, high upkeep, and a diff does
  not say what is wrong. The fact-based checks plus a person's review of the release screenshots are used instead.
- **A toast for every job end.** A toast is gone before it is read and has no time. The outcome sits on the feature's own
  card; ADR 0010's notice covers a page that was closed.
- **Drop the checkbox touch target to stop the overhang.** An accessibility regression. Rejected.
- **A release checklist item "check the layout on Windows".** It is not measurable and gets skipped under pressure.
  Rejected in favour of the gated VM pass.
- **Fix the five instances only.** Each is fixed (WP-0); the earlier lessons show each class coming back. Rejected as
  the whole answer (ER-21).

## Decisions for the user

- **D1:** Adopt rule 67 as one rule with three parts (recommended), or three separate rules?
- **D2:** C84's scope: every constant empty `Option` or collection field in reply types (recommended to start with,
  measured in WP-A2), or collection fields only from the start?
- **D3:** The Spotter 1.0 gate: hold the tag for WP-S, WP-C2, WP-C4 and WP-A1 (about 8 days) (recommended), or
  tag Spotter after WP-S alone and land the rest before Pitwall 1.1?
- **D4:** The checkbox alignment after the shared rule goes: (i) Material density tokens in inline forms (recommended,
  less code), or (ii) an overlay halo?
- **D5:** Does every release need the Windows page pass and a screenshot review, including patch releases (recommended:
  yes for any release whose diff touches UI or a shared UI package; skip for a Rust-only patch with its reason in the tag
  checklist)?
- **D6:** The `large` profile sizes (5 000 rows, 500 notifications): enough, or scale to the largest real data seen per
  app (Debrief's millions of events need its own virtualised-only profile)?
