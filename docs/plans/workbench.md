# Plan: the Paddock app shell and the workbench

- Status: Plan for ADR 0012 (app shell) and ADR 0013 (workbench), both Accepted (2026-10-24): the full shell is built first, then Pitwall adopts it; D1–D6 as recommended
- Source of the inventory: Debrief branch `integrate/redesign` at `5c1ebcda` (Debrief 1.1 candidate), read-only; Pitwall,
  Pitcrew, Spotter and Dyno `main` on 2026-10-24
- Order (decided): Debrief 1.1 → **Pitwall release builds the shell and adopts it first** → **Pacenotes starts by
  extracting the workbench; Debrief moves onto shell and workbench** → Sighter on both. Pitcrew, Spotter and Dyno adopt
  the shell when each is next worked on.

## 1. Two layers

```
 Debrief      Pacenotes     Sighter          Pitwall   Pitcrew   Spotter   Dyno
 ───────────── workbench (ADR 0013) ─────────     (Dyno later, optional)
 dock + planLayout · activity bar · Explorer · palette · pop-outs · AI pane
 ───────────────────────────── app shell (ADR 0012) ──────────────────────────────
 frame · startup + watchdog · notifications + health · dialog kit · jobs · downloads
 settings · about/updates/what's new · native menu + keys · pages frame · tray · theme · i18n · test kit
 ───────────── @decryp7/paddock-ui (tokens, components, planLayout) · paddock-jobs ─────────────
```

## 2. Inventory: Debrief's shell code to extract

Lines are non-test source lines (TypeScript templates are inline); "spec" is the test lines that move with it.
"Coupling to cut" lists the Debrief-specific imports that must become contributions, ports or app code.

### 2.1 Shell layer (ADR 0012)

| Debrief module | Lines (spec) | Goes to | Coupling to cut |
|---|---|---|---|
| `app/src/app/health/` (NotificationHost, chip, store, WaitingFor) | 661 (608) | `paddock-ui/shell/notify` | none (already domain-free by ADR 0055) |
| `app/src/app/core/service-health/` | 141 | `shell/notify` (port + Tauri + mock) | `debrief://` event names |
| `app/src/app/notifications/` (store, panel, actions, source) | 1,207 (1,264) | `shell/notify` (generic half) | `notice-actions.ts` names Debrief actions (open view, open project): actions become `defineNotificationKind` contributions; imports of `jobs`, `ai`, `dock` |
| `app/src-tauri/src/health/{model,registry}.rs` | ~700 of 1,705 | `paddock-health` | none (written to move); `hub.rs` and the five services stay |
| `app/src-tauri/src/notification_centre/` | 1,456 | `paddock-health::notify` | `NoticeSource` lists Debrief sources: becomes an app-defined string id |
| `app/src/app/dialog-windows/` + `ui/dialog.service.ts`, `dialog-host.ts`, `confirm-dialog.ts` | 1,971 + 339 (1,105) | `shell/dialogs` | `dialog-registry.ts` (590, the 51 entries) stays; `dialog-shell.ts` and `tauri-dialog-backend.ts` import `popout`, `windowing`, `tools`, `projects`: host-command forwarding becomes a port |
| `app/scripts/check-dialog-commands.mjs` | (script) | package bin `paddock-check-dialog-commands` | Debrief's `HOST_ONLY_SERVICES`, `DIALOG_SYNC_COMMANDS` become config passed in |
| `dialog_windows.rs`, `dialog_contracts.rs`, `dialog_placement.rs` | 1,175 + 239 + 500 | `paddock-dialogs` | `Dialogs` state keyed by Debrief window labels (`d-<host>-<n>`): label scheme becomes shell-defined |
| `app/src/app/core/startup/`, `lifecycle/recovery.ts`, `lifecycle/ui-state.ts` | 246 + 173 | `shell/startup` | the checkpoint content (query, filters) is Debrief's: a `UiCheckpoint<T>` contribution |
| `startup_watchdog.rs` | 629 | `paddock-startup` | watched-label list, Safe Mode restart hook: builder options |
| `app/src/app/jobs/` | 1,723 (1,826) | `shell/jobs` (store, list, status item, follow/wait/cancel helpers) | `jobs-panel.ts`, `job-viewer.ts` import `ai`; `mock-jobs.ts` (422) has Debrief jobs: fixtures move to the app |
| `app/src-tauri/src/jobs/` | 4,265 | split: queue/model/persist generic into `paddock-jobs` (+ `tauri` adapter); Debrief job kinds stay | ADR 0008 step 2 (Debrief adopts `paddock-jobs`) has to be done first or in the same package |
| `app/src/app/settings/` (screen, model, field, store, dialog, themed controls) | ~1,600 of 5,216 | `shell/settings` | `settings-model.ts` names Debrief pages (video, parsers, storage, keyboard): nav groups become `defineSettingsPage` contributions; parsers/storage/performance pages stay in Debrief |
| `debrief_core::settings` types, `config_store.rs`, generic half of `settings.rs` | ~350 + 374 + ~600 of 1,537 | `paddock-settings` | `builtin_sections.rs` (Diagnostics, Workspace, Plugins sections) stays |
| `app/src/app/updates/`, `about/`, `whats-new/` | 748 + 296 + 295 | `shell/about` | AI runtime/model updates in `updates/` stay in Debrief (an app section on the Updates page) |
| `updater/` + `updater.rs` | 1,783 + 567 | `paddock-updater` (+ `paddock-download`) | `section.rs` (Debrief's settings section for updates) becomes generic |
| `legal.rs` | 44 | `paddock-shell` (About: licences) | none |
| `shell/menu/` (model, menu bar, native menu, app menu service, submenu, recent opener) | 940 | `shell/menu` | `menu-model.ts` imports `ANALYSIS_MENU_ORDER`, `ViewCategory`, `ThemeId`; menus become contributions; recent files are an app contribution |
| `menu.rs` | 1,007 | `paddock-menu` | `debrief://menu` event name |
| `extensions/keybinding*.ts`, `command-registry.ts`, `command-context.ts` | ~600 | `shell` (contributions core) | `CommandContext` has Debrief methods (`setQuery`, `addFilter`, `selectRecord`, `jumpTo`): split into `ShellContext` + Debrief's context |
| `shell/title-bar.ts`, `shell/status-bar.ts`, `shell/edit-commands.ts` | 119 + 146 + 62 | `shell` (frame) | status bar hard-codes events/matching/profile/live tail/import/AI chip: these become `defineStatusItem` contributions in Debrief |
| `native_feel.rs`, `instance.rs`, `tray.rs`, `core/native-feel.ts`, `core/platform.ts` | 117 + 290 + 458 + 252 + 137 | `paddock-window`, `shell` | tray menu items are Debrief's: a tray model contribution |
| `theme/` | 1,291 | appearance stays in `paddock-ui`; theme registry in `shell` | Debrief's four themes become Debrief `defineTheme` packs |
| `i18n/` | 270 (223) | `shell/i18n` | the loader table is the app's |
| `ui/` (context menu host and service, error details, next steps, overwrite guard, help hint) | ~500 of 825 | `shell` (generic widgets) | `kind-filter.ts` stays in Debrief |
| `commands.rs` + `command_table!` in `lib.rs` | 553 + ~150 | `paddock-commands` | the 300-odd Debrief entries stay; window classes become shell-defined |
| `logging.rs`, `paths.rs`, `app_dirs.rs` | 622 + 801 + 127 | `paddock-window` / `paddock-env` (ADR 0006 row) when needed by a second app; not in the first shell release | data-folder and log names are per app |
| e2e helpers `e2e/app.ts`, `axe.ts`, `mock-url.ts`, `menu-support.ts`, `retried-reporter.mjs` | 257 + reporter | `@decryp7/paddock-e2e` | `app.ts` knows Debrief's welcome page and mock URL params |

Shell layer total: about 13,000 lines of TypeScript (plus 8,000 of spec) and 11,000 of Rust move or are generalised.

### 2.2 Workbench layer (ADR 0013)

| Debrief module | Lines (spec) | Goes to | Coupling to cut |
|---|---|---|---|
| `dock/` (39 files: `dock.service.ts` 976, `dock-host.ts` 404, `dock-placement.ts` 394, `dock-layout.ts` 335, `perspectives.ts` 255, `angular-panel.ts` 254, ...) | 5,680 (4,745) | `workbench/dock` | imports `extensions` (18), `core` (11): `DebriefView` → `defineView`; `WorkspaceSummary`-based `when` → typed context; Debrief perspectives (`profiling`) → `definePerspective` |
| `layout-plan/` | 538 (327) | `paddock-ui` root (`planLayout`, pure; ADR 0011) + `paddock-e2e` (scenario runner) | none (self-contained by ADR 0056) |
| `popout/` | 2,514 (1,965) | `workbench/panels` | imports `core` (16), `dock` (14), `live`, `analysis`, `settings`: `shared-state.ts` syncs Debrief's query/selection, which becomes a `defineSharedState` contribution |
| `panel_windows.rs`, `drag_ghost/`, `ui_dispatch.rs` | 913 + 1,256 + 101 | `paddock-panels` | label scheme, `debrief://` events |
| `shell/activity-bar.ts`, `activity-overflow.ts`, `extensions/activity-items.ts` | 501 | `workbench/activity` | built-in items are Debrief's: they stay as contributions |
| `shell/sidebar.ts`, `sidebar-sections.ts`, `explorer-names.ts`, `regions.ts`, `narrow-window.ts` | 732 | `workbench/explorer`, `workbench/frame` | `sidebar.ts` (432) is mostly Debrief's Sources tree (sources, live tail, notes, encodings, AI, parsers): it becomes Debrief's `Sources` Explorer provider; only the frame moves |
| `palette/`, `extensions/fuzzy.ts`, frame of `search-everything/` | 467 + 33 + ~300 | `workbench/palette` | search sources (events, fields, settings) are Debrief palette providers |
| `extensions/` (tokens, view registry, provide, events, index) | ~550 | `shell` + `workbench` contributions | `DEBRIEF_*` tokens → generic tokens; `inspector-sections`, `field-formatters` stay in Debrief |
| `windowing/` generic half (service, window title, close prompt, open-ask, background work) | ~500 of 991 | `workbench/windows` | `open-ask`/`close-prompt` talk about projects: "unsaved state" becomes a port |
| `ai/chat/` frame, `ai-chat-persistence.ts`, needs-AI states | ~1,200 of 1,937 | `workbench/assistant` | tool cards for Debrief tools (queries, filters) stay as Debrief renderers of tool results |
| `debrief_ai::llm::prompted`, Pitwall half of `debrief_ai::engine` | (crate) | `paddock-assistant` | Debrief tool schemas stay; the built-in llama.cpp adapter stays in Debrief |
| `app.ts` frame, `jobs/jobs-panel.ts` as a view | 98 + 316 | `workbench/frame` | `FilterBar`, `Welcome`, `ImportToast`, `TourOverlay` are Debrief slots |

Workbench layer total: about 12,000 lines of TypeScript (plus 9,000 of spec) and 2,500 of Rust plus the assistant crate.

## 3. Target layout in the Paddock repo

```
ui/projects/paddock-ui/
  src/                       existing root entry: tokens, components, appearance, echarts, pd-job, planLayout (moved here)
  shell/                     @decryp7/paddock-ui/shell          contributions core, ShellContext, frame, status items, widgets
  shell/startup/  shell/notify/  shell/dialogs/  shell/jobs/  shell/settings/  shell/about/
  shell/menu/  shell/pages/  shell/i18n/  shell/store/  shell/testing/
  workbench/                 @decryp7/paddock-ui/workbench      frame, contributions (views, activity, explorer, palette, perspectives)
  workbench/dock/  workbench/panels/  workbench/palette/  workbench/explorer/  workbench/assistant/  workbench/testing/
  bin/                       paddock-check-dialog-commands, paddock-i18n
ui/projects/paddock-e2e/     @decryp7/paddock-e2e (Node, Playwright helpers and page objects)
ui/projects/shell-demo/      reference app on the pages frame (mock backend; screenshots; CI)
ui/projects/workbench-demo/  reference app on the workbench (mock backend; layout scenarios; CI)
ui/projects/gallery/         unchanged
crates/
  paddock-jobs/  paddock-commands/  paddock-health/  paddock-dialogs/  paddock-startup/  paddock-download/
  paddock-settings/  paddock-updater/  paddock-window/  paddock-menu/  paddock-shell/           (shell)
  paddock-panels/  paddock-assistant/  paddock-workbench/                                        (workbench)
  paddock-pitwall/                                                                               (ADR 0004; needed by paddock-assistant)
```

Each crate: pure core, `tauri` feature for its adapter, `>= 99%` lines, `cargo check --target x86_64-pc-windows-msvc`,
Rust tests on Linux, macOS and Windows runners (lesson: platform-only bugs). `dockview` (`^8`) is an optional peer of
`paddock-ui`, needed only by `workbench/dock`.

## 4. Work packages

One agent each; sizes are agent-days of focused work including tests (rough, ±50%). Owned files are in the Paddock repo
unless marked. Every package: its unit tests, the affected reference-app e2e in Chromium (rule 3), lint and type-check,
`>= 99%` Rust lines, and an update of `CHANGELOG.md` (Unreleased). "Extract" means copy the Debrief 1.1 code with its
tests (read-only from `integrate/redesign` or the 1.1 tag), remove the coupling listed in section 2, and rename
`debrief://` to `paddock://`.

### 4.1 Shell layer: built in the Pitwall release (Pitwall adopts it first)

| # | Package | Owned files | Depends on | Acceptance | Days |
|---|---|---|---|---|---|
| S0 | Scaffold: entry points, crate members, CI jobs per crate and entry point, `shell-demo` app, Gitea cargo registry publish job (D3) | `ui/angular.json`, `ui/projects/shell-demo/`, `ui/projects/paddock-ui/shell/**/ng-package.json`, `crates/Cargo.toml`, `.gitea/workflows/ci.yml`, `publish` workflow | - | `shell-demo` builds and serves; a dry-run publish lists every package and crate; CI runs each crate on three OSes | 1.5 |
| S1 | `paddock-commands`: table macro, window classes, limits, async-only check, guard hook, generated TS command names (`ts-rs`) | `crates/paddock-commands/` | S0 | table tests ported from Debrief; a sync command fails to compile or test; stale generated TS fails CI | 1.5 |
| S2 | Contributions core: `define*` for commands, keybindings, menus, pages, status items, settings pages, dialogs, health services, notification kinds; `provideContributions`, `ShellContext<T>`, manifest writer and checker, `assertContributionsValid` | `shell/` (contributions), `shell/testing/` (lint) | S0 | lint fails on each listed defect (one test per defect); manifest round-trip test | 2 |
| S3 | `paddock-health`: model, registry, cause-key notification centre, persistence, `tauri` adapter | `crates/paddock-health/` | S1 | Debrief's health and centre tests ported; "one failure is one chip and one notification in two windows" with a fake service | 1.5 |
| S4 | `shell/notify` + `shell/store`: `NotificationHost`, sources, `ServiceHealthStore`, chip, `WaitingFor`, `withRequestStatus`, ports with Tauri and mock backends | `shell/notify/`, `shell/store/` | S2, S3 | ADR 0055 specs ported; health cause never toasted; axe clean in both themes in `shell-demo` | 1.5 |
| S5 | `paddock-dialogs`: contracts, `allows`, placement (pure, table tests), window creation by kind, fit/grow, bounds | `crates/paddock-dialogs/` | S1 | placement table tests incl. mixed-scale monitors; a denied command logs dialog id + command | 2 |
| S6 | `shell/dialogs`: kit, typed `DialogService`, `DialogShell`, in-page backend, `ModalGate`, bounds, keys, per-window bootstrap, boot-every-dialog harness, `paddock-check-dialog-commands` bin | `shell/dialogs/`, `bin/` | S2, S5 | `shell-demo` has one dialog per kind; harness boots all and fails on a planted missing provider and a planted undeclared command | 2.5 |
| S7 | `paddock-startup` + `shell/startup`: watchdog, ticker, native ask, boot errors, `retryUntilAnswered`, `provideStartupContract`, checkpoint port, splash | `crates/paddock-startup/`, `shell/startup/` | S1, S2 | ADR 0058 tests ported (pure watchdog with fake clock); e2e: withheld IPC still paints and reports ready | 1.5 |
| S8 | `paddock-download`: segmented Range, per-host caps, 429/`Retry-After`, resume, in-order hash while writing, phases | `crates/paddock-download/` | `paddock-jobs` | tests against an in-process HTTP server: no-Range host, 429, truncated segment, digest mismatch, cancel within 250 ms; benchmark vs single stream | 2 |
| S9 | Jobs: `paddock-jobs` `tauri` adapter (`jobs_snapshot`, `job_cancel`, sink) + `shell/jobs` (store, list popover, status item, follow/wait/cancel) | `crates/paddock-jobs/src/tauri.rs`, `shell/jobs/` | S1, S2 | "Still working" after N s without events; cancel from list; no UI timeout anywhere (grep check) | 1.5 |
| S10 | `paddock-settings` + `shell/settings`: section schema, validation, versioned file, generic field renderer, Settings `tool` window with contributed pages | `crates/paddock-settings/`, `shell/settings/` | S2, S6 | corrupt file → defaults + one notification; every field kind rendered and validated in Rust; keyboard and axe | 2.5 |
| S11 | `paddock-updater` + `shell/about`: state machine over `paddock-download`, manifest and signature, About (versions, licences), Updates, What's New | `crates/paddock-updater/`, `shell/about/` | S6, S8 | updater state tests (Debrief's); What's New reads the version from the engine (rule 6); bad signature refused | 2.5 |
| S12 | `paddock-window`: window state, single instance, deep links, materials, tray popover window | `crates/paddock-window/` | S1 | ported tests; tray popover fitted height and close on blur; no real Login Items or windows in tests (rule 7) | 2 |
| S13 | `paddock-menu` + `shell/menu`: one model to native and in-app menu, diff, routing, keybinding registry with user overrides and conflict check | `crates/paddock-menu/`, `shell/menu/` | S2 | Debrief menu tests ported; macOS native spec snapshot; overrides persisted and conflicts reported | 2 |
| S14 | `shell/pages` + frame: `pd-app-frame` (title bar, sidebar nav 216/56, routed pages, status items, scrim, regions), narrow-window by `planLayout`, `planLayout` moved to root entry | `shell/pages/`, `shell/` frame, `src/lib/layout-plan.ts` | S2, S4, S9, S13 | `shell-demo` at 320 px: no horizontal scroll; sidebar collapses by plan; screenshots both themes | 2 |
| S15 | Theme and i18n: theme registry, Pacenotes and Sighter accents with token tests, `shell/i18n` boot, `@@pd.` messages, `paddock-i18n merge`, pseudo-locale spec | `scss/_tokens.scss`, `test/tokens`, `shell/i18n/`, `bin/` | S2 | token tests green for seven accents; pseudo-locale run of `shell-demo` shows no untranslated shell string | 1.5 |
| S16 | Test kit: `@decryp7/paddock-e2e` (port, Chrome for Testing guard, axe, retried reporter, page objects for shell parts), `provideShellTesting`, fixtures from Rust round-trips | `ui/projects/paddock-e2e/`, `shell/testing/` | S4, S6, S9, S10, S13 | `shell-demo`'s e2e uses only the kit; the kit refuses the installed Chrome; fixtures decode in Rust and TS | 2 |
| S17 | `paddock-shell` plugin + builder, `shell_version` check, permission sets, docs (`ui/README.md`, crate READMEs), release 0.2.0 | `crates/paddock-shell/`, docs | S1-S16 | a Tauri test app on `MockRuntime` registers every shell command; version-mismatch test; tag dry run green | 1.5 |

Shell total: **about 34 agent-days** (with up to five agents in parallel: S1, S2, S8, S12, S15 start at once; about two
to three calendar weeks).

**Pitwall adoption** (Pitwall's own work packages in its release, after S17 or package by package as each lands):

| # | Step | Days |
|---|---|---|
| P1 | Frame and pages: `pd-app-frame` replaces `shell/sidebar.ts` (327); pages as contributions; status items (serving, queue, memory) | 1 |
| P2 | Startup contract + watchdog; splash | 0.5 |
| P3 | Notifications + health: `core/toasts.ts` and per-page service states → `NotificationHost`; services `llm` (engine), `server` (the background service), `downloads`; Repair via `service_repair.rs` behind `health_repair` | 1.5 |
| P4 | Dialogs: `create-app-dialog`, `key-secret-dialog`, `quit-prompt`, `service-consent`, `confirm` → kinds and contracts (credentials and consent `decision`/`form` modal) | 1.5 |
| P5 | Jobs and downloads: engine setup and library downloads → `paddock-download` + jobs with phases (ADR 0008 step 3); remove `timeout-backend.ts` for jobs | 1.5 |
| P6 | Settings, About, Updates, What's New; `updater.rs`, `native_feel.rs`, `window.rs`, `tray.rs`, `app_menu.rs` deleted for the shell crates | 1.5 |
| P7 | Full gate, screenshots both themes, real-app smoke on three OSes | 1 |

Pitwall total: **about 8.5 agent-days**.

### 4.2 Workbench layer: at Pacenotes' start

| # | Package | Owned files | Depends on | Acceptance | Days |
|---|---|---|---|---|---|
| W0 | Debrief baseline (in Debrief, before any move): screenshot matrix of shell and workbench states (both themes; 1440x900, 1920x1080, 800x600; default, Full and profiling perspectives; every dialog kind; health chip down; jobs running; palette open; a pop-out), stored as Playwright snapshots; list of e2e specs that cover the shell | Debrief `app/e2e/shell-baseline.e2e.ts` and snapshots | shell released | baseline stable over three runs in Chromium and WebKit | 1.5 |
| W1 | Workbench contributions: `defineView`, `defineActivityItem`, `defineExplorerProvider`, `definePaletteProvider`, `definePerspective`, `defineSharedState`, `defineAssistantContext`; registries; lint additions | `workbench/` (contributions), `workbench/testing/` | S2 | lint catches a view without layout and `min > pref` | 2 |
| W2a | Dock core: dockview host, `planLayout` adapter, `setConstraints` per group, persistence (versioned, fit on load with Undo), named layouts | `workbench/dock/` | W1, S14 | Debrief's planner and dock specs ported; ADR 0011 scenario table green in `workbench-demo` | 2.5 |
| W2b | Dock interactions: auto-hide, floating, drop guides, drag-out cue, switcher, group actions, keys, perspectives | `workbench/dock/` | W2a | ported specs; e2e for float, auto-hide and perspective switch | 3 |
| W3 | Pop-outs and multi-window: `paddock-panels`, `workbench/panels`, shared-state forwarding, drag ghost, generic windowing (titles, close prompt port) | `crates/paddock-panels/`, `workbench/panels/`, `workbench/windows/` | W2a, S5 | pop-out on a second display clamped to its work area (pure tests); host close closes pop-outs; no window left behind in tests | 3 |
| W4 | Frame, activity bar, Explorer, regions, narrow window, status items in the workbench frame, `workbench-demo` | `workbench/frame/`, `workbench/activity/`, `workbench/explorer/`, `ui/projects/workbench-demo/` | W1, W2a | F6 cycles regions; Explorer closes by plan below its minimum; activity overflow | 2 |
| W5 | Command palette and palette providers (fuzzy, recent, did-you-mean, abortable providers) | `workbench/palette/` | W1 | ported specs; a slow provider is cancelled on the next keystroke | 1.5 |
| W6 | `paddock-assistant` + `workbench/assistant`: port, prompted protocol, tool loop with budgets and cancel, conversation persistence, Pitwall adapter, chat pane, needs-AI state, context chips | `crates/paddock-assistant/`, `workbench/assistant/` | `paddock-pitwall` (ADR 0004), W1 | fake-Pitwall contract tests (pairing refused, version mismatch, dies mid-stream, slow); Stop within 100 ms; text never bound as HTML | 4 |
| W7 | Workbench test kit and plugin: dock/palette/Explorer/assistant page objects, scenario runner; `paddock-workbench` plugin; release | `ui/projects/paddock-e2e/`, `crates/paddock-workbench/` | W2-W6 | `workbench-demo` e2e uses only the kit; tag dry run | 1.5 |

Workbench total: **about 21 agent-days** (plus `paddock-pitwall` if not built yet: ADR 0006 puts it first after the
Debrief release; about 4 days).

### 4.3 Debrief's migration (Debrief work, no behaviour change)

Debrief adopts the shell and the workbench **together**, at Pacenotes' start (decision D4), so Debrief is touched once
and rule 1 holds during the Pitwall release. Until then Debrief's shell folders are frozen: only bug fixes, each also
filed against the Paddock copy.

| # | Step | Days |
|---|---|---|
| M1 | Contributions: `DEBRIEF_*` tokens → `provideContributions`; `CommandContext` → `ShellContext & DebriefContext`; status bar items, menus, settings pages, notification kinds, theme packs, checkpoint, shared state as contributions | 3 |
| M2 | Shell services: notify/health, dialogs (registry entries keep their ids; the contract file is regenerated), startup, jobs, settings, about/updates/what's new, menu, i18n merge; delete the moved folders | 3 |
| M3 | Workbench: dock, pop-outs, activity bar, Explorer (Sources provider), palette (Debrief providers), assistant pane (Debrief tools, built-in engine adapter kept) | 4 |
| M4 | Rust: `paddock-shell` and `paddock-workbench` plugins; Debrief's table entries for moved commands deleted; events renamed; capabilities grant the shell sets; delete `health/{model,registry}`, `notification_centre`, `dialog_*`, `startup_watchdog`, `menu.rs`, `updater/`, `panel_windows.rs`, `drag_ghost/`, `ui_dispatch.rs` | 2 |
| M5 | Safety net: W0 snapshots (pixel diff with a small tolerance; every difference explained or fixed), the full e2e suite in Chromium and WebKit, real-app smoke on three OSes (`--self-test`, Quit within 10 s), the dialog boot harness and command lint, the layout scenario table | 1.5 |

Debrief total: **about 13.5 agent-days**. Plugin UI contract: `UI_PLUGIN_API_VERSION` 3 (views keep `layout`).

### 4.4 Pacenotes and Sighter first screens on the workbench

**Pacenotes** (v1: core, Understand, Review, Debrief bridge):

| Contribution | Pacenotes items |
|---|---|
| Activity items | Explorer (files), Symbols, Search, Review, Graph, Assistant (bottom: Settings) |
| Explorer providers | Files (index tree, git status badges), Symbols outline of the active file, Repositories (open folders, trust state) |
| Views | Orientation (document, priority 100, the first screen after indexing), Code (document, read-only viewer, instances, min 480 wide), Dependency graph (document), Search results (analysis), References and Call path (analysis), Review findings (side, min 360), plugin `ViewDoc` views (`pd-view`), Jobs (status), Assistant (side, min 320, pref 400) |
| Palette providers | Go to file, `@` go to symbol, `#` search text, `>` commands |
| Perspectives | Understand (Explorer, Code, Orientation, Assistant), Review (diff, findings, Code), Graph |
| Commands and menus | Open folder, Trust this repository, Reindex, Find references, Trace call path, Run review, Export SARIF, Open in editor at line |
| Status items | index progress (job), language tier per language, MCP server (clients connected), Pitwall |
| Health services | index store, parser workers, language servers (one entry per language), MCP server, vector index, Pitwall (optional: degraded, never down) |
| Dialogs | `trust-repository` (decision, modal), `pack-manager` (tool), `mcp-clients` (tool: keys and grants), `review-setup` (form, modeless; the run is a job), `about`/`updates`/`whats-new` (shell) |
| Settings sections | Index (limits, ignore), Language packs, Semantic search (on/off, Low memory), MCP, Review rules, Pitwall |
| Jobs | index (tiers as phases), review, diagram generation, pack download (`paddock-download`) |
| Assistant tools (Rust) | `repo_map`, `search`, `read_symbol`, `references`, `call_graph` (read-only; the same functions as the MCP tools) |

**Sighter** (studio):

| Contribution | Sighter items |
|---|---|
| Activity items | Recordings, Label, Review queue, Models, Playground, Runs (bottom: Keys and permissions, Settings) |
| Explorer providers | Project → sessions → distinct frames (thumbnails, flag counts) |
| Views | Frame canvas (document, priority 100, min 640x400), Filmstrip / frame timeline (analysis, min height 120), Review queue (side, priority 80, min 360), Inspector (side), Playground (document), Runs (document), Model metrics (document), Assistant (side; agent mode later) |
| Palette providers | Go to frame, go to element class, commands |
| Perspectives | Label (canvas, filmstrip, queue, inspector), Review, Playground |
| Status items | sighter-server (port, keys), model version, GPU/VRAM, recording indicator |
| Health services | sighter-server, recorder helper, training worker, OCR/detector runtime, Pitwall (optional), screen-recording and accessibility permission (Repair opens the right OS Settings pane; lessons) |
| Dialogs | `start-recording` (form, modal), `permissions` (decision), `train-model` (form, modeless; training is a job), `api-keys` (tool), `export-dataset` (form, modeless) |
| Settings sections | Recording and redaction, Server, Models and training, Safety (agent rules), Pitwall |
| Jobs | scene detection, training (phases: prepare, train, evaluate, export), dataset export, model download |

### 4.5 Pitcrew, Spotter and Dyno: shell adoption when each is next worked on

| App | Steps | Days |
|---|---|---|
| Pitcrew | `layout/` (409) → `pd-app-frame`; `ToastStore` + `connection-banner.ts` → `NotificationHost` + a `service` health entry with Repair (`service_repair.rs`); startup contract (fixes the known blank reopened window); confirmations and `register-wizard` → `decision`/`form` dialogs; cleanup scan, disk map and deletes as jobs, remove `call-timeout.ts` for jobs (ADR 0008 step 4); `updater.rs`, `native_feel.rs`, `tray.rs` → shell crates; settings sections | 5 |
| Spotter | `layout/` (443) → frame; `layout/toasts.ts`, `toast.rs`, `notifier.rs`, connection banner → notify/health; `confirm-dialog.ts` → `decision`; `jobs.rs` → `paddock-jobs` + shell jobs (check-all, release download with hash while writing, Chromium install); `tray.rs` (664) → tray popover host; updater, native feel | 4 |
| Dyno | `shell/` (576) → frame with collapsible sidebar; `notifier.ts` → notify; collectors and plugins → health services (one per collector; silent failures become chips); `rule-dialog`, `fix-dialog`, `process-dialogs`, `web-search-dialog` → kinds; popover → tray popover host; `window.rs`, `updater.rs`, `native_feel.rs` → shell crates; dashboard tiles declare min/pref for `planLayout` | 6 |
| Dyno to workbench (later, when its dashboards or task manager become arrangeable views: three or more views wanted at once, or charts popped out to another display) | tiles and task manager as views, Explorer for machines/sensors, perspectives per dashboard | 4-6 |

## 5. What the Pitwall release must extract, and in what shape

The shell is built from Debrief's code during the Pitwall release; for the workbench to build on it later without a
second redesign, each part must already have this shape:

1. **No app name anywhere**: commands, events (`paddock://`), window labels, storage keys, message ids (`@@pd.`), log
   targets. The window label scheme (`main`, `w-<n>`, `d-<host>-<n>`, `p-<host>-<n>`) is defined by the shell, including
   `p-` for pop-outs, even though Pitwall has none, so `paddock-panels` slots in.
2. **Contributions, not configuration objects**: Pitwall registers its pages, status items, menus, dialogs and health
   services through the S2 registries; the workbench adds kinds to the same registry (W1) and never a second mechanism.
3. **Generic context**: `ShellContext<T>` from the first release, so Debrief's and Pacenotes' contexts extend it.
4. **Window classes in `paddock-commands` include `POPOUT`** and the guard handles per-dialog contracts, so the workbench
   plugin is built by the same macro.
5. **Frame with slots**: `pd-app-frame` and the workbench frame share the title bar, status bar, scrim and regions
   components; the pages frame is a body variant, not a fork.
6. **`planLayout` in the root entry** (pure), used by the pages frame for the sidebar; the dock adapter later uses the same
   function.
7. **Status items have area and priority** (they are laid out by priority when the bar is narrow), so the workbench's
   status bar is the same component.
8. **Jobs list is a component** that the pages frame shows in a status-bar popover and the workbench hosts as a view.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Debrief's migration changes behaviour unnoticed (30,000 lines move) | W0 baseline first; M5 pixel diff plus full e2e both browsers plus real-app smoke; migration in four steps, each green |
| Extracted code drifts from Debrief's 1.1 copy while Debrief waits | freeze Debrief's shell folders; every fix filed against both; extraction from the 1.1 tag, diffed again at M1 |
| Cross-repo churn: every shell fix needs a release and a bump in several apps | reference apps and the nightly consumer canary; patch releases from the tag workflow; Renovate one PR per release |
| Over-generalising from one app (Debrief) | the Pitwall release is the second consumer of the shell, Pacenotes of the workbench; a contribution kind is added only when a second app needs it |
| Contribution functions (`when`, `badge`) become a performance problem with hundreds of items | evaluated as `computed` over the context, defensively; a perf budget for palette open and menu build in the benchmark workflow (rule 16) |
| dockview major upgrade breaks every IDE app at once | peer dependency pinned to `^8`; upgrade only in a Paddock release with the scenario table green |
| Pitwall release grows by a shell and slips | the shell packages are independent; Pitwall can adopt S3/S4, S5/S6, S7, S8/S9 first (the user's original list) and the rest (settings, updater, menu, window) in the same release as they land |
| Assistant pane depends on `paddock-pitwall`, not built yet | build `paddock-pitwall` (ADR 0004) before W6; W1-W5 do not need it |
| Command renaming breaks dialog contracts and mocks | contracts and command names are generated; the lint and the boot harness fail on any mismatch |
| Bundle size of manager apps grows | secondary entry points; each app records its initial chunk and keeps the budget (rule 35) |

## 7. Effort summary

| Block | Agent-days |
|---|---|
| Shell layer in Paddock (S0-S17) | ~34 |
| Pitwall adoption (P1-P7) | ~8.5 |
| `paddock-pitwall` (ADR 0004), if not built yet | ~4 |
| Workbench layer in Paddock (W0-W7) | ~21 |
| Debrief migration (M1-M5) | ~13.5 |
| Pitcrew / Spotter / Dyno shell adoption, when next worked on | ~5 / ~4 / ~6 |
| **Total before Pacenotes' own features** | **~77 (shell, Pitwall, workbench, Debrief), plus ~15 for the three manager apps later** |

With the five-agent limit on the Mac mini: the shell is about three calendar weeks, the workbench plus Debrief's
migration about three more.

## 8. Decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| D1 | Packaging: one `@decryp7/paddock-ui` with secondary entry points (`/shell/*`, `/workbench/*`), or separate `@decryp7/paddock-shell` and `@decryp7/paddock-workbench` packages | **One package with entry points** (plus `@decryp7/paddock-e2e` for Node): one version, one install line, rule 39 as written; tree-shaking keeps dockview out of manager apps |
| D2 | Rust delivery: shell and workbench as Tauri plugins (`plugin:paddock-shell|...`) built with the shared command-table macro, or library crates spliced into each app's own table | **Tauri plugins**: adopting is one line, an app cannot forget a command, permissions are generated; the same macro keeps one guard |
| D3 | Crate consumption: switch to the Gitea cargo registry with the first shell release, or stay on git tags | **Registry now**: ten-plus crates per app; per-crate bumps; publish only from the tag workflow |
| D4 | When Debrief moves: shell and workbench together at Pacenotes' start (Debrief frozen meanwhile), or the shell right after the Pitwall release and the workbench later | **Together at Pacenotes' start**: Debrief is touched once and one app at a time (rule 1) holds during the Pitwall release |
| D5 | Assistant pane scope: the workbench owns the chat UI and tool loop over Pitwall only, Debrief keeps its built-in llama.cpp engine as its own adapter; or the built-in engine moves to Paddock too | **Pitwall only in Paddock**: Pacenotes and Sighter use Pitwall by design; the built-in engine is Debrief's fallback (Debrief ADR 0059) |
| D6 | i18n in manager apps: adopt `$localize` with the shell (English only shipped), or keep plain strings there | **Adopt with the shell**: the shell's strings are already ids; app strings move when each page is next touched |

## 9. Input from Debrief 1.1's start-up work (2026-10-24)

Measured on Debrief 1.1: about 780 kB of the main window's first-paint JavaScript is the ~50 `provide*()` calls in
`app.config.ts`, each statically importing its stores, commands and start-up work, and pop-out windows boot the whole app
config. So the contribution model (S2, W1) must make features lazy by construction: a contribution is a manifest of
command, view and menu stubs plus a loader; its stores and services load on first use or when idle; a lint stops a
contribution's registration file from importing store or command classes; and each view declares the providers it needs,
so pop-outs and dialog windows compose only those. Acceptance for S2/W1 and Debrief's M1: the main window's first-paint
JavaScript drops well below today's 1233 kB, measured by Debrief's `scripts/window-weight.mjs`.
