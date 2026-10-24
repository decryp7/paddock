# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Added
- Settings (plan S10, rule 21, ADR 0012; ported from Debrief's settings schema, `config_store.rs` and `settings.rs`):
  crate `paddock-settings` 0.1.0 with the section schema (nine field kinds, schema check at registration), validation
  in Rust (`validate`, `sanitize`, `ValidValues::decode`), the versioned `settings.json` (missing, damaged, newer or
  unreadable: defaults and one `LoadProblem`; damaged and newer files kept aside; atomic writes; sections nobody
  registers kept), the `Settings` service (`register`, `set` laying changes over the values, `reset` of a section or a
  field, contained `apply`, `restart_pending`, one change event per real change), the shell's Appearance section, and
  the Tauri adapter (`manage`, `paddock://settings` to every window, one notification for a start-up problem,
  `settings_overview` / `settings_set` / `settings_reset` on the blocking pool, `route`, dialog grants) with generated
  wire types, command names and Rust-written fixtures. `@decryp7/paddock-ui/shell/settings`: `SettingsBackend` (Tauri,
  memory), `SettingsStore`, `SettingsDrafts` (Signal Forms), `pd-setting-field` (a `FormValueControl` per field kind),
  `pd-settings-section` and `PdSettingsSectionPage`, the Settings window `pd-settings-window` (modeless tool dialog:
  pages by group with lazy page components and their own providers, search across every setting, restart bar,
  start-up problem, unsaved edits marked and asked about before closing, keyboard navigation), `followAppearance`,
  `settingsWindowProviders`, `provideShellSettings` (`OPEN_SETTINGS`) and `SETTINGS_CONTRIBUTIONS` (Settings…, Mod+,).
  `@angular/forms` (^22) is an optional peer dependency. `shell-demo` has the Settings window over the fixtures, with
  Chromium e2e (every field kind, refusal and unsaved edits, search and keys, a damaged file, axe in both themes, 320 px).
- Downloads (plan S8, ADR 0008 §5 and 0012; ported from Debrief's model downloader): crate `paddock-download` 0.1.0.
  `Downloader::download(ctx, req)` runs inside a `paddock-jobs` job with the phases `connect`, `resume` (only when
  resuming), `download` and `verify`, each on its own scale and never backwards, and Cancel noticed within 250 ms in
  every phase. Hosts that serve byte ranges are downloaded over several connections (default 4) with positioned writes,
  splitting of slow ranges and one in-order SHA-256 (`paddock_jobs::HashingWriter`) fed while writing; ranges ahead of
  it wait in a bounded buffer, so the file is never read back (rule 15). Hosts without ranges are streamed, and a
  dropped connection restarts and skips what was written. Per-host connection caps across downloads (`HostCaps`, default
  4), 429/503 with `Retry-After` (seconds or date) lowering the connections and holding the host, exponential back-off,
  stall detection, resume from the hashed prefix (`<name>.part.json`, matched by size, pinned checksum and strong ETag or
  Last-Modified, `If-Range` on every range), pinned checksum and size, size limits before and while writing, and an
  atomic rename after a sync. Typed `DownloadError` with `keeps_partial()`. The network is the `Transport` port;
  `HttpTransport` (feature `http`, default: ureq, rustls, the OS certificate store) and a `tauri` adapter (`manage`,
  `downloader`). Tests against an in-process HTTP server (no-range host, 429, truncated segments, digest mismatch, a
  file that changes, resume, host caps, cancel in every phase) and a scripted transport; a benchmark against one stream.
- Menus and keys (plan S13, ADR 0012; ported from Debrief ADR 0008 and `menu.rs`): crate `paddock-menu` 0.1.0 validates the
  UI's menu model (`parse`: limits on menus, entries, depth, ids, labels; key bindings to native accelerators), builds the
  macOS menu bar with the standard items (`native_spec`: app menu with Services and Hide and `pd.app.quit` last, Edit
  with Undo..Select All, Window with Minimize and Zoom; `outline` snapshot), diffs it (`Unchanged`, states in place,
  `Rebuild`), routes clicks to the focused top-level window (`Focus`, `route`), and checks the manifest's menus and keys
  (`manifest::check`: every item routes, every key has an accelerator, a command in one menu-bar menu only). Tauri
  adapter: `install`, `menu_set` (async; answers `MenuApplied`), `on_window_event`, `menu_chosen` on `paddock://menu`,
  `NativeMenu::builds` (a start builds twice, the same model again builds nothing); generated
  `shell/menu/generated/menu.ts`. `@decryp7/paddock-ui/shell/menu`: `defineSubmenu` / `provideSubmenus` (the `submenus`
  kind with its lint: missing parent, cycle, depth, a command in two menus), the pure menu model (`buildMenus`,
  `buildMenu` for context menus), `AppMenu`, `pd-menu-bar` and `pd-context-menu` on the CDK menu, `Keymap` (effective
  bindings with user overrides persisted under `paddock.keybindings`, followed across windows, conflicts reported,
  `bind`/`reset`, dispatch by `data-pd-key-scope` scope with the text-field rules) with `provideKeyboardShortcuts`, the
  key helpers (`matchesKey`, `formatKey`, `ariaKeyShortcuts`, `recordKey`), and `NativeMenuBridge` (first build once the
  app is ready, changes coalesced to the next idle moment, again on focus, shortcut echo ignored) with
  `tauriNativeMenuPort` and `provideNativeMenu`. `@angular/cdk` (^22) is an optional peer dependency. `shell-demo` has
  the menu bar, a submenu, a toggle, a context menu and a shortcuts editor, with Chromium e2e.
- Notifications and service health in the app shell (plan S4, ADR 0010 and 0012; the port of Debrief ADR 0055):
  `@decryp7/paddock-ui/shell/store` publishes the shared `withRequestStatus` SignalStore feature (rule 30: one
  `idle | loading | loaded | { error }` union, `setLoading`/`setLoaded`/`setError`, `errorMessage`); `shell/jobs` uses it
  instead of its local copy. `@decryp7/paddock-ui/shell/notify`: the `ServiceHealthBackend` and `NotificationsBackend`
  ports (Tauri over the app's IPC, `MemoryServiceHealth` and `MemoryNotificationCentre`), edge parsers of every payload,
  `ServiceHealthStore` (newest revision wins, nothing healthy until the first snapshot), `NotificationsStore` (the
  engine's centre plus the UI's own notices from `SHELL_PORTS.notify`, merged by cause with counts), the one
  `pd-notification-host` (one toast per cause with its count, details folded, host-owned time held on hover and focus,
  never a `health:` cause), the centre panel and its status item, the service chip `pd-service-status` with Repair
  served by the shell (rule 19) and Try again when the state is unknown, `pd-waiting-for` and `pd-service-error`,
  notice action handlers, `provideShellNotify`, `notifyPort` and `NOTIFY_CONTRIBUTIONS`. The mirrored wire types are
  checked against the generated `shell/generated/health*.ts` by a type-level drift spec. `shell-demo` replaces its notice
  list with the host and adds a demo engine whose Repair works and a search index whose Repair fails, with Chromium e2e
  (three identical failures are one chip and one notification with count 3 and no toast, the waiting panel loads again
  by itself after Repair, dedupe of notices, "Show in Notifications", axe in both themes).
- `@decryp7/paddock-ui/shell/dialogs` (plan S6, ADR 0009; ported from Debrief ADR 0054's UI): the kind policy (sizes,
  fit and grow-only, Escape/Cmd+W/Enter, a visible Close/Cancel in every state), `DialogService` (typed by the
  definition, one window per id or per input, `ModalGate`), `WindowDialogBackend` over the `paddock-dialogs` commands and
  events (remembered size and offset, falls back to the page), `InPageDialogBackend` and `pd-dialog-host` (backdrop,
  focus trap and restore, scrim while a modal window is open), `DialogShell` and `dialogWindowConfig` (each dialog window
  its own small bootstrap with only its dialog's providers), `tauriDialogIo` over a passed-in Tauri bridge,
  `browserDialogIo` (dialog routes opened by URL for e2e), `DeniedCommands`, and the contract file writer
  (`dialogContractFile`, read by `Contracts::parse`).
- `@decryp7/paddock-ui/shell/testing` (plan S6): the boot-every-dialog harness (`bootDialog`, `assertDialogBoots`,
  `bootEveryDialog`: missing provider, failed or empty body, commands the contract does not declare, Close/Escape per
  kind, fit per kind), `dialogTableProblems` against the generated command table, `FakeDialogIo`, `recordIpc`.
- Package bin `paddock-check-dialog-commands` (plan S6): the static call-graph check that every dialog declares exactly
  the commands it can reach, configured per app (`paddock-dialog-commands.json`: registry, table, base set, host-only
  services, boundaries, reviewed exceptions); `typescript` is an optional peer dependency for it.
- `shell-demo`: one dialog per kind (plus a modal form and a required choice), its `dialog-contracts.json`, dialog windows
  booted from `#/dialog`, and a Chromium e2e over the contract file.

### Changed
- `paddock-dialogs`: dialog ids may contain `.`, so the shell's namespaced contribution ids (`sd.about`) are valid
  contract keys; a test parses the shell demo's generated contract file.
- Crate `paddock-dialogs` 0.1.0 (plan S5, ADRs 0009 and 0012): dialog contracts (`Contracts::parse`, `allows`,
  `problems` against the command table, kind size policy), `Grant` for the `paddock-commands` guard, pure placement on
  the parent's monitor (mixed DPI, negative coordinates, remembered offset, never the primary display as default),
  `Dialogs` bookkeeping with close-request and required-choice rules, and the Tauri adapter (window creation by kind
  with title-bar buttons disabled, modality, close requests forwarded, `page_unresponsive`, fit-height grow rules, the
  four `dialog_*` commands); generated `shell/generated/dialogs.ts`.
- Crate `paddock-health` 0.1.0 (plan S3, ADR 0010 and 0012): the service-health model (closed `ServiceState`, `Cause`
  with hint, detail, code and memory-cap advice, `RepairAction`, `Snapshot`), `Health` (app-registered `ServiceSpec`s,
  one announcement per real change, repairs as shell closures on a background thread), the cause-key notification
  centre (merge with counts, caps, read state, resolve, app file with debounced atomic writes, project records,
  OS-notification click-through) and the Tauri adapter (`paddock://service-health`, `paddock://notifications`,
  `health_snapshot`, `health_repair`, `notifications_*` with their `COMMANDS` table); generated TypeScript
  `shell/generated/health.ts` and `shell/generated/health-commands.ts`. Ported from Debrief ADR 0055.
- Jobs in the app shell (plan S9, ADR 0008 and 0012): `paddock-jobs`' `tauri` feature is the adapter (`paddock_jobs::tauri`):
  the `Emit` sink on `paddock://jobs`, the managed `Jobs` state and `manage`, the `COMMANDS` table (`jobs_snapshot`,
  `job_cancel`, async, returning at once) with its guarded `handler` / `handler_with_grants` and `route` in front of the
  app's handler, and generated `shell/jobs/generated/jobs-commands.ts` plus `jobs-fixtures.ts` (Rust round trip).
  `@decryp7/paddock-ui/shell/jobs`: the `JobsBackend` port (`TauriJobsBackend` over the app's IPC, `MemoryJobsBackend`),
  `parseJobSnapshot`, the per-window `JobsStore` (highest `seq` per job, request status union, "Still working" without a
  timeout, notices when a job ends), `pd-jobs-list`, `pd-jobs-status`, `JobFollow`, `provideShellJobs`,
  `JOBS_CONTRIBUTIONS` and the `uiTimeoutProblems` grep check. `shell-demo` runs a sample job with phases, a step that
  cannot be cancelled and a quiet job, with Chromium e2e (phases, Cancel from the list, "Still working" for ten minutes,
  axe, no UI timeout anywhere).
- Crate `paddock-commands` 0.1.0 (plan S1, ADR 0012): `command_table!` (kind, window classes, request limit, validated at
  compile time, optional invoke-handler macro), the shell's window label scheme, the pure `check` with the
  `DialogGrant` hook for per-dialog contracts, the Tauri `Guard` (refuses and logs before arguments are parsed),
  `Table::contract_problems`, the async-only check and generated-file check for tests, and generated TypeScript:
  `shell/generated/commands.ts` (wire types) and `shell/generated/shell-commands.ts` (command names).
- Crate `paddock-startup` 0.1.0 (plan S7 and §11, ADR 0012; ported from Debrief ADR 0058): the pure `Watchdog` (start-up
  deadline, reload once then ask, Keep Waiting, the pause rule for a stopped app process, terminated content processes
  with a reload budget, boot-error cap) plus the liveness heartbeat after ready (`paddock://ping` / `window_pong`; hidden
  windows are not pinged; N misses mark a window unresponsive, offered Reload / Keep Waiting; a page that answers again
  recovers), `Checkpoints` and `ready_reply`, `clean_error`, the native question's texts, `Config` with checked timings
  and the `COMMANDS` table (`window_ready`, `window_boot_error`, `ui_checkpoint`, `window_pong`). Feature `tauri`:
  `Startup::builder()` (Safe Mode restart, unresponsive hook for `paddock-dialogs`, ready hook, watch filter, asker),
  `install`, `on_page_load`, `web_content_terminated`, the ticker thread and the commands; feature `dialog`: `NativeAsker`
  over `tauri-plugin-dialog`. Generated TypeScript in `shell/startup/generated/`.
- `@decryp7/paddock-ui/shell/startup` (plan S7): `provideStartupContract` (ready after the first render, then answers
  liveness pings), `retryUntilAnswered`, boot error forwarding, `StartupBackend` with `TauriStartupBackend` (over an
  app-given `StartupIpc`, no `@tauri-apps/api` dependency) and `MockStartupBackend`, `WindowRecovery` with `UiCheckpoint`
  (`provideUiCheckpoint`) and the recovery notice through `SHELL_PORTS.notify`, and `splashProblems` for an app's
  `index.html`. The shell demo wires it (note kept across a recovery reload) with Chromium e2e for the splash, withheld
  IPC, recovery and a page that stops answering after ready.
- App shell contributions core (plan S2, ADR 0012 §2, lazy by construction per plan §9) in `@decryp7/paddock-ui/shell`:
  `defineCommand`, `defineKeybinding`, `defineMenuItem`, `definePage`, `defineStatusItem`, `defineSettingsPage`,
  `defineDialog`, `defineHealthService`, `defineNotificationKind` (frozen static data plus loaders; texts are `msg(id, text)`
  so message ids survive to run time), `provideContributions<T>`, `ContributionRegistry` with kinds the workbench extends
  (`defineContributionKind`, `provideContributionKinds`), `ShellContext<T>` with the app part loaded on first use or when
  idle (`provideShellContext`), `CommandService` (defensive `when`/`enabled`, failures become notices), `loadPart`
  (component plus its own providers), key binding parsing and accelerators, `IpcCommandName` (the app's generated `AppCommand`
  via `ShellTypes.appCommand`, plus the re-exported `ShellCommand` of plan S1) typing dialog contracts, and the `paddock-manifest.json` writer, parser and drift check.
- `@decryp7/paddock-ui/shell/testing`: `assertContributionsValid` (duplicate ids, key conflicts per scope, menu items and
  keys naming missing commands, missing icons and message ids, dialogs without a kind or with a forbidden modality,
  `min > pref`, unknown kinds) and `checkRegistrationSource` / `assertRegistrationsLazy` (a `*.contributions.ts` file
  imports no store, service, backend, command class or component).
- `shell-demo` registers one contribution of every kind, runs commands against a lazily loaded app context and checks
  both lints.
- App shell scaffold (plan S0 of `docs/plans/workbench.md`, ADR 0012): secondary entry points `@decryp7/paddock-ui/shell`,
  `/shell/{startup,notify,dialogs,jobs,settings,about,menu,pages,i18n,store,testing}`, `/workbench` and
  `/workbench/{dock,panels,palette,explorer,assistant,testing}` (empty until their work packages land); the
  `@decryp7/paddock-e2e` package skeleton; the `shell-demo` reference app with its Playwright checks (axe both themes,
  320 px).
- Crates `paddock-commands`, `-dialogs`, `-download`, `-health`, `-menu`, `-settings`, `-startup`, `-updater`, `-window`
  (scaffolds: pure core plus a `tauri` feature, held back from publishing until filled); `paddock-jobs` gains its `tauri`
  feature with the adapter placeholder.
- CI: every crate is linted and tested with all features on Linux, Windows and macOS; the library entry points, the e2e
  package and the shell demo are built; the publish dry run runs on every push.
- `.gitea/workflows/publish.yml` and `scripts/publish.mjs`: on a `v*` tag only, the crates go to the Gitea cargo registry
  `decryp7` (dependency order, scaffolds and existing versions skipped) and the npm packages to the Gitea npm registry,
  then the release is created; without `--publish` the script is the local dry run (decision D3).
- ADR 0012 "Paddock app shell" and ADR 0013 "Paddock workbench" (proposed), with the plan in `docs/plans/workbench.md`.
- ADR 0008 "Long operations: phases, progress, cancel and parallel work".
- Crate `paddock-jobs` 0.1.0: job/phase/progress wire model, throttled `Reporter`, `CancelToken` with deadline, `JobRegistry`/`JobContext`,
  `HashingWriter`/`copy_hashed`, `par_walk`, `par_map_ordered`, `pipeline_ordered`, `JobPool` (spawn, `offload`) and `assert_nonblocking`.
- `pd-job` (`SuJob`) and the pure `jobView`, `formatCount`, `formatBytes`, `formatEta` helpers in `@decryp7/paddock-ui`; gallery section.
- CI job for the Rust crates (fmt, clippy, test, coverage, deny).

### Changed
- `paddock_jobs::EVENT_NAME` is `paddock://jobs` (was `paddock-job`), the shell's event scheme (ADR 0012 §3).
- `@decryp7/paddock-ui` has `@ngrx/signals` ^22 as an optional peer dependency (the shell's stores).
- Publishing moved from the `release` job of `ci.yml` (and `ui/scripts/release.mjs`) to `publish.yml` and
  `scripts/publish.mjs`; `npm run release` in `ui/` is still the dry run.
- `crates/deny.toml` checks the crates with all features (the Tauri adapters) and allows file-level MPL-2.0 (Tauri's tree),
  as Debrief does.
- `@decryp7/paddock-ui` is published to the Gitea npm registry by the tag workflow (`.npmrc` with the `@decryp7` registry and
  `"@decryp7/paddock-ui": "^0.1.0"`) instead of a release-asset tarball; releases carry notes only.

## 0.1.0 - 2026-10-22

### Added
- `@decryp7/paddock-ui`: tokens (neutral roles, status, five accents, 8-colour CVD-checked chart palette, diverging and sequential
  scales, spacing, radii, type scale with tabular numerals, elevation, motion, three densities, dark and light, high contrast),
  base styles with native feel (Debrief ADR 0028), focus rings, thin scrollbars, splash and tray-page helpers.
- Components: `pd-card`, `pd-status-pill`, `pd-metric`, `pd-sparkline`, `pd-empty-state`, `pd-error-state`, `pd-skeleton`,
  `pd-segmented-control`, `pd-tray-popover`; `applyPaddockAppearance`.
- ECharts theme from tokens (`readPaddockTokens`, `createEchartsTheme`, `registerPaddockTheme`, `areaGradient`, `eventLaneFragment`,
  `sequentialRamp`).
- Token test: WCAG contrast of every pair and colour-vision separation of the chart palette (131 tests).
- Gallery app with Playwright screenshots (every component, both themes, five accents), axe, keyboard and CSP checks.
- `crates/` Cargo workspace root (no members), `docs/` design guide and lessons, CI workflow, release script.

### Changed (design guide values that failed the token test)
- text-subtle, text-muted (light), success (light), chart-neutral-bar, Pitwall fill and the whole chart palette; added
  `border-control` and `danger-fill`. See `ui/README.md`.
