# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Added
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
