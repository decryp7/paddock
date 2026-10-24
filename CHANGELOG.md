# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Changed
- Appearance has one writer and one source (rule 40, rule 34): `paddock_settings::shell::appearance` holds the whole
  look (theme, contrast, motion, density and the theme pack; `PACK_ID`); `provideShellSettings` provides it as the
  window's `APPEARANCE_SOURCE`, so `provideAppearance` follows the engine's Appearance settings in every window and
  writes every change there (`SettingsAppearanceBackend`: `overview` in a main window and the Settings window,
  `events` in a dialog window whose contract has no settings command, `provideSettingsAppearance`). The old
  `appearanceFollower` / `followAppearance` / `appearanceOf` are gone (`appearanceFromSettings`, `appearanceToSettings`
  replace the value mapping); the Settings window never writes the look itself. An e2e check allows only the appearance
  store (and the pre-paint copy) to put the look on `<html>`, and a View-menu change and a Settings change in another
  window never revert each other. The shell demo's in-memory settings engine is shared by the origin's windows and kept
  in localStorage, as the Rust engine is.
- The in-app menu bar (Windows, Linux, the browser) has no app menu: the title bar already names the app, so its entries
  (Settings…, About, Quit) are at the end of File (`foldAppMenu`); the macOS menu bar keeps the app menu.
- Pages frame, review round 3: status items sit at equal distances (4 px gap, 4 px item padding; the service chip
  keeps 5 px of margin for its visible edge) and the chip looks pressed while its details are open. `shell-demo`: the
  status item says the table's exceptions ("1 offline · 1 updating", the full count in its name and tooltip), and the
  Runners badge counts the same offline runner ("1 runner offline").
- Pages frame after its re-review (`ui/docs/screenshots/shell-demo/REVIEW-2.md`): a running job no longer blinks out
  of a narrowing status bar for a frame (an item not yet measured in its compact form counts as no width; every item is
  re-measured when the bar changes form; regression tests in the unit specs and in Chromium). The service chip's
  compact form keeps a subject ("Index stopped"); its Repair button is named for the action and the service
  (`repairButtonLabel`: "Restart demo engine", "Reset search index"). The jobs status item names the lead job ("1
  running: Update runner") with the phase or "still working" in its tooltip (`jobsStatusDetail`); its spinner is a quarter
  arc on a faint ring. `definePage({ badgeLabel })` says what a badge counts (tooltip and accessible name). The drawer
  starts below the title bar's hairline and casts its shadow to the right only; status-bar focus rings are inset; the
  job's warning icon sits on the first line. `shell-demo`: consistent runner data (build-mac-2 "Updating"), the harness
  pages (Dialogs, Keyboard) only in developer mode (`sd.developer` off for the reference screenshots), new focus shots
  of a selected and an unselected page link.
- Pages frame after its design review (`ui/docs/screenshots/shell-demo/REVIEW.md`): the status bar shrinks its items to
  their compact form first (`STATUS_COMPACT`: the service chip says only the state, jobs only the count) and hides only
  unpinned items behind a counted "More status (n)" button (24 px, text-muted); `defineStatusItem({ pinned })` keeps
  the service chip and the bell; one selection style in every sidebar mode (accent tint, accent icon, 600, 2 px bar);
  the rail badge sits off the icon with a ring; the sidebar over the page is modal (scrim over the page and the status
  bar, both inert); a stable toggle glyph and a truncating app name; a 40 px title bar (design guide §2) with 78 px for
  the window buttons in the macOS app, whose menus are native only; pages lay out by `@container pd-page` (16 px gutter
  when narrow). The service chip is a borderless danger tint at 12/500; the unread count is neutral (danger only for
  an unread error); the running jobs item shows a neutral spinner. `pd-job`: the phase is muted, a stalled step says so
  with an icon in warning, the fill is the accent on surface-2 (3:1), an unknown total without motion is a striped track
  and counts its elapsed time (`formatElapsed`, `elapsedMs` from `pd-jobs-list`). `shell-demo` shows a Runners page with
  sample fleet data, page-scoped state and a "3 of 4 runners online" status item; screenshots re-shot with health
  popover, keyboard focus and macOS frames.

### Added
- Updates, About and What's New (plan S11, ADR 0012; from Debrief's `updater/`, `legal.rs` and What's New): crate
  `paddock-updater` 0.1.0. `Updater` is the update phase machine (unavailable, idle, checking, up to date, available,
  downloading, ready, installing, failed) with typed errors and a next step for each; a check fetches the channel's
  `latest.json` through `paddock-download`'s transport (bounded, validated, the package chosen by Tauri's target keys)
  and a download runs through `paddock-download` (size-pinned, resumable, free space checked) followed by a streamed
  minisign check against the key built into the app that also requires the signature to name the announced version; a
  refused signature deletes the file. Both are job bodies (`update-check`, `update-download`, phase `signature`), so the
  commands return at once and Cancel works in every phase; nothing has a UI timeout. A build without a key is
  "Updates are not available in this build". Installing happens at exit (`install_at_exit`) through an `Installer`
  (`tauri::PluginInstaller` over `tauri-plugin-updater`, feature `install`, without its `zip` feature) after
  `ServiceHooks::before_install`; `ServiceHooks::on_version_change` runs once per version change at start-up and again
  after a failure (rule 24). Preferences (automatic check, release channel), the last check and a failed install live in
  a versioned `updates.json`. `About` serves the running version, build facts and legal texts on request, and What's New
  the running version's notes only (`notes`: never another release's). Tauri adapter: `UpdatesBuilder`, `install`,
  `on_exit`, `COMMANDS` (`updates_*`, `about_*`), the `paddock://updates` event, generated `shell/about/generated/`.
- `@decryp7/paddock-ui/shell/about` (plan S11): `AboutBackend` and `UpdatesBackend` (Tauri, and in memory with a fake
  update server), the per-window `UpdatesStore` (events win over a crossed reply; the main window announces a found
  update once), the About and What's New viewers and the Updates tool (`defineAboutDialogs`), the Help menu's commands
  (`aboutContributions`) and `provideShellAbout` (the main window asks once, after ready, whether to open What's New).
  `shell-demo` wires them with `?updates=release|bad-signature|offline|none|unavailable`; Chromium e2e in
  `e2e/shell-demo-about.spec.ts`, including the Tauri backends against a fake engine.
- `paddock-download`: a free-disk pre-check (plan §13): a pinned size is checked before connecting and an announced
  size before writing, against `Config::disk_reserve` (64 MiB by default); `DownloadError::NotEnoughSpace` says "needs
  N, M is free". `Downloader::with_free_space` and `Downloader::transport`, `human_bytes`.
- Theme and i18n in the app shell (plan S15, ADR 0012 §5-6, decision D6). Tokens: accents for Pacenotes (route green,
  `#7fc85a` / `#3b7114`, fill `#3d7515`) and Sighter (orchid, `#cc8cf2` / `#a224cc`, fill `#a224cc`), a complete high
  contrast mode (opaque hairlines and scrim, outlined popovers, subtle text as muted; `data-contrast="more|off"`) and
  reduced motion (`data-motion="reduce|full"`, durations at most 100 ms, `--pd-motion-scale`); `applyPaddockAppearance`
  takes `contrast` and `motion`. Token tests for seven accents in both schemes and both contrast modes, high contrast
  and reduced motion (289 tests); the colour maths moved to the shell and is shared with `themeProblems`.
  `@decryp7/paddock-ui/shell`: theme packs as the `themes` contribution kind (`defineTheme`, `provideThemes`,
  `themeProblems`), the window's appearance (`provideAppearance`, `AppearanceStore` with the choice resolved against
  the OS, `applyStoredAppearance` before the first paint) synced across windows by `BrowserAppearanceBackend`
  (BroadcastChannel) or `IpcAppearanceBackend` (`paddock://appearance`), and `msg()` texts read through the loaded
  translations (`localizedText`). `@decryp7/paddock-ui/shell/i18n`: `initI18n` (runtime-loaded translation chunks, a
  broken file falls back to English), `LanguageStore`, locale, plural and number helpers. Every user-visible string of
  the root components, the frame parts and of `shell/jobs`, `shell/notify`, `shell/dialogs`, `shell/startup`, `shell/menu`, `shell/pages` and the Settings window's own words is a
  `$localize` message with a `@@pd.` id (201 in `i18n/messages.pd.xlf`, shipped with its pseudo-locale); the service
  chip names a service by its contributed `defineHealthService` label. Bin `paddock-i18n` (`catalog`: `$localize`
  extraction plus `msg()` and `$localize` with `@@id`s scanned from the sources, or a manifest, into a sorted XLIFF
  catalog; `merge`: the shell's and the app's translations into the app's loader file, the pseudo-locale generated;
  `--check` for CI) and `npm run i18n` /
  `i18n:check` (CI). `shell-demo`: View > Appearance (theme, a sample pack, high contrast, reduced motion,
  pseudo-locale), Chromium e2e for switching across windows and a pseudo-locale run that finds no unmarked English in
  any shell part, and review screenshots in `ui/docs/screenshots/shell-demo/`. With Settings (plan S10) the look has
  one owner: `SettingsAppearanceBackend` / `settingsAppearanceBackend({ local, gate })` in `/shell/settings` keeps theme
  and density in the engine's Appearance section (contrast, motion and the pack in the window), and the demo passes
  `appearance: false` to `provideShellSettings`.
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
- Crate `paddock-window` 0.1.0 (plan S12, ADR 0012): top-level windows in the shell's label scheme (`main`, `w-<n>`;
  open, list, reveal, titles, the close setting with quit / keep running in the tray / ask, hide or destroy), remembered
  bounds restored on the right monitor (versioned file; scaled, shrunk and moved inside the work area; cascade from the
  parent), single instance with deep links of the app's schemes and the open plan (`second_instance`, macOS `Opened` and
  `Reopen`), the bounded quit (teardown steps on their own threads, the exit watchdog ending a stalled exit, once), native
  feel for windows and dialogs, the tray (menu model, redraw on change, the shell's items, `NativeTray` behind `tray`)
  and the tray popover window (frameless, transparent, OS material, anchored to the icon, fitted height, closed on blur
  and Escape with a fade); `COMMANDS` and generated `shell/generated/window.ts` and `window-commands.ts`.
- Window class `TRAY` in `paddock-commands` (plan S12): tray popovers are labelled `tray` and `tray-<item>`, are not
  part of `WINDOW`, and call only commands that name the class; `paddock-startup`'s default watch leaves them out.
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
- Pages frame (plan S14, ADR 0012): `@decryp7/paddock-ui/shell/pages` composes a manager app's whole main window from its
  contributions with `provideShellPages({ appName, notify, jobs, startup, nativeMenu, dialogs, routes, paths, sidebar,
  icon })` and `<pd-shell-frame />`: routes for the contributed pages (hash URLs by default; each page lazy with its own
  providers through `loadPart`, `when` guarded, the first page for the empty path and anything unknown, "Page – App"
  window titles), the sidebar (`pd-pages-sidebar`: top and bottom pages, neutral badges, arrow keys) expanded, an icon rail
  or over the page with a scrim as `planLayout` leaves room (`sidebarFit`; the user's collapse kept under
  `paddock.pages.sidebar`), `ShellPages` (`showPage` port, a page that stops being offered gives way), the menu bar and
  keys (F6/Shift+F6 regions reserved), notifications, jobs, dialogs and the start-up contract with the native menu built
  after ready. The root shell entry gains the frame parts shared with the workbench: `pd-title-bar`, `pd-status-bar`
  (contributed status items by side and priority, the lowest hidden when narrow with "Show all status items",
  `planStatusBar`), `pd-part-outlet` (lazy part with skeleton or error and Try again), `pd-icon` with the `SHELL_ICON`
  port and the shell's `pd:` icons, and F6 region helpers. `planLayout` (ADR 0011) moves from Debrief to the root entry
  with its spec. `@angular/router` (^22) is an optional peer dependency. `shell-demo` is now a full pages app (Overview,
  Dialogs, Keyboard, Activity with page providers and a badge, a conditional Lab, About; a status item of its own; its
  own icon component) with Chromium e2e for navigation, lazy pages, conditions, status bar parts, every width from 320 px,
  F6 regions and axe in both themes, and screenshots in `ui/docs/screenshots/shell-demo/`.
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
- `@angular/localize` (^22) is a peer dependency of `@decryp7/paddock-ui`: apps add `@angular/localize/init` to their
  polyfills (the root components' texts are `$localize` messages now). The gallery and the shell demo do.
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
