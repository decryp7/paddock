# 12. Paddock app shell: one shell for every app

- Status: Accepted (2026-10-24; user: the full shell is built first and Pitwall then adopts it; decisions D1–D6 as recommended)
- Date: 2026-10-24
- Related: rules 10-12, 17-19, 21, 27-40; ADRs 0006 (versioning, consumption), 0008 (`paddock-jobs`), 0009 (dialogs),
  0010 (notifications and service health), 0011 (layout); ADR 0013 (the workbench, built on this shell); Debrief ADRs
  0050, 0054, 0055, 0056, 0058; plan: [docs/plans/workbench.md](../plans/workbench.md)
- Users: Pitwall (first adopter, in its next release), then Debrief, Pacenotes, Sighter, Pitcrew, Spotter, Dyno

## Context

Every Paddock app has the same frame around its own content, and every app wrote it again:

| Part | Debrief (1.1) | Pitwall | Pitcrew | Spotter | Dyno |
|---|---|---|---|---|---|
| Updater | `updater/` + `updater.rs`, 2,350 lines | `updater.rs` 150 | 134 | 126 | 78 |
| Native feel (window state, title bar) | `native_feel.rs` 117 + `instance.rs` 290 | 123 + `window.rs` 125 | 94 | 94 | 94 + `window.rs` 176 |
| Tray / menu-bar popover | `tray.rs` 458 | 163 + `tray_model.rs` | 379 | 664 | 266 + `popover.rs` |
| Native menu | `menu.rs` 1,007 + `shell/menu/` 940 | `app_menu.rs` 76 | - | - | - |
| Toasts / errors | `NotificationHost` + centre (ADR 0055) | `core/toasts.ts` | `ToastStore` + banner | `layout/toasts.ts` + banner | `shared/notifier.ts` |
| Service health | `health/` 1,705 Rust + 661 TS | per page | connection banner | connection banner | per page |
| Dialogs as windows | 51 dialogs, kinds and contracts (ADR 0054) | ~6 in-page | inline | 1 in-page | ~5 in-page |
| Start-up contract and watchdog | ADR 0058 | - | (blank-window bug) | - | - |
| Jobs and Cancel | `jobs/` 4,265 Rust + 1,723 TS | - | fixed UI timeouts | `jobs.rs` | - |
| Settings | engine-described sections, generic renderer | pages | pages | pages | pages |
| i18n | `$localize`, runtime translations | none | none | none | none |

Debrief's 1.1 versions of these parts are the ones that survived rule 40: they were rebuilt as declared contracts after
recurring bugs (51 dialogs, three toasts per failure, blank windows). The other apps carry the earlier, buggier shapes,
and Pacenotes and Sighter would copy one or the other. ADRs 0008-0011 already moved the *decisions* to the family; this
ADR decides the *one shell* those decisions are delivered in, so an app gets them by adopting the shell rather than by
re-implementing four ADRs.

Two kinds of app share the shell. Manager apps (Pitwall, Pitcrew, Spotter, Dyno) are a sidebar of pages plus a tray
popover. IDE-style apps (Debrief, Pacenotes, Sighter) are docked workspaces. Everything in the table above is common to
both; the dock, activity bar, Explorer, palette and AI pane are only for the second kind and are ADR 0013.

## Decision

### 1. What the shell is

The **Paddock app shell** is the frame and the cross-cutting services of a Paddock desktop app, as one Angular layer and
one Rust layer that are adopted together:

| Area | The shell provides | The app provides |
|---|---|---|
| Window frame | title bar region (macOS inset traffic lights, Windows/Linux in-app menu bar), body, status bar, modal scrim, skip link, F6 regions | its pages or (ADR 0013) its views |
| Start-up | the splash in `index.html`, `provideStartupContract()`, `retryUntilAnswered`, `window_ready`, boot-error forwarding, the Rust watchdog (reload once, then Reload / Safe Mode / Keep Waiting), the UI checkpoint hook (ADR 0058) | which start-up reads must retry; what its checkpoint holds |
| Notifications | one `NotificationHost` with cause keys, one display rule, sources (ADR 0010) | its notification kinds and actions |
| Service health | registry, status-bar chip with Details and Repair, `WaitingFor` panel state, `health_repair` served by the shell (rule 19) | its services, their supervisors and repair actions |
| Dialogs | the four kinds, `defineDialog`, typed `DialogService`, `DialogShell`, placement on the parent's monitor, the per-dialog command guard, the boot-every-dialog harness and the command lint (ADR 0009) | its dialog entries and components |
| Jobs | the `paddock-jobs` Tauri adapter (`paddock://jobs`, `jobs_snapshot`, `job_cancel`), a Jobs list (status-bar popover in the pages frame, a pane in the workbench), `pd-job`, "Still working" (ADR 0008) | the jobs themselves |
| Downloads | segmented Range downloads with per-host caps and 429 back-off, hash while writing, resume (rule 15) | what to download and the expected digest |
| Settings | a Settings `tool` window whose navigation is built from engine-described sections (typed fields, validated in Rust, versioned file with defaults) plus app UI pages | its sections and pages |
| About / Updates / What's New | the three `viewer` windows, the updater state machine, manifest and signature check over the shared downloader, update notifications | feed URL, public key, release notes, licence list |
| Native menu and keys | one menu model feeding the macOS native menu and the in-app menu bar, keybinding registry with user overrides | its menus, commands and default keys |
| Pages layout | `pd-app-frame`: sidebar navigation (216 px, collapses to 56), routed pages, status items; narrow-window decisions through `planLayout` (ADR 0011) | its pages and status items |
| Tray | the tray popover window model and `pd-tray-popover` host (native look per lessons) | its popover content |
| Theme and i18n | `--pd-*` tokens, appearance (theme, accent, density, contrast), app theme registry; `$localize` with runtime-loaded translations, the shell's own messages | its accent, optional theme packs, its messages |
| Test kit | fake backends and fixtures, TestBed helpers, Playwright helpers (see 7) | its specs |

What stays per app: its domain pages or views, its engine, its own commands and their table entries, its services'
supervisors, its notification kinds, its dialog components. The shell has no domain types and imports nothing from an
app (rule: the core never depends on an adapter).

### 2. The contribution model (shared with ADR 0013)

Apps plug into the shell through **typed, declarative registries in TypeScript and in Rust, in process**. There is no
plugin runtime, no activation events and no string `when` expressions; out-of-process plugins stay the separate
platform of ADR 0001, whose views reach the shell only as `ViewDoc`s (ADR 0002).

TypeScript: each kind has a `define*` function that returns a frozen, typed value, and `provideContributions()` registers a
batch through multi-provider tokens (the pattern of Debrief's `extensions/tokens.ts`, generalised):

```ts
provideContributions<AppContext>({
  commands:      [defineCommand({ id: 'models.refresh', title: $localize`:@@pw.models.refresh:Refresh models`, run: (ctx) => ctx.models.refresh() })],
  keybindings:   [defineKeybinding({ command: 'models.refresh', key: 'Mod+R' })],
  menus:         [defineMenuItem({ menu: 'view', command: 'models.refresh', group: 'models', order: 100 })],
  pages:         [definePage({ id: 'models', title: ..., icon: 'pw:model', route: () => import('./models/models-page'), order: 200 })],
  statusItems:   [defineStatusItem({ id: 'serving', side: 'end', priority: 80, component: () => import('./serving-chip') })],
  settingsPages: [defineSettingsPage({ id: '@engines', group: 'application', title: ..., component: () => import('./engines-settings') })],
  dialogs:       [defineDialog({ id: 'create-app', kind: 'form', modality: 'modal', commands: ['apps_create'], load: ... })],
  healthServices:[defineHealthService({ id: 'llm', label: $localize`...`, icon: 'pw:engine' })],
  notificationKinds: [defineNotificationKind({ id: 'model.update', actions: ['install', 'later'] })],
});
```

Rules of the model:

- **Ids are stable strings**, unique per kind, namespaced by the app (`pw.`, `db.`) for app items and `pd.` for shell items.
- **Static versus live.** Ids, titles, icons, keys, menu placement, dialog kinds and layout constraints are static data.
  `when`, `enabled`, `badge`, `checked` and `run` are functions over the app's context (signals), evaluated defensively: a
  throwing function hides that one item, never the bar or the menu (Debrief's activity bar rule).
- **The context is typed per app.** The shell is generic over `AppContext` (`ShellContext & TApp`): the shell part
  (`notify`, `openDialog`, `showPage`, `runCommand`) plus what the app adds. Plugin-facing contexts stay small (interface
  segregation).
- **A generated manifest** (`paddock-manifest.json`, written by a test in write mode and compared on every run, like
  Debrief's `dialog-contracts.json`) holds the static parts that Rust needs: dialog contracts, menu ids and accelerators,
  health service ids, settings page ids. Rust verifies it at build/test time: every menu id routes, every contracted
  command exists and is open to dialogs, every UI health service id has a Rust supervisor and vice versa.
- **One lint over every instance** (rule 40): `assertContributionsValid(injector)` in the test kit fails on duplicate ids,
  a keybinding conflict in the same scope, a menu item naming a missing command, a page or view without an icon or title
  message id, a dialog without a kind, and a `min > pref` layout. Every app runs it.

Rust: the app builds the shell plugin with its parts:

```rust
let shell = paddock_shell::Builder::new(AppIdentity::new("pitwall", "Pitwall", env!("CARGO_PKG_VERSION")))
    .commands(&app_commands::TABLE)                  // the app's own table (same macro), for the shared guard
    .dialog_contracts(include_str!("../dialog-contracts.json"))
    .health(LlmService::new(supervisor.clone()))     // impl paddock_health::Service (state + repair)
    .settings(EngineSection::new(config.clone()))    // impl paddock_settings::Section
    .jobs(job_pool.clone())
    .updater(UpdaterConfig { feed, public_key })
    .tray(tray_model)
    .watch_windows(WatchPolicy::default())           // ADR 0058
    .build();
tauri::Builder::default().plugin(shell) ...
```

### 3. Rust side

Pure library crates, each with no Tauri dependency in its core and a `tauri` feature for its adapter (the `paddock-jobs`
set-up), plus one Tauri plugin crate that wires them:

| Crate | Content | Taken from |
|---|---|---|
| `paddock-jobs` (exists) | job model, reporter, cancel, pools, walk/map/pipeline | ADR 0008; gains the `tauri` adapter (sink, `jobs_snapshot`, `job_cancel`): ADR 0008 open question 2 |
| `paddock-commands` | the command table macro (kind, window classes, request size limit, async-only), the guard hook, the activity-log classification, generated TypeScript command names | Debrief `command_table!` (`lib.rs`), `commands.rs` (ADR 0016) |
| `paddock-health` | `ServiceState`, cause, repair action, registry, snapshot, the cause-key notification centre and its versioned persistence | Debrief `health/model.rs`, `health/registry.rs`, `notification_centre/` |
| `paddock-dialogs` | contract schema and `allows(dialog, command)`, pure placement on the parent's monitor, window creation by kind, fit/grow, bounds | Debrief `dialog_contracts.rs`, `dialog_placement.rs`, `dialog_windows.rs` |
| `paddock-startup` | the pure `Watchdog` and its ticker, `window_ready`, `window_boot_error`, `ui_checkpoint`, the native ask dialog | Debrief `startup_watchdog.rs` (ADR 0058) |
| `paddock-download` | segmented Range downloads, per-host caps, 429/`Retry-After` back-off, resume, one in-order SHA-256 while writing (`paddock-jobs::HashingWriter`), phases | lessons (rule 15); Debrief and Pitwall model downloads |
| `paddock-settings` | section schema (`Section`, `Field`, `FieldKind`, validation, owners), the versioned settings file with defaults on corruption | `debrief_core::settings`, Debrief `config_store.rs`, `settings.rs` |
| `paddock-updater` | update state machine, manifest, signature, install hand-off, over `paddock-download` | Debrief `updater/`; the four small copies are deleted |
| `paddock-window` | window state, single instance, deep links, title bar and materials, tray popover window | ADR 0006 row; Debrief `native_feel.rs`, `instance.rs`, `tray.rs`; the apps' `window.rs`/`tray.rs` |
| `paddock-menu` | menu model DTOs, validation and limits, the macOS native menu with standard items, diffing, routing to the focused window | Debrief `menu.rs` |
| `paddock-shell` | the Tauri plugin: one builder, all shell commands and events, managed state | new (thin) |

**Tauri plugin over splicing into the app's table** (decision D2 in the plan): the shell's commands are a Tauri plugin
(`plugin:paddock-shell|dialog_open`), so adopting the shell is one `.plugin(...)` line and an app cannot forget a command.
The plugin's invoke handler is generated by the same `paddock-commands` macro as the app's own table, so both get the
same guard (window classes, per-dialog contracts, size limits, async-only, logging class). `tauri-plugin`'s build step
generates the plugin's permission files (`allow-<command>`); the app's capabilities grant the shell's sets per window
class (`paddock-shell:main`, `paddock-shell:dialog`, `paddock-shell:popout`). Tauri's ACL is the coarse boundary, the
command guard the fine one (per dialog), as in Debrief ADR 0054.

**Naming.** Commands `<area>_<verb>` (`health_snapshot`, `health_repair`, `dialog_open`, `dialog_fit_height`,
`jobs_snapshot`, `job_cancel`, `window_ready`, `settings_schema`, `settings_set`, `updates_status`, `menu_set`). Events
`paddock://<area>` (`paddock://jobs`, `paddock://service-health`, `paddock://notifications`, `paddock://menu`,
`paddock://dialog-result`); never an app name in a shell command or event. Wire types are Rust-first: TypeScript types
and the command-name union are generated (`ts-rs`) into the npm package, and a CI check fails on a stale file, so a typo
in a command name is a compile error.

**One version across the boundary.** The plugin answers `shell_version`; the UI package checks at start-up that it
equals its own version and logs a plain error (and fails a test) when an app bumped one side only.

### 4. Angular side

The shell ships as secondary entry points of `@decryp7/paddock-ui` (decision D1), so a manager app never bundles the
workbench or dockview: `@decryp7/paddock-ui/shell` (frame, contributions, context), `/shell/startup`, `/shell/notify`
(notifications and health), `/shell/dialogs`, `/shell/jobs`, `/shell/settings`, `/shell/about` (About, Updates, What's
New), `/shell/menu` (menu model, in-app menu bar, keybindings), `/shell/pages` (`pd-app-frame`), `/shell/i18n`,
`/shell/store` (`withRequestStatus`, rule 30), `/shell/testing`. Peer dependencies: Angular 22 (core, common, router,
cdk, localize), `@ngrx/signals`, `@tauri-apps/api`.

- Zoneless, `OnPush` everywhere, signals, SignalStore with `protectedState`, `withRequestStatus` (rules 27-31).
- Every backend is a port (abstract class used as DI token) with a Tauri implementation and a mock; components never call
  `invoke` (AGENTS.md). Mocks return values decoded from fixtures produced by the Rust round-trip tests (lesson C19b).
- Per-window bootstraps (rule 34): `provideShellMain(app)`, `provideShellDialog(id)`, `provideShellPopout()` give each
  window only its providers; dialog windows get the dialog's declared backend groups and nothing else.
- Stores at the narrowest scope (rule 32); the only root singletons are the notification host, service health and the
  contribution registries.

### 5. Theming

The shell uses only `--pd-*` tokens (`paddock.css`); it never defines colours. `applyPaddockAppearance` selects theme,
accent, density and contrast. Accents for Pacenotes and Sighter are added to `_tokens.scss` and pass the token tests
(contrast, the 20-degree warning-hue rule) before either app ships. App theme packs (Debrief's precision, console and
contrast traits, Debrief ADR 0018) are a registry of token overrides (`defineTheme`), validated by the same token tests
in the app. Icons: the shell's own icons are registered as SVG literals in the `pd` namespace of `MatIconRegistry`;
each app keeps its namespace (`debrief:`, `pw:`); no icon fonts, no runtime URLs (CSP).

### 6. i18n

Angular `$localize` with translations loaded at run time from bundled JSON chunks (one build for every language, the
Debrief `i18n/boot.ts` pattern). The shell's messages use `@@pd.` ids and ship as an XLIFF source plus translation JSONs
in the package; `paddock-i18n merge` (a package bin) merges them into the app's loaders at build time. A pseudo-locale
spec (Debrief `i18n-pseudo.e2e.ts`) is part of the test kit. Manager apps adopt `$localize` for the shell's strings on
adoption and for their own strings when a page is next touched; English is the only shipped language until the user
asks for another. Engine-written texts (health causes, notification bodies) carry a stable `code` next to the English
text so they can be translated later without a wire change.

### 7. Test kit

- `@decryp7/paddock-ui/shell/testing`: `provideShellTesting()` (all shell backends as fakes with fixture data, a fake
  clock), `assertContributionsValid`, the boot-every-dialog harness and its contract check (ADR 0009), the startup
  contract helpers, store test helpers (`TestBed`, rule 38).
- `@decryp7/paddock-e2e` (a plain Node package, Playwright): the e2e port from the environment (required in CI), the Chrome
  for Testing guard (never the installed Chrome, rule 7), axe in both themes, the retried-test reporter (lesson C17),
  page objects for menu, notifications, health chip, dialogs, jobs, settings, and `expectPaneMinimums` (ADR 0011).
- Rust: `paddock-shell::testing` builds the plugin on `tauri::test::MockRuntime`; every shell command test uses
  `paddock_jobs::assert_nonblocking`; temp folders are drop guards with a leftover check (lessons).
- Two reference apps in the Paddock repo exercise every contribution kind against the mock backend in CI: `shell-demo`
  (pages frame) and, with ADR 0013, `workbench-demo`. They are the first consumer of every change.

### 8. Versioning and distribution

- One repo tag `vX.Y.Z` releases everything (ADR 0006): the npm packages carry the tag's version (lockstep), each crate
  bumps its own `version` only when it changes. Shell crates and `paddock-ui` are released from the tag workflow only.
- **Cargo: the Gitea cargo registry from the first shell release** (decision D3; ADR 0006 open question 8): ten crates
  resolved from one git tag clone the whole repo per app and pin every crate together; the registry lets an app bump one
  crate. npm already uses the Gitea registry.
- Apps pin `^0.N.0` (pre-1.0 caret = same minor) and `Cargo.lock`/`package-lock.json` are committed. Renovate opens one
  PR per Paddock release; it lands only through the app's gates.
- **Breaking changes.** Pre-1.0 a minor may break. Every breaking change carries a "Migration" section in the changelog,
  updates both reference apps in the same change, and is checked by a nightly **consumer canary** that builds each
  adopting app's `main` against the candidate (`npm pack` + Cargo `[patch]`) and runs its type-check, lint and unit tests
  (not e2e). The shell goes 1.0 when three apps run on it; from then on strict semver, `cargo semver-checks` and an API
  report for the TypeScript entry points (`@microsoft/api-extractor`) in CI.

## Consequences

- An app gets ADRs 0008-0011 and 0058 by adopting one layer; the recurring bug kinds of Debrief 1.0 become failing shared
  tests in every app (boot every dialog, one notification per cause, the start-up contract, pane minimums).
- About 1,000-2,000 lines of copied Rust and TypeScript leave each manager app; Debrief's 1.1 shell code moves to
  Paddock and is maintained once.
- Shell changes now cross repositories: a fix needs a Paddock release and a bump. The canary and the reference apps
  keep that honest; urgent fixes are a patch release from the tag workflow (never a local publish).
- Manager apps take on `@ngrx/signals`, `@angular/localize` and the CDK if they do not already have them.

## Alternatives considered

- **Keep copies per app, share only the ADRs.** The table in Context is the result: five updaters, five tray modules,
  three kinds of toast; rule 39 says shared parts live in Paddock.
- **One shell including the workbench.** Manager apps would carry the dock, palette and Explorer they never show, and
  every workbench change would bump them. The workbench is a layer on top (ADR 0013).
- **Library crates only, spliced into each app's command table** (`paddock_shell::commands!()` inside the app's
  table). One table, but every app must remember every command and its window classes; a Tauri plugin built by the
  same macro gives the same guard with one line. Recorded as decision D2.
- **Electron or a web framework other than Angular.** The family is Tauri 2 and Angular by rule; the user prefers the
  first-party Angular set (lessons).
- **Tauri's ACL as the only command boundary.** It cannot express per-dialog command lists (Debrief ADR 0054).
