# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Added
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
