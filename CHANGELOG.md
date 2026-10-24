# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Changed
- X21 data parts after the independent review (`ui/docs/review/x21-data/REVIEW.md`): `pd-table` keeps its primary column at 128 px
  or more (optional columns drop out from the right as the table narrows; the action column is sized to what the rows can show;
  compact toolbar), rows are 32 px by default (24/32/36 by density) under a 28 px header, a quiet sort chevron shows on hover and
  focus, first column weight 500, selected rows get the accent tint and a 2 px bar, loading and error states and a projected
  next step for an empty table, middle ellipsis measured as one piece of text (`pd-middle-text`). Menus use shared `.pd-popmenu`
  classes (surface-3, 2 px focus ring inset, icon slot, rule before destructive items) and `copy-menu` gets the same ring.
  `pd-chart` draws a visible title (label and unit), loading and error states with Retry; donut: total in the middle, every slice named
  with value and share, slice order chosen so neighbours (the wrap-around pair too) keep delta-E 20 under protan, deutan and
  tritan (tested over the real palettes in `test/tokens/ring.spec.ts`), more than five slices merge into "Other"; only the top
  stacked-bar segment is rounded; a lone value between gaps gets a marker; one series uses the accent. `pd-field` puts the error
  under the control in place of the hint and takes `size="short"`. New `columns` glyph. Design guide: palette slot 6 (light) is
  #494FDB as in the CSS.

### Added
- `paddock-view` (plan X51, ADR 0002): the declarative view model plugins return. `ViewDoc` with `Node`/`NodeKind` (stack, text,
  table, tree, graph, diagram, form, review list, matrix, chart, badge, progress, empty, needs-AI, link), `ActionRef` (with a
  label), `ViewAction`, `ViewPatch` and `apply_patch`; JSON (camelCase) and postcard codecs with a size limit and a decode depth
  guard; `validate` with the ADR limits (errors name the node path; http and https links only; every interactive node labelled);
  a never-panics property test and a cargo-fuzz target (`crates/paddock-view/fuzz`); generated TypeScript in
  `@decryp7/paddock-ui` (`view/generated/view.ts`, stale-checked by a test). The `<pd-view>` renderer is the next step.
- Every Paddock crate that parses untrusted input has a never-panics test (`paddock_testkit::assert_never_panics`, new: the one-line
  form of `corpus` + `never_panics`), so C36 reports nothing and is strict in Paddock's own CI (`paddock-conformance --strict C36`: the
  warnings of the named checks fail the run). C36 now assigns a file to the nearest crate above it.
- The `sidecar-self-test` composite action (rule 23 on Linux and Windows bundles) and its use in the shell demo's release workflow. The
  demo agent's `--self-test` is `paddock_proc::self_test` now (a failing check exits 1; before it always exited 0). `paddock-proc` was
  already in the agent through `paddock-service`, so no dependency was added.
- `paddock-files` and `@decryp7/paddock-ui/shell/files` (plan X31): the user's files. `PathGrants` (open only paths the user chose,
  by dialog, drop or command line; per window, bounded, expiring, canonical), the validators (`parse`, `canonical`, `output_file`,
  `child`, `confined`), validated pickers behind `Picker` (`NativePicker` over `tauri-plugin-dialog`), `Recents` (named lists in a
  versioned file, recorded only after a granted open, granted again on use), reveal, open and web addresses with command lines per
  OS (never a file that runs on open; http, https and mailto only), `Files` and the `files_*` commands in the shell plugin
  (`Builder::files`, drops granted by the window hook, `WindowGrants` for the app's own commands). UI: `FilesBackend`
  (Tauri, memory), `FilesStore`, `pd-recent-files`; the shell demo has a Files page.
- Undo and an activity journal (plan X44). `paddock_jobs::journal`: a bounded (at most 5,000 entries, cut texts), persisted (paddock-env
  versioned file, user-only permissions, a damaged file kept, a newer one never overwritten) log of what the app did on the user's
  behalf, each entry `NotUndoable`, `Available`, `Undone` or `Expired`; `record`, `mark`, `expire_before`, `entries`, listeners for
  pushing changes, a save failure kept for the app to show. Undoing stays the app's work. UI: `@decryp7/paddock-ui/shell/undo` with
  `UndoStore` (the history of the user's edits: labels, merge keys, groups, limit 100; port of Debrief's `UndoStack`), Edit > Undo and
  Redo (`UNDO_CONTRIBUTIONS`, Mod+Z / Mod+Shift+Z), the `pd-undo-toast` "Done, Undo" toast, `ActivityStore`, the Activity page
  (`ACTIVITY_CONTRIBUTIONS`) and `TauriActivityBackend` / `MemoryActivityBackend`. The shell demo shows all of it (Say hello is
  undoable). `paddock-jobs` now depends on `paddock-env`. The `tauri` feature adds `paddock_jobs::tauri_activity`: `manage(app, journal)` emits every entry on `paddock://activity`, the commands `activity_list`, `activity_undo` (runs the app's `Activity::on_undo` handler off the IPC thread, then marks the entry undone; the handler's `Err` text is the user's message) and `activity_mark`, with generated names in `shell/undo/generated/`. The schedule and journal tests moved into the single `it` test binary.
- `paddock-chromium` and `paddock-runtime` (plan X14). `paddock-chromium` is the one Chromium driver, merged from Debrief's
  `chromium-driver` and Spotter's `spotter-render`: one command-line builder with the start page always last, `Sandbox::decide`
  (root plus an explicit opt-in only), launch as a `paddock-proc` supervised tree with a time limit on an injected clock and a
  memory cap, the DevTools client that knows when the user closed a window, pages (snapshot, wait until readable, capture),
  both apps' readiness and challenge rules, discovery that gives tests only `CHROMIUM_TEST_PATH`, the pinned Chrome for Testing
  runtime and a fake browser for tests. `paddock-runtime` installs pinned runtimes (Chromium now, FFmpeg and llama.cpp later)
  from a checked catalogue as one job (download hashed while written, unpack through `paddock-archive`, self-test, install), keeps the previous release
  for rollback and quarantines a release that fails.
- `@decryp7/paddock-ui/live`: `streamCall` and `tauriChannelFactory` for commands that stream to their caller over a per-call
  channel (ordered delivery, nothing after the call settled, failure, handler error, last-message marker, abort). `paddock-metrics`:
  `folder_size` does not cross volumes (`other_volumes`) and reads a folder entry by entry, so a cancel or the time limit is seen
  between reads. CI: a step for `paddock-net`; `paddock-archive` and `paddock-net` take the workspace hack.
- `paddock-mcp` (plan X22, ADR 0007): the local-only MCP server. `Server` with `ToolDef`/`ResourceDef` registration, `Policy` (off by
  default, read-only tools, actions only by name), schema validation, result caps, cursors, time limits with cancel, panic
  containment, `AuditLog` with redacted arguments, `serve_stdio` and `serve_loopback` (per-client `KeyStore` keys, loopback peers
  only, bounded lines, connection cap), `client` request builders and the shared MCP types for Pitwall; a contract test of the
  2025-06-18 message shapes.
- `paddock-proc` (plan X54): every `Supervised` child on Windows runs in a job object of its own with kill-on-close (and
  die-on-unhandled-exception), so the child and every helper it started end when the value is dropped or the app exits or
  crashes, also helpers that left its process tree; `Supervised::in_job`. `paddock-ipc`'s launcher gets it through
  `Supervised` (`Child::in_job`). Windows refusing a job degrades to the tree kill.
- `paddock_jobs::schedule` and the shared schedule editors (plan X33). `Schedule` (every N seconds, days at times of day, cron with five or
  six fields) in the computer's or a chosen IANA zone with daylight saving (a time inside the spring gap runs once, shifted; a repeated
  time runs once), `next_check` (jitter, back-off after failures, a per-source minimum interval), `preview` (the words, the next runs and
  warnings for the editor), `due` with a `CatchUp` policy (`Skip`, `Once`, `All { max }`) for runs missed while the machine slept, and
  `PauseWindows` (pause windows and quiet hours: spans of local time on chosen days that may cross midnight; `holds`, `resume_at`,
  `next_open`). Time is always passed in (`UnixMs::from_clock`), so every case is a `FakeClock` test. Spotter's `schedule.rs` and
  `calendar.rs` are the base; Debrief's agent timers and Pitcrew's `pause.rs` become adopters. UI: `@decryp7/paddock-ui/schedule` with
  `pd-schedule-editor` (Every / At times / Cron over the engine's preview, warnings and problems) and `pd-pause-editor`, and a
  Schedules page in the shell demo. New dependencies of `paddock-jobs`: `jiff` (bundled tz database) and `croner`.
- The shared CI image (plan X53, `ci-image/`): `paddock-ci` on top of garage with cargo-hakari, cargo-machete, libduckdb, a pinned Chrome for
  Testing and sccache 0.18.0, every download checksum-verified, a verifying last build step and manifest, a manual `ci-image` workflow that
  pushes it from CI. `ci.yml` runs its Linux jobs in it and no longer installs Node, browsers or hakari/machete per job; `setup-rust` skips the
  toolchain install inside it.
- `paddock-testplan` (ADR 0005, plan X52): `.sighterplan` / `.sighterresult` as TOML. `parse_plan` / `parse_result` (4 MiB, 5,000 cases, 200 steps, 4 KiB
  strings, unknown keys named by line, newer majors refused), typed ids and the closed `Step` enum, `Plan::to_toml` (canonical) and `Plan::hash`,
  secrets by reference (`SecretRef`, `Resolver`, `Redactor`, `SecretRedactor`, `lint`), `RunResult::to_toml` with redaction, `Coverage::from`,
  `diff`, `junit::write`; property tests and a fuzz target.
- `@decryp7/paddock-ui/workbench/dock` (plan X50 W2a, W2b): the workbench dock over dockview (optional peer, loaded on first use): `<pd-dock />`,
  `WorkbenchDock` from `provideWorkbenchDock` (`showView`, `closeView`, `openPanels`, `activePanel`, perspectives, named layouts, `verdict`/`onWindowPlan`
  for the Explorer column, checkpoint), `planLayout` adapter with grow-back and a "layout adjusted" notice with Undo, auto-hide edges, floating groups,
  drop guides, pop-out through `PANEL_WINDOWS`, Ctrl+Tab switcher, `pd.wb.dock.*` commands and keys, and a `DockLayoutStore` port (memory, localStorage, IPC).
- `paddock-panels` (plan X50 W3): pop-out panel windows. Validated panel ids (`<view>` / `<view>#<key>`), placement on any display
  clamped to its work area (mixed scales), remembered place per view, at most 12 per host, closing a host closes its pop-outs,
  dock-back events (`paddock://panels`), the drag ghost window (never left behind), and UI-state forwarding only between the windows
  of one host (512 KiB, rate-limited, dialogs get display state only); commands `panel_*`, `panels_list`, `ui_sync_publish`,
  `drag_ghost_*`; generated TypeScript in `workbench/panels/generated`.
- `paddock-assistant` and `@decryp7/paddock-ui/workbench/assistant` (plan X50 W6): the assistant over Pitwall: `LlmProvider` port, prompted tool protocol, `AssistantTool` with budgets and an approval step for tools that write, a bounded cancellable tool loop on an injected clock, versioned bounded conversations, the Pitwall adapter, `assistant_*` commands and the `paddock://assistant` event, and the `pd-assistant` chat pane with its store, safe markdown renderer, context chips, conversation list and tool-approval prompt.
- `paddock-workbench` (plan X50 W2a/W7): the dock's layout store, a bounded `paddock-env` versioned file in the config folder,
  served by `layout_get` [MAIN | POPOUT] and `layout_set` [MAIN] with the `CommandError` envelope.
- `@decryp7/paddock-ui/workbench/palette` (plan X50 W5): `<pd-command-palette>` and `PaletteStore`: the palette's own commands
  and "Open <view>" entries (`>`, recent and common first, word-start, inner and initials matching, "did you mean"), contributed
  providers by prefix (debounced, `AbortSignal` aborted on the next keystroke or close, late answers dropped, capped at 50,
  inline error with Retry), `namesPaletteProvider` over an engine's `names_search` (X18), Mod+Shift+P and Mod+P.
- `@decryp7/paddock-ui/workbench/explorer` and `/workbench/frame` (plan X50 W4): `<pd-explorer>` (one section per Explorer
  provider: lazy, paged with "Show more", cancellable on collapse, middle-truncated names, context menu on right-click and
  Shift+F10, drag data, empty state with a command, errors with Retry) and `<pd-workbench>` with `provideShellWorkbench`:
  title and menu bar, activity bar (badges in the accessible name, fly-outs, overflow into More, pinned bottom items),
  the Explorer side bar sized by the user and closed and reopened by the layout plan in a narrow window, the stage slot for
  the dock, status bar, palette, notifications, dialogs, F6 regions, the Jobs view and "Toggle side bar" (Mod+B).
- `ui/projects/workbench-demo`: the workbench reference app on mock backends (views with layouts and instances, Explorer
  sections, palette providers including `@` over a names search, perspectives, shared state, assistant context, jobs).
- `@decryp7/paddock-ui/workbench` core (plan X50 W1, ADR 0013 §2): the workbench's contribution kinds in the shell's one registry
  (`defineView` with a required `ViewLayout`, `defineActivityItem`, `defineExplorerProvider` (lazy, paged, cancellable),
  `definePaletteProvider` (one prefix each, `AbortSignal`, capped), `definePerspective`, `defineSharedState`, `defineAssistantContext`),
  `provideWorkbenchContributions`, the typed `WorkbenchRegistry` (offered views by `when`, instances as `<view>#<key>` panel ids,
  palette provider by prefix), `VIEW_PANEL` and the `WORKBENCH_PORTS` the frame provides. The shell's `assertContributionsValid`
  checks every kind: a view without a layout or with `min > pref`, targets naming missing views, commands or Explorer sections,
  palette prefix conflicts, perspectives naming missing views.
- Wall-clock limits on the injected clock everywhere (rule 40, follow-up of X20/X13): `CancelToken::on_cancel` (waits are
  event-driven), `FakeClock::{waits_started, wait_for_started}`; `paddock-ipc` `PeerOptions::clock`, `Host::with_clock`,
  `Authenticator`, `Client::connect_on`, per-client handshake threads, `ServerConfig::{max_connections, max_handshakes,
  handshake_timeout, clock}`, `Subscriptions`, `Peer::id`, the Windows discovery-file owner check; `paddock-download`
  `HostCaps::{with_clock, clock, held}`; `paddock-window` `WindowConfig::clock` and `teardown_step_on`; `paddock-health`'s
  debounced writer on the centre's clock; `paddock-proc` `Supervised::terminate` returns `Terminated`; `JobPool::hold`.
  UI: `FakeClock` in `@decryp7/paddock-ui/shell/testing` (wall and monotonic readings, `every`, `after`, `setWall`).
- `paddock-live` and `@decryp7/paddock-ui/live` (plan X29): the live feed. `Hub<S: LiveState>` (state, sequence numbers, bounded
  subscriber queues with latest-wins merging and snapshot resynchronisation of slow consumers), `Frame` (snapshot, delta, link),
  `run` (a `Source` kept connected with `Backoff` on the shared `Clock`, announcing the `Link`), `Channel`, and the `tauri` adapter
  (`paddock://live/<channel>` events, `live_snapshot`). UI: `withLiveFeed` (SignalStore feature: fold, gap resync, link, request
  status), `foldFrame` / `parseFrame`, `tauriLiveTransport` / `MemoryLiveTransport`, bounded list helpers and `Debouncer`; a
  Rust-generated conversation fixture keeps the TypeScript fold equal to Rust's.
- `paddock-packs` Tauri adapter and `@decryp7/paddock-ui/shell/packs` (plan X15/X23): `paddock_packs::tauri` (`Setup`, `install`; commands `packs_status` and
  `packs_reload`; the `paddock://packs` event on every real change; a `paddock-health` entry that goes down with the first refused pack and whose Repair
  (Retry) reloads), `paddock_packs::view` (`kind`, `KindView`, `PacksView`: the pack type erased for the UI), `paddock_shell::Builder::packs`, and the
  Settings > Knowledge packs page (`PdPacksPage`: the packs in use with built-in or yours, each refused pack named with what to fix, ignored keys, Reload as the
  primary button when a pack is refused, "Still working", an inline error with Retry; file names shortened in the middle). `PacksBackend` (Tauri and in-memory),
  `PacksStore`, generated wire types and command names. The shell demo has the page (`?packs=problems|notes|clean`) and a sample kind with a watched
  `packs/` folder.
- `paddock-watch` (plan X23): `Debouncer` on the shared `Clock`, the `ChangeSource` port (`FakeSource`, `NotifySource` behind the
  `notify` feature) with `spawn`, and Debrief's settle rule (`Settle`, `Batcher`, `scan_folder`). `paddock-packs` now watches through it.
- `paddock-net` (plan X16): the outbound HTTP policy. `Egress` (`send`, `stream`, `set_offline`, `download_transport`), `Policy`
  (reach, redirects, caps, timeouts, allow-list, per-host gap), `Reach`/`Class` classification, `Wire`/`Lookup` ports with `UreqWire`
  (paddock-download's TLS and proxy, the address guard on the connecting addresses, one pool per proxy and reach), `FakeWire`,
  `StaticLookup`. A redirect to 169.254.169.254, to the LAN or to this computer from a public start is refused; offline refuses
  before any lookup or socket. `paddock_download::ureq_config` exposes the shared agent configuration.
- `paddock-channels` now sends over `paddock-net` (`EgressPoster`, `channel_policy`; `UreqPoster` is gone), keeps one secret per
  channel (`channel:<id>:secret`), allows plain http only to this computer, and has the settings-page engine `ChannelsControl`
  with the `tauri` adapter (commands and the `paddock://channels` event).
- `@decryp7/paddock-ui/shell/channels` (plan X17, UI half): Settings > Notification channels. `ChannelsBackend` (Tauri adapter and an
  in-memory engine with the engine's validation, secrets in a map and test outcomes `succeed`, `rejected`, `unreachable`,
  `missingSecret`, `hang`), `ChannelsStore` (the engine's refusal of a save shown as it is, a per-channel test state that is
  never timed out, removal clears the secret), and `PdChannelsPage` in `shell/channels/page` (a table with Edit, Send test and
  Remove on each row, one Add channel, an inline editor on Signal Forms, "Still working" and the engine's advice inline).
  The token or password is typed in the modal `pd.channels.secret` dialog (`PdChannelSecretDialog` in `shell/channels/dialog`)
  and goes from that window straight to the engine, never through a store, event, log or URL. The shell demo registers both.
- `paddock-build` (plan X34): the Tauri build step in one call (the Windows manifest in every binary, sidecar placeholders, rerun
  triggers; the manifest is shared, the shell demo gains it); `paddock_proc::self_test` (the `--self-test` of a sidecar);
  `paddock-sidecar` in `@decryp7/paddock-conformance` (`build` and `self-test` for every app's sidecars; the shell demo's
  `agent/sidecar.mjs` is now three lines over it, and `new-app` carries that).
- Conformance checks C34 (sidecars declared for the bundle check and implementing `--self-test`), C34 (every `untimed` call of a
  `withCallTimeout` policy is a declared job-starting or user-waiting command, rule 12; C35) and C36 (parser crates have a fuzz target or a
  never-panics test; warning).
- Reusable workflows `app-fuzz.yml`, `app-mutants.yml` and `app-never-panics.yml` (plan X41, from Debrief's fuzz.yml and mutants.yml);
  never-panics tests with `paddock_testkit::{corpus, never_panics}` for the release manifest and the launchctl report parsers.
- Accessibility checks (plan X49). `@decryp7/paddock-e2e`: `axeInThemes`, `axeTargets` (a list of windows by name), `axeEveryDialog`
  (axe over the window of every dialog in the app's `dialog-contracts.json`, both themes, so a new dialog is covered the moment it is
  declared; an empty list is an error), and the keyboard-only helpers `tabWalk`, `expectTabOrder` (no trap, no stop without a focus
  indicator), `expectReachableByKeyboard` and `expectEscapeReturnsFocus`. `@decryp7/paddock-ui/shell`: `accentProblems`, `cssRules`,
  `resolveTokens` and `definedAccents`, the contrast and colour-vision targets of `test:tokens` as pure functions an app runs on its
  own accent (`test/tokens` now uses the same code; a spec holds every accent of the family to it). `paddock-conformance` C33: an
  e2e spec runs axe, every dialog of the contract file is covered, warnings for a missing keyboard flow or accent test. The shell
  demo's `shell-demo-a11y.spec.ts` runs it all. `ShellDialogs.loaded()` now waits for the dialog shell itself (a window still on the splash passed it).
- `@decryp7/paddock-ui` data parts (plan X21): `/table` (`pd-table`: sortable columns with `aria-sort`, filter, column menu, optional
  group rows, CDK virtual scroll above 100 lines, one tab stop with arrow/Home/End/Page keys, Enter opens, Shift+F10 and right-click
  open the row menu, up to two hover icon actions plus a "..." button and the same list in the menu with destructive items last,
  columns that drop out by the table's own width so nothing scrolls sideways at 320 px, middle ellipsis for paths); `/chart`
  (`pd-chart` over ECharts loaded lazily through `PD_ECHARTS`, theme and palette from the tokens and re-read on a theme change,
  accessible name and text twin, `chartOption` for line, area, bar, stacked bar and donut, escaped tooltips, works with the test
  kit's `FakeECharts` through `fakeEchartsLoader`); `/forms` (`pd-field` and `pdFieldControl` for Signal Forms: label, hint and one error
  with the alert icon at 14 px, when errors show, words for the built-in kinds). Based on Pitwall's and Spotter's `row-nav`/`row-menu`,
  Pitcrew's, Pitwall's and Spotter's chart options and the settings window's field. Gallery sections, Playwright specs (gallery and
  shell-demo; the demo's Runners page now uses `pd-table`) and a contact sheet at `ui/docs/review/x21-data/` for the independent
  review (not approved yet).
- `paddock-archive` (plan X24): safe unpacking of zip, tar, tar.gz and tar.xz. Limits on entries, file size, total size, expansion
  ratio, depth and path length, checked while writing; `..`, absolute paths, drive letters, device names and case-only duplicates
  refused; `Links` (refuse, skip, copy, keep-relative), `Layout` (keep, strip, flat), a name filter, file modes; all-or-nothing
  `extract` through a staging folder that is swapped in; `extract_job` for a `paddock-jobs` job with phases and Cancel; `sweep_stale`;
  a fuzz target.
- `tools/macos-vm-smoke.sh` and the `service-smoke` workflow (C21): the service-after-update smoke runs in a fresh Tart VM (pinned
  macOS base image; `--guest linux` for a systemd --user session with `scripts/shell-demo/smoke-service-update-linux.sh`), always
  deleted, one at a time, with disk and load checks (rule 7).
- `paddock-metrics` (plan X30): the tiered metrics store (`TieredStore<P: Series>`, checked `Retention`, bounded memory and file, late samples
  dropped, `StoreFile` through paddock-env), the machine series and `Sampler` on the shared `Clock` (CPU, memory, free disk of a path's
  volume, the app's own memory with PSS on Linux), `volume_free` / `free_bytes` and a bounded `folder_size`.
- i18n kit (plan X48). `paddock-i18n build [--config paddock-i18n.json] [--check] [--no-extract]` runs an app's whole translation
  build from one declaration: the `localize-extract` run over its built output, every catalog (sources, manifests, extraction),
  the pseudo-locale and each translated `messages.<tag>.xlf` into its runtime JSON together with the shell's translations
  (Debrief's `build-locales` and this repo's `scripts/build-i18n.mjs` are replaced by it; `--check` writes nothing and fails on
  drift, `--no-extract` reads the committed catalogs so no build is needed). `LanguageChoice` now also names a shipped
  translation by its tag, `isLocaleTag` validates one, and `@decryp7/paddock-ui/shell/i18n/page` is the Settings > Language page
  (`PdLanguagePage`, `defineLanguageSettingsPage`): System default, English, each translation by its own name, the pseudo-locale
  where the app ships it, a restart offer. `paddock-conformance` C32 runs `build --check --no-extract` over the app's
  `paddock-i18n.json`.
- `app_relaunch` (`paddock-startup`, a window-class command: any window, a dialog lists it in its contract) and
  `StartupBuilder::relaunch(|app| app.restart())`: the checkpoints are written to the session first, then the app's relaunch
  runs, so the whole app restarts (a language change from the Settings window); without a hook it answers `relaunchUnavailable`,
  a failing one `relaunchFailed` and the app keeps running. The shell demo wires it; the Language page calls it through
  `LANGUAGE_CONFIG.reload`.
- The app IPC kit (plan X28). `paddock_commands::CommandError` / `ErrorCode`: the one error envelope (code, message, retryable,
  hint, field, cause, kind), with constructors, `From` for `io::Error`, strings and the guard's `Refusal`, and its TypeScript
  generated into `@decryp7/paddock-ui/ipc`. `@decryp7/paddock-ui/ipc`: `toCommandError`, `CommandFailure`, `withCallTimeout` (a
  Proxy with a default, per-method limits and `untimed` for job-starting commands, rule 12) and `callTimeoutProblems`,
  `provideBackend` (Tauri or a lazily imported mock). `@decryp7/paddock-ui/ipc/testing`: `parseMockOptions` / `formatMockOptions`
  (`?mock=&latency=&fail=&hold=`) and `withMockOptions` (latency, failures, held answers, recorded calls).
- `paddock-bench` (plan X26, rule 16): the benchmark harness with stored budgets. `Recorder` (median of several runs on an
  injected `Clock`; `ManualClock` for tests), `Kind` (startup, throughput, first event, query latency, memory: unit and
  direction), `Results` (versioned results file per app, atomic, bounded), `Budgets` (`budgets.toml`: limit, tolerance, regression
  against the previous run, `propose`), `check` (pass, tolerated, over budget, regressed, missing, wrong kind, unbudgeted;
  text, Markdown, JSON) and the `paddock-bench check|propose` command (exit 5 on a miss). `.gitea/workflow-templates/bench-nightly.yml`
  is the nightly workflow template.
- `paddock-cli` (plan X25): the family's command-line conventions. `Exit` (0 ok, 1 failure, 2 usage, 3 not running, 4 refused,
  5 findings, 130 interrupted), `CliError` with a next step, `finish` (one JSON object on stdout in `--json` mode, errors
  included), `Table` (text, CSV, JSON; bounded cells), `Progress` (throttled, de-duplicated phase lines on stderr, injected time)
  and `StopFlag`, `Help` and `version_line`, `CommonFlags`, `parse_time`, `parse_size`, `format_size`. `paddock_service::Verb`
  describes itself for `--help`, rejects extra arguments with a hint and decides `status`'s exit code.
- Paddock runs its own conformance checks in CI (C06, C14, C15, C16, C29, C30, C31; the app-only ones are skipped with reasons in
  `ci.yml`); C24 runs non-blocking until the `docs/images` review lands. New check C31: ADR numbers are unique. The waiver of a
  Rust test sleep (C30) must give a reason: `// conformance: hang-guard — <why>`.
- Tests no longer sleep to wait for other threads: `paddock-jobs` `tests/parallel.rs` and `tests/pool.rs` (gates, a barrier and
  event waits) and `paddock-window`'s exit-watchdog test (the process end is a channel event).
- `paddock-names` search index and `names_search` (plan X18 follow-up): the catalogue keeps hash maps for the exact, joined and alias
  rungs and an inverted index of words (prefix by range, inside-a-word by scan, typos by letter-bag and edit distance), so a
  search over 250,000 names takes about 20 ms (was 500 ms; `cargo bench -p paddock-names` fails over 50 ms). `search_scan` ranks every
  entry and a property test checks that both give the same answer. `NamesService` + `NamesSearchRequest`/`NamesSearchReply`
  are the engine side of the palette's `names_search` command (arguments validated in Rust, catalogue swapped whole).
- Conformance check C24 (rule 22): every image under `docs/screenshots` matches its `hashes.json` entry and has a `REVIEWED.md` row
  (image, SHA-256, reviewer, date) with the current hash; `docs/images` needs the row. A regenerated image that changed fails until
  it is reviewed again.
- `@decryp7/paddock-e2e` `paddockConfig()` and `captureDocScreens()` (plan X39). The config preset: projects Chromium and WebKit
  (plus `screenshots` only with `SCREENSHOTS=1`), the port from an environment variable (required on CI), Chrome for Testing
  only through `CHROMIUM_TEST_PATH`, `retries: 0`, traces on failure, blob reports for shards. The runner takes the five apps'
  screenshot tables: both themes at 1440 x 900 @2x, frozen clock, long pages framed, `docs/screenshots/<theme>/<name>.png` and a
  `hashes.json` for the review gate (`clock: 'paused'` stops the fake clock at exactly the frozen time). Node tests for the pure parts and a Playwright self-test on `data:` pages.
- `paddock-testkit` (plan X40): the Rust test kit. `Scratch` (work, home and tmp folders, a leftover check on drop), `env_lock()` and
  `EnvLock::hermetic` (HOME, APPDATA, XDG_* and TMPDIR in the scratch folder, restored afterwards), golden files with
  `PADDOCK_UPDATE_GOLDEN`, `FakeRunner` for OS-command seams, `corpus`/`never_panics`, and `FakeClock` re-exported from
  `paddock_jobs::time`.
- Conformance check C30 (warning level, rule 40): a `#[test]` fn that compares a duration measured with `elapsed()`,
  `Instant::now() - x` or `duration_since` against a limit; a `thread::sleep` in a test that uses threads; and `Date.now()`/`performance.now()`
  differences under `expect` in TypeScript specs. A hang guard of at least 10 s may carry `// conformance: hang-guard`.
  First findings in Paddock: `paddock-diag` window.rs, `paddock-proc` login.rs and supervised.rs, `paddock-updater` tests,
  `paddock-window` exit.rs and tauri/tests.rs.
- `paddock-names` (plan X18): identifier search per rule 20, from Debrief's `names/` (segmentation with the embedded word list,
  normalisation, grouping of spellings, typo distance, the ranking ladder and the clarifying question) and Dyno's counter search
  (every word in any order, other fields ranked below the name). `Catalogue::new` / `text` / `with_vocabulary`, `Entry`,
  `search` -> `Search { matches (how, why), clarify }`, `Style::{Path, Text}`. Pure and bounded; property tests for
  never-panic, determinism and self-lookup.
- `paddock-channels` (plan X17): outbound notification channels (webhook, ntfy, e-mail) from Pitcrew's `notify-channels` and
  Spotter's `mail.rs`. `Settings` with `validate`, secrets through `paddock-secrets` (`channel:<id>`), bounded `Message`s,
  `Dispatcher` (parallel channels, retries with `paddock_jobs::time` back-off and `Retry-After`, cancel, panics contained,
  `test` / `test_with` for the settings page), ports `HttpPoster` (`UreqPoster` over `paddock-download`'s agent, proxy and OS
  certificates) and `MailSender` (`LettreSender`), `FakePoster` / `FakeMail`. `paddock_download::ureq_agent` is new: the one
  place the TLS and proxy set-up of a ureq agent lives.
- `paddock-ipc` (plan X13, ADR 0001 step 1): the family's one IPC from `debrief-ipc` and `dyno-ipc` (with Pitcrew's service
  pipe SDDL). Postcard frames with a skip of oversized frames (one call fails, not the link), private endpoints, the token +
  pid + protocol-version handshake, a thread-based peer (calls both ways, streams with credits, cancel, pushes, busy, typed
  errors), the launcher on `paddock-proc` and the child loop, and the companion `link`: an owner-only discovery file
  (stale and untrusted files ignored), `LinkServer`, and `LinkClient` with reconnect back-off on `paddock_jobs::time` and
  replacing a companion of another build. A frame-codec fuzz target (`crates/paddock-ipc/fuzz`); JSON numbers in `Json`
  now cross unchanged (`float_roundtrip`, found by the fuzzer).
- `paddock-packs` (plan X15, decision D11): knowledge packs for all apps. `PackKind` (serde type, format number, the app's own
  rules), `parse_pack` with `Limits` (file size, entries, string length, nesting, values, files), unknown keys as named warnings,
  a bad pack refused whole with a reason while the rest load (last good version kept), `PackLoader`/`PackStore` (build new, then
  swap, observers told once per real change), `Reloader` (debounced hot reload on the fake-able `Clock`), the `ChangeSource` port
  with a `notify` adapter (feature `watch`; X23 takes it over), `Snapshot::health()` and `user_regex` (length and size caps, typed
  errors; merges Debrief's and Spotter's copies).
- Shell demo: the sample agent is its own crate, bundled as `externalBin` on every OS (`agent/sidecar.mjs` in
  `beforeBuildCommand`), ad-hoc sealed with the app; `shell-demo-agent --self-test` runs from the built bundle in the macOS
  verification (which also checks every Mach-O's TeamIdentifier) and from the .deb / the bundled .exe in the release
  workflow. `scripts/new-app.mjs` carries the agent into new apps, renamed.
- `@decryp7/paddock-ui/shell/pitwall` and `paddock-pitwall`'s `AiSnapshot` (ADR 0004): the family's one `<pd-needs-ai>` component.
  It shows the engine's sentence for a missing, stopped, unpaired, paused, incompatible or model-less Pitwall and only the
  actions the engine offers: Connect (the six-digit code to compare, Cancel), Use the built-in engine for this session (the
  repair that does not depend on Pitwall, rule 19), Open AI settings, and `(step)` for what only the app can do (open or
  install Pitwall, update, change permissions). `PitwallBackend` port (Tauri over the four `PITWALL_COMMANDS` and the
  `paddock://pitwall` event, and an in-memory engine), `PitwallStore` (SignalStore, request status union, listens to the event),
  `providePitwall`. The wire types and names are generated from Rust (`ts-rs`, stale files fail `cargo test -p paddock-pitwall`).
  `Session::snapshot` builds the value. The shell demo has a Local AI page on a fake Pitwall (Vitest and a Chromium spec: axe,
  320 px).
- `paddock-pitwall` line coverage is above 99% (lcov): the socket error arms are classified by pure functions with tests, and the
  pairing races (an approval after the request expired, a late answer for a replaced request) are tested deterministically.
- `@decryp7/paddock-ui/shell/service` (plan X4): Settings > Background service. `ServiceBackend` (Tauri adapter and an in-memory
  one that plays install, a repair's phases, failure, busy, switched-off and needs-approval), `ServiceStore` (status, the
  `paddock://service` updates folded in, "Still working" while a repair runs and never timed out, an inline error with
  Retry), `defineServiceSettingsPage` for the `pd.login` page and `SERVICE_SETTINGS_COMMANDS` for the Settings dialog's
  contract, and `PdServicePage` in `shell/service/page` (a status word, the engine's message, Install, Repair and Open Login
  Items… only where the engine says they apply, the "Start at login" switch beneath in the generic settings form). The shell
  demo shows it (`?service=notInstalled|switchedOff|...`, `?service-outcome=fail|hang|...`).
- `paddock-service` in the app (plan X4b): the Windows service host (`--service`: the program in the user's session,
  restarted, moved with the session, stopped gracefully) behind a `Sessions` port; `ServiceControl` and the `tauri`
  adapter (`service_status`, `service_repair`, `service_install`, `open_login_items`, the `paddock://service` event, the
  health entry and its Repair, start-up checks); `paddock_shell::Builder::service` with the `pd.login` "Start at login"
  section; `RepairAction::OpenLoginItems` in `paddock-health`; the shell demo's sample agent (`shell-demo-agent`, its
  generated plist and NSIS hooks) and `scripts/shell-demo/smoke-service-update.sh` (C21, disposable Macs only).
- `paddock-service` (plan X4, decision D4, rule 24, catalogue R03/R05): background services and login items. One
  `ServiceSpec` rendered (pure, golden-tested) as the bundled launchd plist for `SMAppService` or a plist file, the systemd
  user or system unit, the Windows `Run` value, the Windows service as `sc.exe` calls and the NSIS installer hooks (they
  replace the hand-written `src-tauri/macos/*.plist` and `src-tauri/windows/installer-hooks.nsh`). Backends for
  `SMAppService`, plist + `launchctl`, systemd, the `Run` value and the Service Control Manager (directly or through one UAC
  prompt), all behind the `System`/`AppService`/`Elevator` ports with recording fakes (`testing`). `plan` + `repair::replace`:
  unregister, `launchctl bootout`, wait until gone (bounded), register, check, retried; once per version change where the
  OS pins the build, automatically for a refused job while the service is down, a missing or moved program re-pointed, a
  newer copy's registration left alone, the user's switch never overridden, nothing installed unasked. `Manager` (status,
  `on_start`, repair, install, uninstall, the registered version in `service-<label>.json`, a cross-copy lock,
  `RepairTracker` progress), `ServiceStatus::health` for `paddock-health`, the `pd.login` launch-at-login section, the
  exe-changed restart (`watch`), the login-shell `PATH` for a supervised program (`supervised`) and the shared verbs.
- `paddock-pitwall` (ADR 0004, plan workbench section 15): the one Pitwall client, extracted from Debrief 1.1's engine and checked
  against Pitwall's router. Discovery (`discovery.json` from `paddock-env` folders, Local then Roaming on Windows, else port 11435),
  the key in `paddock-secrets`, pairing with the user's consent on a background thread, a typed `Client` (health, select, models,
  jobs, tokens, tasks, embeddings in batches, chat whole or streamed, cancel within a read slice, provenance from `X-Pitwall-*`,
  capability checks with Pitwall's reasons), retries with back-off and `Retry-After` that never repeat a stream after its first delta,
  `PitwallError` with `user_message`/`next_step`, `AiState` and `NeedsAiView` for the shared needs-AI component, the Automatic /
  Pitwall / Built-in choice with `run_or_fallback` (never a silent mix), health reporting whose Repair does not depend on Pitwall, the
  bounded loopback HTTP transport (also for Debrief's llama-server client, X16 note) and a fake Pitwall (`testing` feature) with a
  contract test and an optional check against Pitwall's source.
- `paddock_jobs::time` (plan X20): one clock for the family. `Clock` (monotonic and wall readings, a cancellable `wait`),
  `SystemClock`, `FakeClock` (moves only with `advance`; `wait_for_waiters` lets a test advance once the work is blocked on
  it), `Deadline`, and `Backoff`/`BackoffPolicy`/`Retry` (Pitwall's restart policy; `retry_forever` for reconnecting
  clients). `CancelToken::with_deadline_on(clock, limit)`; `JobRegistry::register_with_deadline` now measures the limit on
  the registry's clock, so a fake clock runs a job's time limit out without sleeping.
- OS notifications (plan X8): `paddock-health` `notify::os` and `tauri::os`/`tauri::native` (feature `os-notify`), the four
  adapters (Debrief, Spotter, Pitcrew, Dyno) as one. One policy (`OsSettings`: level off/alerts/all, quiet hours across
  midnight with an optional critical bypass, cool-down and burst limits), `screen` and `user_sees` pure, `RateLimiter` per
  cause key on the injected clock, `OsNotifier` over a `Toaster` port (`PluginToaster` over `tauri-plugin-notification`,
  `FakeToaster` for tests), toasts with up to three labelled buttons, and `route` for clicks: open the message, repair in the
  shell, the app's handler (`Setup::on_notice_action`), or `paddock://notifications-action` to the window
  (`NOTIFICATIONS_ACTION_EVENT`, `NotificationsBackend.onAction`, `NotificationsStore.runFromOs`). `Setup::os_notifier`
  installs it, with a quiet-hours digest ("N notifications held during quiet hours" once when they end, rate-limited,
  `Setup::os_digest_every`); `paddock-shell` adds the `pd.notifications` settings page (`Builder::os_notifications`) and the `os-notify`
  feature. The shell demo shows the Notifications settings page and runs the notifier (the real plugin installed, a fake
  toaster in tests); `OS_NOTIFICATIONS_CONTRIBUTIONS`, `NOTIFICATIONS_SECTION_FIXTURE` and `OsActionRunner` are the UI side.
- `paddock-diag` and `@decryp7/paddock-ui/shell/diagnostics` (plan X2, decision D9): Report a problem, the three designs of Debrief,
  Dyno and Pitcrew as one. A diagnostics zip of the app's log (newest part, read from the end), system facts, settings (secrets
  masked) and crash marker, plus the sections an app adds; one `Redactor` (secrets from Pitcrew and Debrief, personal data from Dyno,
  `paddock-log`'s field rules; a log line stays valid JSON); a preview that equals the zip, file by file with sizes; an atomic zip
  that a cancel never leaves partial. Nothing is sent: the Tauri adapter asks where to save in a native dialog and writes the zip as a
  job (`diagnostics_preview`, `diagnostics_save`, `diagnostics_cancel`, the `paddock://diagnostics` outcome). The UI is a decision dialog
  (description, the file list, Save, Stop saving), a Help menu item and the "Logging" settings page. `Builder::diagnostics` in
  `paddock-shell` installs it and registers the `pd.logging` section ("Diagnostics > Logging": level, MB per day, days kept) on the
  `LogHandle` (the level applies at once), and the new `log_from_webview` command (with `provideWebviewLog`) puts the page's errors
  in the engine's log with their window. The shell demo shows it (browser and desktop).
- Shared sccache backend (plan X10): `setup-rust` takes `sccache-redis-endpoint`/`sccache-redis-db` (defaults from `vars.SCCACHE_REDIS_*`); every OS uses the shared Redis database 5 (`sccache-backend: auto|redis|local`); a pinned, checksum-verified sccache is installed when the runner has none; `CARGO_INCREMENTAL=0`, `SCCACHE_IGNORE_SERVER_IO_ERROR=1`; new `sccache-stats` action; used in Paddock CI.
- `paddock-proc` (plan X6): child processes. `command` (no console window on Windows), `clean_env` and `lower_priority`;
  `run_bounded` (timeout, output cap, the whole tree ended, typed `RunError`); `Supervised` children (own process group,
  `terminate` with grace, tree ended on drop, an optional `MemoryCap` with warning, stop and peak, a `Registry` and
  `end_all` for shutdown); `process_memory` / `tree_memory` (PSS from `smaps_rollup` on Linux, thread entries skipped);
  `kill_tree`, `ask`, `end`, `terminate` for single pids; `login_path` and `merge_path`.
- `paddock-download` gaps for 0.3.0: **system proxy** (`ProxyMode::System` is the default: `HTTPS_PROXY`/`HTTP_PROXY`/
  `ALL_PROXY`/`NO_PROXY` in either case, then macOS system configuration (`scutil --proxy`) and Windows Internet Settings or
  WinHTTP; loopback never proxied; `NoProxy` knows domains, `.suffix`, `*` patterns, IPs, CIDR, `<local>` and ports),
  explicit `ProxyMode::Rules`/`ProxyMode::explicit`, authenticated proxies through `ProxyServer::with_credentials` or the
  address (Basic, percent-decoded; the password never appears in `Debug`, `Display` or an error), `HttpTransport::reload_proxy`.
  Also `Request::header` (validated, values hidden), `Request::resumable()`, `Downloader::download_with_cancel`,
  `Downloader::{get_json, get_text, get_bytes}` with `Fetch`, `NotEnoughSpace.path`, `phases::{ALL, *_LABEL, UNPACK,
  INSTALL, SELF_TEST, is_download_phase}`, `Downloaded::sink_panics` and `Reporter::sink_panics` in `paddock-jobs`
  (an `EventSink` panic was dropped silently). The engine stays thread-based and blocking (documented: use `spawn_blocking`
  or the `JobPool`); there is no private async runtime.
- Shared CI, lint and repo baseline (plans X10, X11, X27, X42, X43, X46, X47; `@decryp7/paddock-conformance` 0.2.0):
  - `.gitea/actions/*`: composite actions `plan` and `nightly-mark` (skip a nightly while main is unchanged since a green
    one, skip a tagged main push), `cache-restore` and `cache-save` (restore, then save-always), `setup-rust` (sccache),
    `setup-node`, `strip-signing-keys`, `build-env` (`PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE` for the About page, which no
    CI set before), `tauri-bundle`, `macos-verify`, `install-test-browser` (pinned Chrome for Testing), `licences`,
    `conformance`, `publish-release` (notes with the First launch block, `latest.json`, the release) and `host-cleanup`; and the
    reusable workflows `app-release`, `app-e2e` and `app-licences`. Their scripts are in `conformance/ci/`, tested. C03, C04
    and C15 recognise them.
  - `@decryp7/paddock-eslint` (`eslint/`): the flat-config preset with rules 27 to 39 as rules (OnPush written out, zoneless,
    effect justification, signal reads before `await`, request status union, store file, scope and injection count,
    `patchState` only in stores, no `[ngClass]` or `[innerHTML]`, raw colours and fonts in styles) on angular-eslint and
    typescript-eslint; Debrief's `lint-effects` and `lint-styles` are rules now. `lint/clippy.toml` and
    `lint/workspace-lints.toml` are the Rust presets; check C13 keeps an app's copies a superset.
  - Repo baseline: `templates/repo/` (editorconfig, rustfmt, licences, `deny.toml`, `cliff.toml`, `lefthook.yml`,
    `about.toml`, Renovate preset, tsconfig, Cargo profiles), `renovate/app.json` (`local>decryp7/paddock//renovate/app`, the
    Paddock crates and packages in one PR per release), `@decryp7/paddock-tsconfig`, check C28 and
    `paddock-conformance sync [--write]`. `docs/blocks/` holds the shared AGENTS.md sections, suite lessons, releasing and
    First launch text; C23 keeps the marked blocks equal. `new-app.mjs` now writes the repo root from both.
  - `paddock-notices` (X42): `THIRD_PARTY_NOTICES.md` from cargo-about and the npm lock, deterministic, `--check` for CI; the
    licence list is one list shared by `deny.toml`, `about.toml` and the generator.
  - C29 (X46): window weight budgets from `ng build --stats-json`, `paddock-conformance window-weight --record` writes measured
    plus 10%.
  - `tools/` (X47): `bootstrap.sh`, `verify-installed.sh`, `linux-container.sh`, `prune-build-folders.sh` and `.ps1`,
    `host-cleanup.sh`, with the app as a parameter.
  - Shell Demo reads its build commit and date from `PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE`.
- `@decryp7/paddock-ui` UI basics (plan X12, X35 to X38 and the shell gaps): `/icons` (the generic `pd:` set, `outlineIcon`,
  `registerPaddockIcons`, `pd-glyph`), `/format` (bytes, durations, relative times, counts, middle ellipsis), `/prefs` (guarded
  localStorage), `/select` (`pd-select`, themed non-native select), `/testing` (`installTestKit`, DOM helpers, fake ECharts),
  `/shell/feel` (native-feel guard, installed by `provideShellPages`), `pd-path-text`, `pd-copy-menu`, `PdClock`, the
  `pd-empty-state` icon, `css/controls.css` and `css/material.css`, `DialogRef.setTitle`, `SettingsPageDef.subgroup`,
  `SettingsStore.setNotice`; every shell and library CSS class is now `pd-` prefixed (breaking for app CSS that targeted them).
  Contact sheet for the drawing review: `ui/docs/review/contact-sheet.png`.
- `paddock-env` (plan X7, decision D3): folders and state files. `AppDirs` (data, config, cache, logs, runtime per OS from an
  injectable environment; `<ID>_HOME` override in debug builds only; portable mode; Windows is Local, the old Roaming folder
  is named for the one-time move) and `VersionedFile<T>` (atomic write with fsync and `.bak`, migration chain, a damaged file
  is kept as `.corrupt-<time>` and defaults or the valid backup load, a newer version is refused and left untouched).
- `paddock-log` (plan X1, decisions D1 and D2): the app and service log the five apps wrote five times. CLEF JSON lines, one
  file per process per day in `AppDirs::logs()` with a daily size budget (then only errors) and old days removed, the level
  changed at run time, `log` and `tracing` in one stream, `ipc_span` and `log_from_webview` for `Window`/`OperationId`/`Command`,
  a `Redactor` that never writes file contents (hooks for the app's rules, `fingerprint`), error de-duplication with
  breadcrumbs, a panic hook with a crash marker for X3, child-process output capture and the `LogSettings` of the
  "Diagnostics > Logging" section. No Tauri dependency: services and sidecars use it as is.
- `paddock-secrets` (plan X5, decision D6): the credential store the five apps copied. A `Secrets` port, a `Secret` that never
  prints and is zeroized, the keyring adapter (`keyring` feature, refused in tests unless `PADDOCK_SECRETS_REAL=1`),
  `MemorySecrets`, an opt-in owner-only file fallback (0600), one naming scheme and `rename_entry` for old entries.
- `@decryp7/paddock-conformance` (`conformance/`, plan X9, not published yet): the `paddock-conformance` CLI with findings
  (text, JSON, Markdown job summary), waivers that expire (`paddock.waivers.toml`), and checks C03 (icons), C04 (macOS
  signing and bundle), C06 (key strip), C07 (version agreement, release gates, tag preflight), C08 (release scripts and
  artifacts), C14 (hermetic test runs, `run --`), C15 (installed Chrome), C16 (hard-coded app version) and C23's first part
  (`commit-lint`: Conventional Commits, no Co-Authored-By or AI attribution). The apps' release scripts are unified under
  `conformance/release/` (`publish`, `latest-json`, `strip-keys`, `macos-verify-bundle.sh`, which captures output before it
  matches); `latest.json` is byte-equal to the apps' copies. CI job `conformance` runs its tests.
- `paddock-startup` lifecycle (plan X3, from Debrief's `lifecycle.rs`): unclean-shutdown detection, safe mode and the autosaved
  UI session. `Lifecycle::begin` writes a running marker (a `paddock-env` `VersionedFile`: pid, start time, safe mode) and
  `end` removes it on a clean exit; the next start calls the run a crash when the marker's process is gone (PID reuse is
  caught by the start time; a still-running predecessor is taken over; a damaged or newer marker counts as a crash) or when
  `paddock-log`'s `CrashMarker` is there (thread and place are told to the UI, the panic message never is). A separate
  instance (`--new-instance`, always safe mode) keeps its state in `instances/<pid>/`, so it never disturbs the main
  instance's marker or session; dead ones are pruned. `Mode`/`Feature` make safe mode one answer every part reads
  (extensions, plugins, last session off). `SessionStore` + `Autosaver` keep each window's `ui_checkpoint` in a versioned
  session file on a background thread (atomic with backup; torn, damaged or newer files start clean with a notice; bounded
  labels, sizes and windows); the previous run's session is kept as the recovered one until the user decides. Commands
  `recovery_status`, `recovery_restore`, `recovery_start_fresh`, `recovery_safe_mode` (main windows), builder option
  `lifecycle(..)`, `tauri::end_run` (the shell calls it at exit), feature `os-probe` for the process table.
- `@decryp7/paddock-ui/shell/startup`: `RecoveryStore`, `PdRecoveryNotice` (the unclean-shutdown notice: Restore / Start in
  safe mode / Start fresh; only what Rust offers; a failed choice stays visible) and `PdSafeModeBadge` (title bar), both placed
  by `PdShellFrame`; `UiCheckpoint.restoreWindows`; the backend's `recovery*` calls with browser mocks
  (`paddock.mock.unclean`, `paddock.mock.safeMode`).
- Paddock Shell Demo (`apps/shell-demo`, plan S18): the shell demo as an installable Tauri app on the `paddock-shell`
  plugin, with a sample engine (a service whose Repair works and one whose Repair fails, jobs with phases and Cancel,
  settings sections, the tray popover), its own neutral icon from the icon pipeline (`icons/shell-demo`), ad-hoc macOS
  signing with the strict verify, and its own release workflow on `shell-demo-v*` tags. In the app the demo's UI uses
  every Tauri backend of the shell (health, notifications, jobs, dialogs as windows); in the browser its stand-ins.
- `node scripts/new-app.mjs <id> "<Name>"`: a new app from the demo, renamed, in the family layout, on the registries.
- The `graphite` accent (`data-accent="graphite"`, `SU_ACCENTS`): the neutral accent of the demo apps, so a demo never
  wears an app's colour; held to the same token tests as Pacenotes and Sighter.
- The jobs status item shows the lead job's phase and value beside its name ("Reading 40%", "Finding files 1,240 so
  far"; rule 11), a ring filled to its share on a known scale, and the jobs glyph when idle (`jobsStatusProgress`).

### Fixed
- The About, Updates and What's New contents were unstyled: their stylesheet still used the old class names (`.btn`, `.muted`, ...)
  while the templates use the `pd-` ones, so the legal-text buttons failed colour contrast in the light theme (found by the new
  axe check over every dialog window). The shell demo's dialog buttons set white text instead of `--pd-on-accent` and failed
  contrast in the dark theme.

### Changed
- Breaking (rule 40): `paddock_jobs::assert_nonblocking(&pool, handler)` holds the pool instead of measuring a budget
  (`Blocked`, `BLOCKED_AFTER`; `HANDLER_BUDGET_MS` and `SlowHandler` are gone); `HostCaps::back_off(host, wait)` takes a
  duration on the caps' clock; `shell/testing`'s `ManualClock` is now `FakeClock` (ticks fire as often as they fall due).
- Breaking (plan X20): `paddock-jobs`, `paddock-log` and `paddock-health` share one `Clock` (`paddock_jobs::time`). The old
  `ManualClock`s are gone: use `FakeClock` (`new()`, `at_wall(..)`, `at_wall_ms(..)`, `advance(Duration)`/`advance_ms(u64)`).
  `paddock-health`'s epoch reading is `Clock::wall_ms` (was `now_ms`); `paddock-log`'s UTC reading is `paddock_log::now_utc(&clock)`
  (was `Clock::now`); `paddock_health::clock` is removed (the types are at the crate root). `SystemClock` is a unit struct
  everywhere (`SystemClock::new()` is gone).
- Breaking, `paddock-download`: `HttpConfig::proxy` is a `ProxyMode` (was `Option<String>`, and the environment was never read;
  now the system's proxy is used unless `ProxyMode::Direct`); `Get` has a `headers` field and `Transport` implementations
  must send them; `DownloadError::NotEnoughSpace` has a `path` field and its message changed; `Downloaded` has a
  `sink_panics` field. `HttpConfig` is no longer constructible with a proxy string: use `ProxyMode::explicit(..)`.
- The tray popover layout has one 16 px inset for its header, content and footer.
- `pd-waiting-for` (and `pd-service-error` while it waits) draws a stopped service as an offline state: the warning icon,
  "<Service> stopped", one muted line and the status chip's repair; it spins only while a start or repair is under way
  (`waitingKind`). The line says what is missing and what brings it back ("Engine data is unavailable until the demo
  engine restarts.": `subject` input, `offlineLine`), and a stopped service has one severity everywhere, danger
  (`serviceStatus`: chip, panel icon, a tray's pill and banner).
- The chosen segment of `pd-segmented-control` is raised (surface-3), also in dark.
- The narrow jobs status item says "1 job" / "3 jobs" instead of a bare count.
- Sentence case for the shell's About items: "Check for updates…", "What’s new" (and "What’s new in <version>").
- The shell demo's product name is "Paddock Shell Demo"; its Rust side is the app (`apps/shell-demo/src-tauri` writes
  `app-commands.ts`); `paddock-shell`'s `tests/demo.rs` keeps only the plugin's checks over the demo's files.

### Fixed
- `paddock-service` (C21 VM smoke): the first Install of a bundled macOS agent failed on every fresh Mac with "macOS cannot find
  ...'s launch agent". `SMAppService.status` is NotFound until the app registers once; that now reads as "not installed" and
  Install registers (a real failure comes from `register`, with macOS's reason in the message). `parse_job` reads only the job's
  own level of `launchctl print`, so macOS 26's coalition blocks (`state = active`) no longer make a running agent "refused".
- The service smoke builds N+1 with the agent's version bumped too (it reported the old version for ever), installs to
  /Applications as a user does, and checks the app's own status is "running" after install and after the update.

## 0.2.0 - 2026-10-24 (the first shell release)

The app shell of ADR 0012 (plan S0-S17): `@decryp7/paddock-ui` 0.2.0 with the `shell/*` entry points,
`@decryp7/paddock-e2e` 0.2.0, and the shell crates on the Gitea cargo registry `decryp7` for the first time:
`paddock-shell` 0.2.0 (the Tauri plugin; lockstep with the npm package) and `paddock-commands`, `-dialogs`, `-download`,
`-health`, `-jobs`, `-menu`, `-settings`, `-startup`, `-updater`, `-window` 0.1.0. The date is set by the tag.

### Migration
- From 0.1.0: bump `@decryp7/paddock-ui` to `^0.2.0` and add `@angular/localize/init` to the polyfills (a peer
  dependency now); the root entry's components and tokens are unchanged. No app has adopted the shell yet: adoption is
  new work per app (ui/README.md "Adopting the shell", crates/README.md "Consuming the crates"), Pitwall first.
- Apps take the crates from the registry (`.cargo/config.toml` `[registries.decryp7]`), never from a git tag.

### Added
- Crate `paddock-shell` 0.2.0 (plan S17, ADR 0012 decision D2): the shell as one Tauri plugin. `Builder` manages the
  shell's state (jobs, health and the notification centre, settings with the shell's sections, dialog contracts, the
  start-up watchdog, windows, menu, updater) and its hooks; every shell command is `plugin:paddock-shell|<command>`
  behind one guard with the per-dialog grants; `shell_version`; permission sets `main` (`default`), `popout`, `dialog` and
  `tray` generated from the table and resolved through Tauri's ACL in the tests; `Builder::problems` checks the app's
  table, dialog contracts and `paddock-manifest.json` (menus and keys, dialog commands, health services, settings sections)
  against the plugin.
- `paddock-commands`: composed tables (`command_table! { static SHELL = [A, B]; }`, a command two parts declare does not
  compile) and `tauri::Routes` (each call to the handler of its part, behind one guard).
- `paddock-health`: `notifications_post`, a window's notice to the engine's centre (validated and bounded, keyed by the
  engine as `ui:<kind>:<title>`, app or window scope), so `SHELL_PORTS.notify` reaches every window (rule 34).
- `paddock-settings`: the shell's `pd.updates` section (`shell::updates`); `paddock-updater`: `PreferenceStore`,
  `Updater::apply_preferences`.
- `@decryp7/paddock-ui/shell`: `shellInvoke` (the shell's commands to the plugin, the app's as they are),
  `checkShellVersion` / `provideShellVersionCheck`, `SHELL_PLUGIN`, `SHELL_VERSION`, the generated `SHELL_COMMANDS` of
  the plugin; the manifest's `settingsForms`.
- `shell-demo`: `paddock-manifest.json` written from the main window's contributions and checked by the demo's Rust side
  (`crates/paddock-shell/tests/demo.rs`, which also generates its `app-commands.ts`); the desktop build sends the shell's
  commands to the plugin and checks its version.
- CI: the `paddock-shell` crate step; the publish dry run refuses a `paddock-shell` version other than the npm package's;
  the nightly consumer canary (`.gitea/workflows/canary.yml`, ADR 0012 §8).

### Changed
- `SHELL_PORTS.notify` posts to the engine (`NotificationsStore.post`, async); a window keeps a notice itself
  (`postLocal`) only when the engine cannot take it. `NotificationsBackend.postNotice` is new (Tauri and memory).
- The update preferences live on the `pd.updates` settings section (one writer and source, rule 40):
  `updates_set_preferences` stores through the settings, and `updates.json` no longer holds `autoCheck` or `channel`.
- `paddock_dialogs::BASE_COMMANDS` adds `notifications_post` and `notifications_list` (every window may post a notice and
  read its host's list); `DIALOG_BASE_COMMANDS` and the dialog command check follow.
- Each crate's unguarded handler is `invoke_handler()` (`paddock-startup`, `-updater` and `-window` renamed `handler()`;
  `paddock-dialogs`, `-menu`, `-jobs` and `-settings` gained one; `paddock-dialogs` and `paddock-menu` have their
  `COMMANDS` tables). `paddock-commands` no longer writes `shell-commands.ts`.
- `paddock-health`'s generated TypeScript moved into `shell/notify/generated/`; the mirrored wire types and their drift
  spec are gone (plan §13). The e2e kit's Tauri fake and the IPC recorder record the plugin's commands by plain name.
- `paddock-menu`'s manifest check counts a command once per menu-bar menu only: a context menu may repeat it (lint
  precision, rule 40).

### Fixed
- `paddock-dialogs`' contract-file test named the shell demo's dialogs before S10/S11 added `pd.*` ones.

### Changed
- Accents for Pacenotes (route-map olive: `#8da74e` dark, `#576d07` light, fill `#657c1f`) and Sighter (optical plum:
  `#c488cf`, `#884f92`, fill `#975ea2`), chosen by CIEDE2000 against the status colours and the other accents in normal
  vision and under simulated protanopia, deuteranopia and tritanopia (table and reasons in the design guide §4; they
  replace S15's green and orchid and the earlier Sighter lime proposal). Token tests: both are CIEDE2000 >= 15 from every
  status colour and >= 12 from every other accent; `deltaE2000` joins the shared colour maths. App icon tiles are
  unchanged (the guide lists the closest tiles that keep the family's CVD minimum).
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
- The "no UI timeout" lint (`uiTimeoutProblems`) flags only timers that give up on work: `Promise.race` against a
  timer, `AbortSignal.timeout`, `...timeout` helpers, and a `setTimeout`/`setInterval` whose callback rejects, aborts,
  cancels, fails or throws; debounce, retry and tick timers are allowed (plan §12). The dialog shell's close check and
  refit are debounced again (30 and 60 ms), `retryUntilAnswered` schedules each attempt with `setTimeout`, and the UI
  checkpoint is a plain `setTimeout` debounce. The dialog shell's start and debounces are pending tasks, so the
  boot-every-dialog harness waits on `whenStable` (its `maxRounds` is now `maxTurns`, only for a window that never
  settles).
- `buildManifest` states a dialog's `minSize` as the contract file does (`dialogMinSize`: the kind's floor, a side left
  out filled from `DIALOG_MIN`); `DIALOG_MIN` and `VIEWER_MIN` now live in the shell core.
- The Playwright config takes its ports through `e2ePort` (CI sets `PW_PORT` and `SHELL_DEMO_PORT`).
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
- Theme and i18n in the app shell (plan S15, ADR 0012 §5-6, decision D6). Tokens: accents for Pacenotes and Sighter (values
  below), a complete high
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
- Test kit (plan S16, ADR 0012 §7). `@decryp7/paddock-e2e` (no longer a scaffold): `test` and `expect` with the
  fixtures `app` (page objects for the frame, sidebar, status bar, notifications, jobs, menus, dialogs and Settings) and
  `pageErrors`, and a worker check that refuses the installed Chrome or Edge (`assertChromeForTesting` for the config,
  `PADDOCK_E2E_CHROME` for a pinned binary, rule 7); `e2ePort` (required on CI); `axeViolations` and `THEMES`; layout
  scenarios (`LAYOUT_WIDTHS`, `expectNoHorizontalScroll`); `installTauriFake` (replies by command, held until
  released, recorded calls, events, a played Mac); `captureStable` screenshots of a settled frame; `readVersion` (the app version from its single source, rule 6); the retried-test
  reporter (`@decryp7/paddock-e2e/retried-reporter`); `dialogContracts`. `shell-demo`'s e2e uses only the kit.
  `shell/testing`: `provideShellTesting` (zoneless, shell core, `RecordingShellPorts`, `ManualIdle`, contributions,
  context, `windowReady` set), `ManualClock`, `settle` and `render` on `whenStable` (rule 38), `lazyExportProblems`;
  the static source checks (lazy registration files, lazy exports of every entry point, no UI timeout, the splash) run
  in `reference-sources.spec.ts` instead of the e2e, with S15's one-writer-of-the-look and catalog coverage checks.
- `shell/settings/window`: the Settings window, its section form and field control and `settingsWindowProviders`, split
  from `shell/settings` so a main window loads none of it (and no `@angular/forms`); a settings page without `load` is
  its Rust section in the window's generic form. The status items' components are exported as types only (their words
  from pure files), so they stay chunks of their own. `shell-demo` loads 559.5 kB of JavaScript before first paint
  in its main window instead of 676.2 kB, and 448.8 kB instead of 561.4 kB in a dialog window.
- `windowReady` (`shell/startup`): set once the window's ready reply was handled; `NATIVE_MENU_GATE` defaults to it
  (with the app context), so `shell/pages` no longer wraps `AFTER_READY` and `PAGES_WINDOW_READY` is gone.
- `DialogService.open(def, { inputs })` types the inputs from the content component's `input()` signals
  (`DialogInputsOf`); `defineDialog` keeps the component type of its `load`.
- `paddock-health`: `NoticesSnapshot.revision` (also on every `NoticesEvent`), raised by every change of any list.
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

### Fixed
- Notifications: a notice posted while a window's first `notifications_list` was in flight vanished from the centre
  (the toast showed, the older list overwrote it); the store keeps the newest revision, like the jobs and health
  stores. The shell demo's ordering workaround is gone.

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
