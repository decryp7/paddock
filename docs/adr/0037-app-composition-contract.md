# 37. The app composition contract: each app is built in a test the way `run` builds it, and one shared check verifies it

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03, after the independent review
  approved it (round 1 and a delta re-review, 2026-11-03).
- Date: 2026-11-03
- Related: ER-14, ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42, 43 (share first, test
  shared behaviour once, one real-app smoke per release), rule 34 (windows share state by commands and events), rule 55
  (ADR 0034, one implementation per mechanism); ADR 0012 (the app shell), ADR 0026 (the shared dialog API, its recorder
  and `dev check dialog-commands`), ADR 0029 (the shell owns the start, `paddock_shell::testing::start`, C50), ADR 0022
  (R19, the dialog command check in Rust); lesson "What the browser e2e cannot see" (2026-11-01).
- Evidence (read at `origin/main` `9da2ccf59`, which is also the `dyno-v1.0.0` tag, and on the release branches named
  below, 2026-11-03):
  - `crates/paddock-shell/src/builder.rs`: `Builder::build` routes all 17 command tables of the shell crates without
    conditions. `Builder::install` manages the state of 12 parts only `if let Some(..)`: health set-up, diagnostics,
    files, storage, service, packs, channels, mcp, windows, updater, first run and downloader.
  - The release-window fix for instance 4: `d371d5e4a` on the local branch `fix/dyno-history-releases` (2026-11-03,
    `crates/paddock-shell/src/builder.rs`, +4 lines). When the app sets up no files part, the shell now manages a minimal
    `Files` service (no folders, no recent lists, web addresses only). It has tests in `paddock-shell` and in Dyno's
    `tests/shell.rs`.
  - `crates/paddock-files/src/tauri.rs:319`: `files_open_url(state: State<'_, Arc<Files>>, url)`.
    `crates/paddock-updater/src/lib.rs:86`: the shell's own `pd.about` contract declares `files_open_url`.
    `SHELL_DIALOGS` (`crates/paddock-shell/src/lib.rs`) registers `pd.about` in every app on the shell.
  - The parts each app configures (`git grep` of the builder calls): Dyno on main has no `.files(`, and neither do Spotter
    (`steward/s29-spotter-1.0`), Pitwall (`steward/s30-pitwall-1.1`) or the workbench demo. The shell demo has it.
  - `apps/dyno/app/src-tauri/tests/shell.rs` builds `shell(Place::Test {..})`. `run()` (`src/lib.rs:345`) builds
    `shell(Place::Installed)`, which adds `.diagnostics(..)`, drops `.in_memory()` and adds the app's
    `.invoke_handler(..)` outside the composition the test sees.
  - Debrief's real-app smoke: `apps/debrief/app/e2e-tauri/` (WebdriverIO through tauri-driver, six specs) runs in
    `.gitea/workflows/debrief.yml` (lines 548 to 760) on Linux under Xvfb and on Windows with msedgedriver. Its config
    says macOS has no WebDriver for WKWebView. That agrees with Tauri's WebDriver page, which lists desktop support for
    Windows and Linux only (<https://v2.tauri.app/develop/tests/webdriver/>, read, not re-checked today).

## Context

Each of the following shipped, or nearly shipped, while every browser e2e test and every unit test was green.

| # | Date | Instance | How it was found | State |
|---|---|---|---|---|
| 1 | 2026-11-01 | Dyno's tray icon did nothing on click: the shell's `toggle_popover` looks up a `Popovers` spec that Dyno never managed | Dyno 1.0 VM test | fixed `2746040e8`, by an app-side `manage` inside the tray closure plus a Dyno start test |
| 2 | 2026-11-01 | Settings' "Popup widgets" and menu-bar editors loaded for ever: the `pd.settings` contract did not declare `widget_layout`, `widget_choices` and `popover_preview`, so the guard refused them to the dialog window | Dyno 1.0 VM test | fixed for those pages by `settings-dialog-commands.spec.ts` (a recording `invoke` over the real `TauriBackend`) |
| 3 | 2026-11-01 | "Customize widgets…" did not re-target an open Settings window: the inputs travelled on a `BroadcastChannel`, which the browser e2e shares between pages and the other webview did not hear | Dyno 1.0 VM test | fixed `c7baebd24` with a native `paddock://dialog-inputs` event |
| 4 | 2026-11-03 | "See all releases" in About failed in the released Dyno 1.0.0: "state not managed for field `state` on command `files_open_url`" | user feedback on 1.0.0 (bug 8) | fixed for every shell app by `d371d5e4a` (a minimal `Files` service, not yet on `main`). The gap was the same in Spotter, Pitwall and the workbench demo (table below). WP-1 replaces this fix after the release window |

The browser builds replace exactly the part of the app that is the real contract: Tauri state, the command guard and its
dialog contracts, native events and the tray. ADR 0029's start test closed one class (a window built inside a plugin hook).
It builds the app's composition on the mock runtime, but it checks only that a main window and a tray exist. It does not
check that what the windows will call can be answered.

**Pattern.** A feature is complete in the browser build and unwired in the app whenever the wiring (managed state, a
contract entry, a popover spec, a native event) is a separate call from the thing that needs it, and only the installed app
puts the two together.

**Invariant.** For the composition `run` builds:
- every routed command can find the state it reads;
- every command a window or dialog sends is declared for that window class or dialog;
- every clickable tray item has its popover;
- cross-window inputs travel on native events.

### Instances found (2026-11-03)

| Where | Instance | Status |
|---|---|---|
| Dyno (main, 1.0.0) | `pd.about` → `files_open_url`, no `Builder::files` | **broken** (shipped); fixed by `d371d5e4a` |
| Spotter (`steward/s29-spotter-1.0`) | same: `.add_to .app_commands .dialogs .downloader .launch .tray .updater`, no `.files` | broken before its 1.0 tag; fixed by `d371d5e4a` once it lands |
| Pitwall (`steward/s30-pitwall-1.1`) | same: no `.files` | broken before 1.1; fixed by `d371d5e4a` once it lands |
| Workbench demo (main) | same: no `.files` | fixed by `d371d5e4a` once it lands |
| Shell demo (main) | configures `.files` | fine |
| Every shell app | `paddock-storage`'s "Show in folder" needs `Builder::files` (its doc comment says so) but nothing checks it | at risk |
| Every shell app | the commands of the 12 optional parts are routed whether or not the part is set up (`d371d5e4a` covers files only) | at risk (the class) |
| Dyno | the test composition differs from `run`'s (diagnostics installed only, `.native_menu(false)` and `.in_memory()` in the test place, the app handler added outside `app()`) | at risk |
| Dyno, fixed | tray popover managed by hand inside the tray closure | fixed by hand; the class stays open for the next tray |
| `ui/…/shell/dialogs/src/io.ts`, `retarget.ts`; `shell/src/theme/appearance-backend.ts`, `provide-appearance.ts`; Debrief `popout/sync-port.ts`, `popout-backend.ts`; shell demo `demo-settings.ts` | `BroadcastChannel` in production code | at risk: each must be shown to have a native-event path (the dialogs one has had one since `c7baebd24`) |
| Pitcrew, Debrief | not on the shell plugin at `origin/main` (Debrief has its own start; Pitcrew moves in 1.1) | adopt when they move |

## Decision

### 1. One composition, used by `run` and by the test

Each app exposes one `compose(place, ..) -> tauri::Builder<R>`. Both `run` and the composition test call it. `Place`
(`Installed` or `Test { dir }`) may change only what this closed list names:
- folders;
- storage (`.in_memory()` or on disk);
- the native menu bar (`.native_menu(false)`: the mock runtime has no menu bar, and the menu model and its commands are
  still built and checked);
- the OS plugins that rule 7 keeps out of tests (single instance, autostart, the updater plugin, notifications).

`composition()` (§7) knows the list and reports any other difference. The place must never change which shell parts,
dialog tables, command tables, tray or popovers are present. Diagnostics becomes a part that a test gives a temporary
folder, not a part that only the installed app has.

`Builder::add_to` also installs the app's own invoke handler, behind the same guard, from `Builder::app_commands`. `run`
then has nothing left to add outside the composition. (Dyno adds `.invoke_handler(..)` after `app()` today.)

### 2. A part registers its commands together with its state (the gap becomes impossible for shell parts)

`Builder::build` routes a part's command table only when that part is configured. A command of a part that is not
configured is answered by the guard with a typed `CommandError`, code `partNotConfigured`, naming the builder method
("this app has no `Builder::files`"). Tauri's "state not managed" can then no longer happen for a shell command.

What a part needs is declared with the part. `Builder::problems()` reports a dialog contract, manifest entry or window
class that names a command of an unconfigured part (for example "`pd.about` calls `files_open_url`, but this app has no
`Builder::files`").

Opening a web address needs no path grants. `files_open_url` therefore moves to an always-on part of the shell (its own
`open_url`, kept in `paddock-files` but routed with the always-on parts), so `pd.about` works in every app without
`Builder::files`.

**Order against the releases.** Inside the release window, instance 4 is fixed by the small change `d371d5e4a`: a minimal
`Files` service whenever the app sets up none. That lands for the Spotter 1.0 and Pitwall 1.1 tags (and Dyno 1.1.0).
ADR 0022 G.7 keeps larger `crates/` changes out of a release window, so WP-1 (conditional routes, `partNotConfigured`,
`open_url`) is **not** scheduled before those tags. It replaces the minimal service afterwards, and deletes it in the same
change.

### 3. Every command declares the state it reads, and the composition test checks it

`paddock_commands::command_table!` entries gain an optional `uses(T, ..)` clause, for example
`Mutation [WINDOW | TRAY] => tauri::files_reveal uses(Arc<Files>)`. The table then knows, for every command, which managed
types it extracts. After Tauri's set-up has run once on the mock runtime, the composition test calls
`AppHandle::try_state::<T>()` for every declared type of every routed command (shell tables and the app's table). No
command body runs, so nothing touches the machine (ER-14).

A source check verifies the declarations against the code. Provisionally numbered **C67** (the number is taken when it is
built), it runs in `./dev check`, or in the JavaScript conformance engine until R8. For every function a table lists, it
compares the function's `State<'_, T>` parameters with the table's `uses` list, resolving type aliases in the same file
(`type Managed<'a> = State<'a, Arc<Files>>`). A command that reads state it does not declare fails, and so does a
declaration with no parameter behind it.

A command body can also read state without a parameter (`app.state::<T>()`, `window.state::<T>()`), and C67 would not see
that from the signature. Two rules close the gap:
- **C67 also flags `.state::<…>()` calls inside any function a command table lists.** This is the check that makes the
  class detected, and it holds whatever the lint allows.
- **Clippy's `disallowed-methods`** lists `tauri::Manager::state` in the family preset (`lint/clippy.toml`), with the reason
  "read state through a declared `State<T>` parameter or `try_state`".

The lint has a measured footprint. At `origin/main` `9da2ccf59`, `git grep '\.state::<'` finds **118** call sites in Rust
files that are not test files (Debrief 87, Pitcrew 11, Pitwall 11, Spotter 4, Dyno 2, shared crates 3). That is an upper
bound, because it also counts inline `#[cfg(test)]` modules. So the lint is scoped:
- test code is exempt once per crate, with `#![cfg_attr(test, allow(clippy::disallowed_methods))]` in its `lib.rs`;
- set-up code (the closures that manage a state and read it back) gets a local `#[allow]` with a one-line reason, and
  nothing else may;
- the remaining sites are migrated to declared parameters or `try_state` by WP-3's implementer, from that list. Debrief's
  87 sites are migrated when Debrief moves to the shell.

`try_state` stays allowed, because its `None` is a handled state.

### 4. The tray and its popovers are one value

`paddock_window::tauri::Tray` takes its popovers in its constructor. Either `Tray::new(backend, view).popover(item,
PopoverSpec)` or `.no_popover(item, reason)` must be given for every item whose click opens one, and the shell manages
`Popovers` from the `Tray`. The app no longer calls `manage` for it. A tray without a popover for a clickable item fails
`Builder::problems()`.

The composition test also clicks: through the `HeadlessTray` it fires the icon's click and checks that a `tray*` window is
built on the mock runtime. Today's test stops at "a popover is registered" (lesson 2026-11-01).

### 5. Every window's calls are checked against its contract

ADR 0026 §3's recorder is built as decided there. The one `invoke` wrapper of `paddock-ui` tags each call with the active
dialog id or window class and fails the run on an undeclared command. `paddock-ui` adds a test kit,
`dialogCommandHarness(dialogId, page)`, that renders a dialog page over the real `TauriBackend` with a recording `invoke`
and checks every command against the generated contract. Dyno's `settings-dialog-commands.spec.ts` is its first user and
then moves into the kit (ER-22). Every dialog an app registers has one harness spec. A conformance check, provisionally
**C68**, enumerates the dialog ids in each app's manifest and fails on a dialog without a spec. `dev check
dialog-commands` (ADR 0026, ADR 0022 R19) stays the nightly "declared but never sent" ratchet.

### 6. Cross-window inputs travel on native events

A `BroadcastChannel` is allowed only inside one adapter in `paddock-ui` that also listens to the native window-system
event, and it is never the only path. An ESLint `no-restricted-globals` rule enforces this in the shared preset for every
app and for `ui/`, with the adapter file as its one exception. Each site in the table above either moves onto the adapter
or is shown (with a test of both ends) to be same-webview only. Because `no-restricted-globals` does not see a member
access, the preset also sets `no-restricted-properties` for `window.BroadcastChannel`, `globalThis.BroadcastChannel` and
`self.BroadcastChannel`, with the same exception.

### 7. One shared check: `paddock_shell::testing::composition`

It extends ADR 0029's `start`. It builds `compose(Place::Test)` on the mock runtime, runs the set-up once and returns a
`Composition` report:
- the parts, tables, dialogs, tray items and popovers present;
- whether the main window and the tray opened, and what a tray click opened;
- every unmanaged declared state;
- every `problems()` entry.

It also builds the parts report of `compose(Place::Installed)` without running any closure, and reports the difference
from the test place. Each app has one test: `assert!(composition(compose).is_ok(), "{report:#?}")`. C50 is extended to
require it in every app on the shell, and the new-app templates get it.

### 8. A real-app smoke per OS

The checks above prove the wiring on the mock runtime. Rule 43's "one real-app smoke per release" proves the real
runtime, webview and OS. One shared kit, `@decryp7/paddock-e2e/tauri`, comes from Debrief's `e2e-tauri` harness (moved,
not copied; rule 55). It gives each app the same smoke:
1. Start the debug app with a temporary data folder, and wait for the shell's ready handshake.
2. Open every dialog the manifest lists once, and press its first action.
3. Fire the tray click through a debug-only test command, and wait for the popover window.
4. Re-target one dialog.
5. Quit, and check that the process tree is gone.
6. Scan the app's own log. The smoke fails on any guard denial, any "state not managed", any `ERROR` line, and any window
   that never reached ready.

That last step automates the lesson's "read the log of the installed app once".

| OS | Driver | Where it runs |
|---|---|---|
| Linux | tauri-driver + WebKitWebDriver under Xvfb | CI: each app's Linux leg, in the affected plan when the app's Rust or UI changes, and in release gates |
| Windows | tauri-driver + msedgedriver (Debrief's `fetch-msedgedriver.mjs`) | CI: the Windows runner, nightly and release gates (as Debrief does today) |
| macOS | no WKWebView WebDriver: the same steps driven from inside the debug app by a `--smoke <plan.json>` mode (shell test commands, no external driver), with the log scan | a **local VM only** (the decision "VMs are local only", 2026-10-31): `vm-smoke`'s macOS image before every tag. It never runs on the CI Mac, because that would put windows on the user's desktop (ER-14) |

The Windows GUI session needed for a tray and popup click test (instance 4 of the Dyno feedback) is the sibling redesign
"Windows GUI test session in vm-smoke". This ADR uses it once it exists, and until then relies on the CI WebDriver leg.

## Checks that make the class impossible or detected

| Check | Kind | Fails on today's instance |
|---|---|---|
| Parts route with their state; `partNotConfigured` | impossible by construction (shell parts) | #4 cannot occur (`d371d5e4a` closes it for files only, until WP-1) |
| `composition()`: declared state managed (`try_state`) | enumerating test, every routed command | #4 (`Arc<Files>` for `files_open_url`) |
| C67: `uses` matches the `State` parameters, and no `.state::<T>()` in a listed command; Clippy `disallowed-methods` for `Manager::state` | source check over every table, lint over every crate | any undeclared state read |
| Place parity in `composition()` | enumerating test | Dyno's diagnostics and handler divergence |
| `Tray` carries its popovers; click opens a `tray*` window | type plus test | #1 |
| Recorder, harness spec per dialog, C68 | run-time plus enumerating check | #2 |
| ESLint `BroadcastChannel` rule | lint over every file | #3 |
| C50 extended | conformance over every shell app | an app without the composition test |
| Real-app smoke with log scan | per OS, per release | all four, on the real runtime |

## Rollout and order

Rule 1 sets the order: the release-critical parts come first, and shared changes land with their adoption (rule 39).

0. **In the release window:** `d371d5e4a` (the minimal `Files` service) lands for Dyno 1.1.0, Spotter 1.0 and Pitwall 1.1.
   It is the only `crates/` change this ADR makes before those tags (ADR 0022 G.7).
1. **WP-1 (shared, after the Spotter 1.0 and Pitwall 1.1 tags):** move `open_url` to an always-on part; make the routes
   conditional with `partNotConfigured`; add the `problems()` entry; delete the minimal `Files` service of `d371d5e4a`.
2. **WP-2:** `composition()` with place parity and the tray click; extend C50; adopt it in Dyno, Spotter and the demos.
3. **WP-3:** `uses` in `command_table!` for about 187 shared and about 170 app commands (Dyno 26, Spotter 47, Pitwall 85,
   demos 11); the `try_state` sweep; C67. The annotation is mechanical and goes to an implementer with the table as the
   brief.
4. **WP-4:** `Tray` with popovers. Dyno's hand-managed `Popovers` is deleted.
5. **WP-5:** the recorder, `dialogCommandHarness`, C68; Dyno's spec moves into the kit.
6. **WP-6:** the ESLint rule and the `BroadcastChannel` adapter; settle each listed site.
7. **WP-7:** `@decryp7/paddock-e2e/tauri` from Debrief's harness, with the log scan; Linux and Windows CI legs.
8. **WP-8:** the macOS `--smoke` mode and its `vm-smoke` step.
9. **Adoption order:** Dyno (1.1.0), Spotter (1.0), Pitwall (1.1, on the shell), Pitcrew (1.1, when it moves to the
   shell), the demos with WP-2. Debrief keeps its own start until it moves to the shell; its `e2e-tauri` becomes the
   shared kit in WP-7, and it adopts `composition()` when it moves.

**Rollback.** WP-1 is one guard rule and one route list. If `partNotConfigured` misbehaves, the routes go back to
unconditional and `problems()` keeps reporting. The other packages add checks and can be reverted one by one.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 parts with state, `open_url`, `problems()` | 1.5 |
| WP-2 `composition()`, place parity, tray click, C50 | 2 |
| WP-3 `uses`, `try_state` sweep, C67 with the in-body `state` rule, the Clippy entry (mostly implementer) | 2.75 |
| WP-4 `Tray` with popovers | 1 |
| WP-5 recorder, harness kit, C68 | 2.5 |
| WP-6 `BroadcastChannel` lint and adapter | 0.5 |
| WP-7 shared real-app smoke kit, Linux and Windows legs | 3 |
| WP-8 macOS `--smoke` mode in `vm-smoke` | 1.5 |
| Adoption: Dyno, Spotter, Pitwall, Pitcrew, two demos (composition and smoke, about 0.75 each) | 4.5 |
| **Total** | **19.25** |

The shipped instance is already closed in the release window by `d371d5e4a` (outside these estimates). WP-1 makes the class impossible after the tags.

## Consequences

- Breaking change to the shared API (allowed before v1, no shim):
  - `files_open_url` moves to `open_url` in the always-on part;
  - `Tray::new` requires a popover decision for each clickable item;
  - `command_table!` gains `uses`;
  - `add_to` installs the app's own handler.
- Each app's `run` shrinks to the OS plugins and `compose(Place::Installed)`. A difference between test and app has to be
  a declared `Place` difference.
- Every new shell part must declare its commands and state in one place. Forgetting one fails C67 or the composition test,
  not the user.
- The mock runtime still cannot show OS or webview behaviour. The real-app smoke covers that per release, and on macOS
  only locally in a VM.
- The macOS smoke mode, and the debug-only tray-click test command of §8, are test code in debug builds only (behind
  `cfg(debug_assertions)` or a `smoke` feature never enabled in release builds). Each debug-only command
  carries a unique marker string compiled in only under `cfg(debug_assertions)`. ADR 0039's verifier checks that the
  marker is absent from every shipped binary without running anything, so a release build cannot carry them.

## Alternatives considered

- **Only a test helper, no API change.** A `try_state` sweep alone detects the gap but leaves "configure a part, then
  remember its state" as two calls. Rejected as the whole answer; it is kept as the check behind the API.
- **Invoke every command on the mock runtime and look for Tauri's state error.** This needs no declarations, but it runs
  the bodies of commands without arguments (side effects, ER-14), and a deserialisation error on an earlier argument
  hides a missing state. Rejected.
- **A proc-macro attribute that derives the state types from the signature.** It is exact, but every `#[tauri::command]`
  in seven workspaces would change. The `uses` clause plus C67 gets the same guarantee with a declarative macro. This can
  be revisited if C67's alias resolution proves brittle.
- **Add `.files(..)` to every app.** It fixes four instances and leaves the class open. Rejected.
- **CrabNebula's WebDriver for macOS.** CrabNebula offers a cross-platform fork of tauri-driver that also drives macOS,
  but it needs a paid API key on macOS. This was checked on 2026-11-03 against Tauri's WebDriver page
  (<https://v2.tauri.app/develop/tests/webdriver/>, read that day). It was rejected because it is a commercial service
  for a personal family of apps, a network dependency inside the gate (the family is local-first and online only by
  opt-in), and a second driver beside tauri-driver. The in-app `--smoke` mode needs neither. The same page also describes
  an embedded WebDriver server inside the app, which is the same idea as `--smoke`; WP-8 evaluates it before building its
  own.
- **More browser e2e.** The mock backend has no Tauri state, guard or native events, so it cannot see this class (the
  lesson of 2026-11-01).
