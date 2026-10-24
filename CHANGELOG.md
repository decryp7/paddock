# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Added
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
