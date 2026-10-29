# Plan: moving Debrief onto the shared workbench

- Status: Plan, 2026-10-29; the user decided W1-W5 the same day (section 9). Nothing here is built. It refines the workbench part of
  [debrief-1-2.md](debrief-1-2.md) (packages M1, M3a, M3b, M4, M5) and [workbench.md](workbench.md) §4.3, with the state of
  the code today and the in-flight branches.
- Decision it follows: ADR [0013](../adr/0013-paddock-workbench.md) (accepted 2026-10-24; D4: Debrief adopts the shell and
  the workbench together, its shell folders frozen until then). Pacenotes and Sighter are built on the shared workbench;
  Pacenotes' own plan is [apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md](../../apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md).
- Rules read: 1, 3, 5, 18, 34, 39, 40, 42, 43, 47 ([rules.md](../rules.md)); root [AGENTS.md](../../AGENTS.md) "Shared code";
  ADRs [0011](../adr/0011-layout-by-declared-constraints.md), [0014](../adr/0014-monorepo-under-paddock.md) §10,
  [0026](../adr/0026-shared-dialog-api.md), [0027](../adr/0027-table-layers-and-sticky-headers.md); Debrief
  [ADR 0056](../../apps/debrief/docs/adr/0056-layout-by-declared-constraints.md).
- Scope: the workbench layer (frame, activity bar, Explorer, sash, dock and layout planner, narrow-window logic, pop-outs,
  palette, assistant pane) and the menu model Debrief's frame uses. The shell services (notifications, health, dialogs,
  startup, jobs, settings, About and updates) are Debrief 1.2's M2a, M2b and M2c and are named here only where they
  constrain the order.

## 0. Facts this plan starts from (checked 2026-10-29 on `origin/main` `912ad7c12`)

| Fact | Evidence |
|---|---|
| **The shared workbench is already built** (plan X50, Paddock 0.3.0): entry points `workbench`, `/frame`, `/dock`, `/explorer`, `/palette`, `/panels`, `/assistant`, `/testing`; crates `paddock-workbench`, `paddock-panels`, `paddock-assistant`; reference app `workbench-demo` | `ui/projects/paddock-ui/workbench/` (about 14,200 source and 10,100 spec lines); `ui/README.md` "Entry points"; `git log -- ui/projects/paddock-ui/workbench` (`3a41c8de9` W1 ... `b97898f40` "port of the Debrief dock sources") |
| **Debrief uses none of it.** Its only shared crate is `paddock-proc`; it imports no `@decryp7/paddock-ui` entry point (the mentions in its sources are comments) | `grep '^paddock-' apps/debrief/**/Cargo.toml`; `git grep '@decryp7/paddock-ui' origin/main -- apps/debrief` |
| Debrief's workbench folders have not changed since the shared copy was made (the D4 freeze held), so the drift is what the in-flight branches add | no commits after 2026-10-25 under `apps/debrief/app/src/app/{dock,shell,popout,layout-plan,palette,extensions,windowing,ai/chat}`; `diff` of the two `layout-plan.ts` shows one rule (`SHARES.rowHeight`) only in Debrief |
| **Dyno 1.0 and Spotter 1.0 ship on the shared shell, not the workbench.** Their branches import the root entry and `shell/*` (`dialogs`, `testing`, `settings`, `about`, `startup`, `confirm`, `notify`, `menu`, `i18n`, `pages`, `jobs`) and `format`, and depend on `paddock-shell`, `-commands`, `-dialogs`, `-download`, `-health`, `-jobs`, `-settings`, `-startup`, `-updater`, `-window`; neither is merged | `git grep '@decryp7/paddock-ui' origin/dyno/shell-1.0` and `origin/spotter/shell-1.0`; their `Cargo.toml` files |
| **Two shared parts sit under both layers**: `planLayout` in the root entry (the pages frame asks it whether the sidebar fits) and the frame parts in `shell` (`PdTitleBar`, `PdStatusBar`, `cycleRegions`, `regionKey`), which `<pd-workbench>` also uses | `shell/pages/src/sidebar-plan.ts` imports `planLayout` from `@decryp7/paddock-ui`; `workbench/frame/src/frame.ts` imports from `@decryp7/paddock-ui/shell` |
| `paddock-workbench` and `paddock-panels` sit on `paddock-commands`, `-dialogs`, `-env`, `-window`; every crate depends on `paddock-workspace-hack`; each app resolves the crates in its own `Cargo.lock` | the crates' `Cargo.toml` |
| Debrief's theme uses its own variables (`--bg-*`, `--text-*`, `--fs-*`, four themes); the shared parts read `--pd-*` | `grep -o 'var(--[a-z]*-' apps/debrief/app/src`; ADR 0027 decision 1 ("apps without the `--pd-*` tokens (Debrief, ...)") |
| The W0 screenshot baseline exists (30 shots per browser, darwin only, opt-in) and was taken before the Explorer became resizable | `apps/debrief/app/e2e/shell-baseline.e2e.ts`, `shell-baseline.md` |
| The Debrief 1.2 plan already schedules the move: M1 (wave 1), M3a dock, pop-outs, windows (wave 4), M3b activity bar, Explorer, palette, assistant (wave 5), M4 Rust (wave 6), M5 safety net (wave 9) | `docs/plans/debrief-1-2.md` §2 |

So the move is not a fresh extraction. It is three steps: bring the shared copy up to Debrief's current behaviour,
switch Debrief to it, and delete Debrief's copy with the tests that duplicated shared ones.

## 1. In-flight branches that land first

| Branch | What it changes in this area | Files outside Debrief | Effect on this plan |
|---|---|---|---|
| `fix/debrief-explorer-resize` (14 commits, last 2026-10-29) | Explorer resizable 240-640 px (default 272); the chosen-width flag `explorerWidthChosen` in Debrief's prefs and View ▸ Reset Explorer width; yield (`DockContext.explorerYield`, `explorerColumnRoom`) and room (`explorerRoom`); the open refusal decided before render (`shell/explorer-fit.ts`, pure); `NarrowWindow` changes; the Events view's minimum from `GridMetrics` and `grid/message-column.ts`; ADR 0056 amended | new entry `@decryp7/paddock-ui/sash` (`PdSash`); `workbench/frame/src/frame.ts` uses it; `ui/e2e/workbench-demo.spec.ts`; root `CHANGELOG.md`; `.gitea/workflows/debrief.yml` (CI builds the shared UI first); Debrief's `tsconfig.json` maps `@decryp7/paddock-ui/*` to `ui/dist` | lands first. Its shared UI parts are outside the frozen set (a new entry and the workbench frame), but every `ui/` change marks Spotter (and Dyno once its branch merges) in `affected.mjs`, so it costs their UI gates a CI run each (section 5, Phase 1 gates). Phase 1 ports its Explorer rules into the shared frame |
| `feat/e2e-hit-test` (10 commits, last 2026-10-29) | `expectNothingCovers` in `@decryp7/paddock-e2e`, used by every app's main view and the workbench-demo | `ui/projects/paddock-e2e/src/hit-test.ts`, `index.ts`; **`apps/dyno/app/e2e/hit-test.spec.ts`, `apps/spotter/app/e2e/hit-test.spec.ts`, `apps/pitcrew/app/e2e/hit-test.spec.ts`, `apps/pitwall/app/e2e/hit-test.e2e.ts`**; `ui/playwright.config.ts`; `scripts/affected.mjs` and its test; ADR 0027 | adds a check to the gates of Dyno, Spotter, Pitcrew and Pitwall, so Pitwall 1.1's gates change too. Decided (W1): it lands after its review, before the tags; its Dyno, Spotter and Pitwall specs must pass on `dyno/shell-1.0`, `spotter/shell-1.0` and Pitwall's 1.1 branch before it merges, and a failure is fixed in that app's release branch, not waived |
| `feat/dock-translucent-redock` (5 commits) | translucent pop-out over its window while re-docking | `crates/paddock-panels`, `paddock-workspace-hack/Cargo.toml`, `workbench/dock`, `workbench/panels`, `workbench/src/ports.ts` | its hack diff is not panels-only (it adds `NSAccessibility` and `NSWorkspace` to `objc2-app-kit`, `Win32_Graphics_Gdi` to `windows`, and drops `cairo-sys-rs`, `gio-sys`, `once_cell`, `semver`), so it changes what Dyno and Spotter build: **it lands after both tags** |
| `feat/icons-mat-registry` | icons through `MatIconRegistry`; touches `workbench/dock/src/dock-host.ts` and many `shell/*` files | yes, shell | a shell change the two releases depend on: its own timing question, not this plan's; Phase 1 rebases on it |

## 2. Inventory

"Lines" are Debrief's non-spec source lines (spec lines in brackets). "Shared" is the counterpart in `paddock-ui` or
`crates/`. "Kind": **generic** moves or is already shared; **Debrief** stays in the app as a contribution or provider;
**split** is both.

| Part | Debrief today | Shared today | Kind | Depends on (Debrief) | Tests today | Drift to bring over |
|---|---|---|---|---|---|---|
| Frame | `app.ts` (117), `shell/title-bar.ts` (119), `status-bar.ts` (146), `regions.ts` (112), `edit-commands.ts` (62) | `<pd-workbench>` (`workbench/frame`), `PdTitleBar`, `PdStatusBar`, `cycleRegions` (`shell/src/frame`) | split: frame generic; the filter bar, welcome, import toast, tour overlay and Debrief's status items are slots and `defineStatusItem` contributions | `core`, `lifecycle`, `menu`, `palette`, `projects`, `browser`, `health`, `import`, `jobs`, `problems` | Debrief `shell.spec.ts`, `regions.spec.ts`, `keyboard-regions.e2e.ts`; shared `frame.spec.ts` (530), `shell/src/frame/frame.spec.ts` | none known |
| Narrow window | `shell/narrow-window.ts` (60; changed on the branch) | `WorkbenchFrameStore` (`fallbackVerdict`, `EXPLORER_PLAN` port), `DockService.explorerVerdict` | generic | `core`, `dock` | Debrief `narrow-window.spec.ts`, `narrow-window.e2e.ts`, `home-narrow.e2e.ts`; shared `frame.spec.ts` | yield, room and the open refusal from the branch; shared minimum is 200 px, Debrief's 240 px |
| Explorer frame | the frame half of `shell/sidebar.ts` (434), `sidebar-sections.ts` (27), `explorer-names.ts` (101) | `<pd-explorer>`, `ExplorerStore` (lazy, paged, cancellable trees; middle truncation) | split: frame and tree generic; the Sources tree (sources, live tail, notes, encodings, AI, parsers) is Debrief's `defineExplorerProvider` | `ai`, `import`, `live`, `notes`, `settings`, `skew`, `sources`, `workspace` | Debrief `activity-bar.e2e.ts`, `source-files.e2e.ts`, `sidebar-parse-with.spec.ts`; shared `explorer.spec.ts` (402) | none in the tree; Debrief's names (`explorer-names.ts`) become the provider's display data |
| Sash | none on `main`; `PdSash` on the branch | `@decryp7/paddock-ui/sash` (on the branch) | generic | | branch: `sash.spec.ts` (128), `explorer-resize.e2e.ts` | lands with the branch |
| Chosen-width flag | `explorerWidthChosen` in `core/workspace/ui-prefs.store.ts` (branch) | `FramePrefs { explorer, explorerWidth }` (version 1) | generic | `core/workspace` | branch: `explorer-width.spec.ts` | the flag and Reset Explorer width move into `FramePrefs` (version 2, old prefs load as not chosen) |
| Explorer fit | `shell/explorer-fit.ts` (42, branch) | no equivalent | generic (pure) | `dock` | branch: `explorer-fit.spec.ts` | moves into `workbench/frame` |
| Events grid minimum | `grid/grid-metrics.ts`, `grid/message-column.ts` (branch) | none | **Debrief**: the Events view's declared minimum | `grid`, theme fonts | branch: `grid-metrics.spec.ts`, `message-column.spec.ts`, `grid-columns.e2e.ts` | stays; the shared part needed is "a view's minimum can change at run time and the plan re-plans", which the dock already supports through `setConstraints` per group (check in Phase 1) |
| Activity bar | `shell/activity-bar.ts` (277), `activity-overflow.ts` (19), `extensions/activity-items.ts` | `PdActivityBar`, `ActivityItems` (`workbench/frame`) | split: bar generic; Debrief's items are contributions | `extensions`, `shared` | Debrief `activity-bar.spec.ts`, `activity-bar.e2e.ts`; shared in `frame.spec.ts` | none known |
| Dock | `dock/` (63 files, 5,777 [4,850]) | `workbench/dock` (6,624 [4,899]) | split: generic, except Debrief's perspectives (`profiling`), `perspective-activity.ts`, `os-window.ts` wiring and its view contracts | `extensions`, `core`, `ui`, `notifications` | Debrief `dock*.spec.ts`, `view-contract.spec.ts`, `dock*.e2e.ts`, `dock-vs.e2e.ts`, `drag-ghost.e2e.ts`; shared `dock-*.spec.ts`, `dock-scenarios.spec.ts` | the branch's `dock-layout.ts` (+140) and `dock.service.ts` (+57) changes; file-by-file diff in Phase 1 (`dock-commands.ts`, `dock-policy.spec.ts`, `dock-ui.spec.ts`, `perspective-layouts.ts`, `provide-docking.ts` have no shared namesake) |
| Layout planner | `layout-plan/` (467 [397]) and `layout-scenarios.playwright.ts` | root `planLayout` (`src/lib/layout-plan.ts`) and the dock's `dock-scenarios.spec.ts` | generic | none | both specs | `SHARES.rowHeight` (0.45 of the dock's height for a row), Debrief only: **root entry, used by the pages frame**, so Phase 3 |
| Pop-outs | `popout/` (2,547 [1,994]); Rust `panel_windows.rs` (913), `drag_ghost/` (1,256), `ui_dispatch.rs` (101) | `workbench/panels` (2,027 [1,538]); `paddock-panels` (5,042, tests in `src`) | split: generic; `popout/shared-state.ts` (Debrief's query and selection) becomes a `defineSharedState` contribution | `core`, `dock`, `live`, `analysis`, `settings` | Debrief `popout.e2e.ts`, `shared-state.spec.ts`; shared `panel-windows.spec.ts`, `popout-shell.spec.ts` | the branch's `shared-state.ts` change (the Explorer follows the main window in pop-outs) |
| Windowing | `windowing/` (991 [822]) | the generic half in `workbench/panels` and the shell | split: titles and close prompt generic over an "unsaved" port; projects and open-ask stay | `projects` | `windowing.e2e.ts` | none known |
| Palette | `palette/` (467 [327]), the frame of `search-everything/` (1,039) | `workbench/palette` (959 [561]) | split: palette generic; events, fields and settings are Debrief palette providers | `extensions`, search engine | `palette.e2e.ts`, `search-everything.e2e.ts`, `ux-did-you-mean.e2e.ts`; shared `palette.spec.ts`, `match.spec.ts` | none known |
| Extensions and contributions | `extensions/` (1,203 [851]): tokens, view registry, command registry and context | `workbench/src` (`define*`, registry, ports), `shell/src` contributions | split: `DEBRIEF_*` tokens become `provideContributions`; `CommandContext` splits into `ShellContext` and Debrief's context; `inspector-sections` and `field-formatters` stay | everything | `commands.e2e.ts`; shared `workbench.spec.ts`, shell `define.spec.ts` | the branch's `command-registry.ts` (+15) and `tokens.ts` (+7) |
| Menu model | `shell/menu/` (1,046 lines in 7 files; `menu-model.ts` 489 imports `ANALYSIS_MENU_ORDER`, `ViewCategory`, `ThemeId`); Rust `menu.rs` (1,046) | `shell/menu` (1,435 [1,091]); `paddock-menu` | split: model and native bridge generic; Debrief's menus, recent files and theme submenu are contributions | `analysis`, `extensions`, `theme`, `dock`, `palette`, `settings`, `undo`, `windowing` | Debrief `menu-model.spec.ts`, `app-menu.spec.ts`, `menus.e2e.ts`; shared `menu-model.spec.ts`, `submenus.spec.ts` | the branch's disabled menu items and the Reset Explorer width item; **`shell/menu` is in Dyno's and Spotter's set**, so any shared gap waits for Phase 3 |
| Assistant pane | `ai/chat/` (1,937 [1,628]) | `workbench/assistant` (2,066 [1,799]); `paddock-assistant` | split: pane, loop and persistence generic; Debrief's tool renderers and its built-in llama.cpp adapter stay (workbench.md D5) | `ai` | `ai-chat.e2e.ts`, `ai-pitwall.e2e.ts` | none known |
| Jobs view | `jobs/jobs-panel.ts` as a view | `JOBS_VIEW` (`workbench/frame`) | generic | `jobs` (M2a) | `jobs.e2e.ts` | after M2a |
| Theme | `theme/` (1,291 [161]), four themes | `--pd-*` tokens, `_tokens.scss`, the token test | split: Debrief's themes become theme packs that also set the `--pd-*` tokens the shared parts read | | Debrief `theme-tokens.spec.ts`, `themes.e2e.ts` (snapshots); `npm run test:tokens` | the mapping does not exist yet |

### 2.1 Drift by date

Debrief's last change to each part against when the shared copy was made (commit dates on `origin/main`). Hashes are the last non-merge commit (`git log -1 --no-merges`). A part changed
in Debrief after its copy was made has drift to bring over; the in-flight branch adds to that.

| Part | Debrief's last change | Shared copy made | Drift |
|---|---|---|---|
| Layout planner | `86d72af47`, 2026-10-24 19:29 ("short-window row share": `SHARES.rowHeight`) | `4a0760b9b`, 2026-10-24 13:26 | **yes**: the row share (Phase 3a) |
| Dock | `86d72af47`, 2026-10-24 19:29 (last non-merge commit) | `b97898f40`, 2026-10-25 03:27 (port of the dock sources) | none expected from `main`; the branch adds `dock-layout.ts` and `dock.service.ts` changes; 1a verifies file by file |
| Frame, activity bar, narrow window | `bbfd4ef22`, 2026-10-24 16:30 | `448b5e627`, 2026-10-25 03:32 | none expected from `main`; the branch adds the Explorer rules (1b) |
| Explorer | (in `shell/`) 2026-10-24 16:30 | `504b8bb25`, 2026-10-24 11:30 (scaffold), built through 2026-10-28 | to check in 1a: the copy's start predates Debrief's last change |
| Pop-outs | `1474dca8d`, 2026-10-24 10:45 | `504b8bb25`, 2026-10-24 11:30 | none expected; the branch's `shared-state.ts` change is Debrief's own |
| Palette | `6ca4b2cd0`, 2026-10-23 20:32 | 2026-10-24 and later | none |
| Extensions and contributions | `84b22df4e`, 2026-10-24 | 2026-10-24 and later | the branch's `command-registry.ts` and `tokens.ts` |
| Windowing | `7b2cd1c0d`, 2026-10-24 | 2026-10-24 and later | none expected |
| Assistant pane | `f04fde46f`, 2026-10-24 | 2026-10-24, built through 2026-10-25 | none expected |
| Menu model | `322483998`, 2026-10-24 11:07 | `504b8bb25`, 2026-10-24 11:30 | to check in 1a; the branch adds disabled items and Reset Explorer width |

## 3. API surface, tokens and CHANGELOG

- **`public-api.ts` additions** (in the entry points that own them; the root `public-api.ts` is not touched until Phase 3):
  - `workbench/frame`: `FramePrefs.explorerWidthChosen`, `WORKBENCH_FRAME_VERSION` 2 with the version-1 load path,
    `resetExplorerWidth()` on `WorkbenchFrameStore`, `explorerFits` and `explorerRoom` (the pure parts of
    `explorer-fit.ts`), a `FRAME_COMMANDS` entry for Reset Explorer width, the open-refusal notice id.
  - `workbench`: `DockPort.explorerYield` (what the Explorer can give up), and **one** `EXPLORER_LAYOUT`. Today there
    are two: `workbench/frame/src/frame.store.ts:7` (min 200 x 200, pref 272) and `workbench/dock/src/dock-layout.ts:39`
    (min and pref 300, min height 240). Package 1b moves the declaration to the workbench core (`workbench/src`), with
    min 240 (decided W3), pref 272 and max 640, and both the frame and the dock import it; the exported name stays
    `EXPLORER_LAYOUT` in `workbench/frame`'s `public-api.ts` as a re-export.
  - `sash`: as on the branch.
  - Root entry (Phase 3 only): the row share rule of `planLayout` (`SHARES.rowHeight`) as an option of the plan's input,
    off by default, so the pages frame's result is unchanged.
- **CSS tokens**: no new `--pd-*` token is planned. Debrief's theme packs map their colours, sizes and fonts onto the
  existing `--pd-*` tokens in Debrief's own files; the token test (WCAG pairs and CVD separation) runs over Debrief's four
  packs as fixtures, in `ui/` (rule 43). If a Debrief theme needs a value no token carries, that is a token change in
  `_tokens.scss` with the design guide and the token test in the same change.
- **CHANGELOG**: every phase adds an Unreleased entry to the root [CHANGELOG.md](../../CHANGELOG.md) for each changed entry
  point, and Debrief's `apps/debrief/CHANGELOG.md` gets a user-facing line only where behaviour changes (none is planned
  beyond the Explorer branch; any pixel difference in M5 is either fixed or written there with its reason).
- No compatibility shims (before v1, AGENTS.md "Shared code"): a renamed export is renamed in every consumer in the same
  change.

## 4. What Dyno 1.0 and Spotter 1.0 depend on (the frozen set)

Generated on 2026-10-29 from `origin/dyno/shell-1.0` (`25c2f5f02`) and `origin/spotter/shell-1.0` (`e2f2811db`), the
same for both:

- **Rust:** every `paddock-*` package in `apps/dyno/Cargo.lock` and `apps/spotter/Cargo.lock`: `paddock-channels`,
  `-cli`, `-commands`, `-diag`, `-dialogs`, `-download`, `-env`, `-files`, `-health`, `-jobs`, `-log`, `-mcp`, `-menu`,
  `-net`, `-packs`, `-pitwall`, `-proc`, `-secrets`, `-service`, `-settings`, `-shell`, `-siblings`, `-startup`,
  `-storage`, `-updater`, `-watch`, `-window` and `paddock-workspace-hack`. The coupling is each app's own `Cargo.lock`
  and the hack, not `crates/Cargo.lock`. (`paddock-proc` also changes in Debrief 1.2, package 2.2: that package waits
  for the tags too.)
- **UI entry points:** the apps import the root entry, `format`, `shell`, `shell/about`, `shell/confirm`,
  `shell/confirm/dialog`, `shell/dialogs`, `shell/i18n`, `shell/jobs`, `shell/menu`, `shell/notify`, `shell/pages`,
  `shell/settings`, `shell/settings/window`, `shell/startup` and `shell/testing`. The closure of their
  `@decryp7/paddock-ui/*` imports (static and `import()`, on each branch's own `ui/`) adds `icons`, `select`,
  `shell/feel`, `shell/store` and `testing`. Only this closure is frozen. The other shell entries (`shell/service`,
  `shell/pitwall`, `shell/mcp`, `shell/storage`, `shell/channels`, `shell/packs`, `shell/setup`, `shell/files`,
  `shell/diagnostics`, `shell/undo` and their pages) stay open, which Pitwall 1.1 needs for `shell/service` and
  `shell/pitwall`; `ipc`, `states` and `table` are reached only through those and stay open too.
- **Paths of the closure:** `ui/projects/paddock-ui/{src,format,icons,select,testing}/**` and the frozen `shell` entry
  folders, each without its nested entries.
- **Declared extra paths, each with its reason** (a separate list in the declaration, tested on its own):
  `ui/projects/paddock-ui/scss/**` (compiled into the `paddock.css` both apps load from `angular.json`);
  `ui/projects/paddock-ui/i18n/**` (merged into the apps' catalogues, C32); `ui/projects/paddock-ui/bin/**` (the
  `paddock-i18n` build the apps run); the package's `package.json` and `ng-package.json` (they build every entry);
  `ui/projects/paddock-e2e/src/**` (both apps' e2e import `@decryp7/paddock-e2e`); the root `package.json` and
  `package-lock.json` entries the apps resolve.

Not frozen (no release app reaches them): `chart`, `forms`, `ipc/testing`, `live`, `material`, `prefs`, `schedule`,
`view`, `sash` (new), every `workbench/**` entry, `ui/projects/workbench-demo/**`, `apps/workbench-demo/**`,
`ui/e2e/workbench-demo*.spec.ts`, the shell entries named above, `ipc`, `states`, `table`, `crates/paddock-workbench`, `crates/paddock-panels`, `crates/paddock-assistant`
(only while `paddock-workspace-hack` stays byte-identical), and `apps/debrief/**`. Package 0a turns this list into a
generated, checked one; until it lands, each package's brief names its allowed paths and its reviewer checks
`git diff --name-only origin/main` against them.

Debrief 1.2 is itself an app release: the phases below are its packages, and rule 1 applies to it as well (Dyno and
Spotter first, then Pitwall 1.1, then Debrief 1.2, by the order of 2026-10-28).

## 5. Phases

Each phase is one or more changes that land with every app green: the shared change and its adoption together (rule 42,
ADR 0014 §10). A phase that touches the frozen set waits for both tags.

### Phase 0: the branches above land (now)

1. `fix/debrief-explorer-resize`: merge after its review; it is the first change where Debrief imports a shared entry
   point (`sash`) and maps `@decryp7/paddock-ui/*` (Debrief 1.2 package 0.4 in part).
2. Retake the W0 baseline on that merge (`npm run e2e:shell-baseline -- --update-snapshots`, every PNG reviewed, rule 22),
   because the Explorer's width and the Events grid's columns change the pixels. M5 compares against this baseline.
3. `feat/e2e-hit-test`: after its review, before the tags (W1), with the two app specs green on the release branches.
   **It lands before package 0a**: once 0a is active, `paddock-e2e/src` is frozen and the branch could not merge.
4. `feat/dock-translucent-redock`: after both tags (its hack diff is not panels-only, section 1).
5. Package 0a, the release-freeze check (W5), lands first of this plan's own packages, so every later phase is gated by it:

| # | Package | Owned files | Failing test first | Days |
|---|---|---|---|---|
| 0a | **Release-freeze check** (conformance; rules 1 and 40). See below | `conformance/src/checks/` (new check), `conformance/bin/release-freeze.mjs`, `conformance/test/`, `conformance/fixtures/`, `conformance/README.md` row; `paddock.conformance.json` entry and schema, the integrate procedure step and the `lefthook.yml` pre-push entry (integrating agent; the user installs lefthook) | see below | 2.5 |

Package 0a in detail. It depends on `feat/e2e-hit-test` having landed (item 3), and builds on `fix/affected-narrow-global`.

- **Declaration.** One joint freeze for the releases in flight: `releaseFreeze: { apps: ["dyno", "spotter"], branches:
  ["dyno/shell-1.0", "spotter/shell-1.0"], until: ["dyno-v1.0.0", "spotter-v1.0.0"], extra: [{ path, reason }] }`. It
  ends when every tag exists.
- **Frozen paths, derived.** UI: an entry-point graph, one node per `ng-package.json` entry of `paddock-ui`, edges from
  each entry's `@decryp7/paddock-ui/*` imports (static and `import()`), starting from the apps' own imports; only the
  closure's folders are frozen (section 4), plus the declared `extra` list. `affected.mjs` today treats all of `ui/` as
  one package (`scripts/affected.mjs:368`), so the graph is a new module that `affected.mjs` can use later; the narrowing
  of its global triggers is being done on `fix/affected-narrow-global` by another agent, and 0a does not change them.
  Rust: `crates/<name>/` for every `paddock-*` package in a frozen app's own `Cargo.lock`, read from the declared
  release branch's tip (`git show <branch>:apps/<app>/Cargo.lock`), not from `main`, plus `crates/paddock-workspace-hack/`.
- **Lockfiles.** A small lock-diff parser reads a frozen app's `Cargo.lock` at the base and at the head (TOML
  `[[package]]` entries: name, version, source, checksum, dependencies) and fails on any added, removed or changed
  entry, with the entry named; the same for that app's `package-lock.json` entries under its workspace.
- **Where it blocks: locally, before the push.** Branch CI here is `workflow_dispatch` and work lands on `main` through
  local merges that are then pushed, so a CI check can only report afterwards. The check is a CLI,
  `node conformance/bin/release-freeze.mjs --range origin/main..HEAD` (two dots), run by the integrating agent as a step of the
  integrate procedure before every push to `main`; that step is the real gate. The CLI derives the source itself and
  takes no `--source` to trust. It walks the range with `git rev-list origin/main..HEAD` and skips every commit that is
  an ancestor of a declared release branch's tip (the release branch's own commits, which the range includes when that
  branch is merged); a merge commit is exempt when its second parent (`<commit>^2`) is such an ancestor; any other
  commit touching a frozen path fails. It exits non-zero
  and lists the commits and frozen paths. A `lefthook.yml` pre-push entry runs the same CLI when a pushed remote ref
  read from stdin is `refs/heads/main` (not when the current branch is `main`). Agents never install lefthook; only
  the user runs `npx lefthook install`.
- **In CI, as a second line.** On `workflow_dispatch` and `push`, the source is `github.ref_name` and the diff is taken
  against `git merge-base origin/main HEAD` (never the plan job's `PLAN_BASE`). On a push to `main`, each new commit is
  checked: a merge commit is exempt when its second parent is an ancestor of a declared release branch's tip (and then
  both apps' gates are added to the plan); any other commit touching a frozen path fails the run. A dispatch on a
  declared release branch is exempt and adds both apps' gates.
- **Failing tests first.**
  - CLI: a range whose non-merge commit touches `ui/projects/paddock-ui/shell/pages/` exits non-zero and names the commit
    and path; a range that merges `dyno/shell-1.0` passes and reports both apps' gates, and its fixture includes the
    release branch's own commits touching frozen paths (they are skipped as ancestors of the tip, not counted against
    the merge); the same fixture with one extra non-merge commit on top touching `shell/menu/` fails on that commit
    only; after both tags exist, all pass.
  - Hook: stdin naming `refs/heads/main` runs the CLI; stdin naming only `refs/heads/feat/x` does not.
  - Lockfiles: the frozen set comes from the branch tips; a fixture where `main`'s `apps/dyno/Cargo.lock` differs from
    the tip's uses the tip's.
  - Dispatch: `ref_name` `feat/x` touching `shell/pages/` fails; `ref_name` `spotter/shell-1.0` passes; the diff base is
    the merge base, so a `main` that moved on does not count its own commits.
  - Push to `main`: a merge commit whose second parent is in `dyno/shell-1.0` passes; a merge commit whose second parent
    is not fails; a non-merge commit touching `shell/menu/` fails.
  - Paths: `workbench/frame/` passes; `shell/service/` and `table/` pass (outside the closure); `crates/paddock-proc/` and
    `crates/paddock-workspace-hack/` fail; a changed entry in `apps/spotter/Cargo.lock` from `feat/x` fails with the
    entry named.
  - The `extra` list is tested on its own: each declared path fails a fixture change and each carries a reason; a path in
    `extra` without a reason is refused.
  - The derived closure for the two current branches equals section 4's list.

### Phase 1: the shared copy catches up (workbench-only paths; may land before the tags)

| # | Package | Owned files | Failing test first | Days |
|---|---|---|---|---|
| 1a | Dock diff: compare Debrief's `dock/` with `workbench/dock` file by file (the branch's `dock-layout.ts` and `dock.service.ts` changes, the five Debrief-only files), port every generic behaviour with its spec, list what stays Debrief's | `workbench/dock/**`, a diff note in the package's PR | each ported behaviour's Debrief spec fails against the shared dock before the port, ported as a shared spec | 2 |
| 1b | Explorer rules in the frame: one `EXPLORER_LAYOUT` in `workbench/src` used by the frame and the dock (section 3), range 240-640, default 272, chosen flag (`FramePrefs` v2 with v1 migration), Reset Explorer width, yield and room through the dock port, open refusal before render with its notice, `explorer-fit` pure functions | `workbench/frame/**`, `workbench/src/ports.ts`, `workbench/src/kinds.ts`, `workbench/src/layout.ts` (new), the `EXPLORER_LAYOUT` line of `workbench/dock/src/dock-layout.ts` | a spec that the frame and the dock see the same minimum (240) fails today (200 against 300); the branch's `explorer-fit.spec.ts` and `explorer-width.spec.ts` cases, rewritten against `WorkbenchFrameStore`; v1 prefs load as not chosen | 2 |
| 1c | `workbench-demo` shows it: a view whose minimum changes at run time (the "Events minimum" case without Debrief's grid), the refusal notice, Reset | `ui/projects/workbench-demo/**`, `ui/e2e/workbench-demo*.spec.ts` | the demo's narrow-window e2e from Debrief's `narrow-window.e2e.ts` and `explorer-resize.e2e.ts` scenarios, ported to the demo | 1.5 |

Gates: `cd ui && npm run lint && npm test && npm run build && npx playwright test --project=workbench-demo` (Chromium),
the dock scenario table, `test:tokens` untouched, and package 0a green. `affected.mjs` marks more than the demo: a
`workbench/frame` change marks `shell-demo`, `workbench-demo` and Spotter (its tsconfig maps `ui/dist`), and Dyno once
its branch merges, because `ui/` is one package to it today. So each Phase 1 change costs one run of Spotter's UI gates
(and Dyno's) during their release; batch Phase 1 into as few pushes as possible, and expect fewer marks once
`fix/affected-narrow-global` lands.

### Phase 2: Debrief's contributions, no shared change (Debrief-only; may land before the tags)

| # | Package | Owned files | Failing test first | Days |
|---|---|---|---|---|
| 2a | M1, workbench half: `DEBRIEF_*` view, activity and palette tokens become `defineView`, `defineActivityItem`, `definePaletteProvider`, `definePerspective`, `defineSharedState` contributions in Debrief's own registration files, still served by Debrief's copy through an adapter; registrations lazy by construction (workbench.md §9) | `apps/debrief/app/src/app/extensions/**`, `builtin/**`, Debrief registration files | the shared contributions lint run over Debrief's manifest (every view declares layout, no `min > pref`); the main window's first-paint JS measured by `scripts/window-weight.mjs` (C29) does not grow | 3 |
| 2b | Theme packs: Debrief's four themes also set the `--pd-*` tokens the shared parts read; the token test over the four packs | `apps/debrief/app/src/app/theme/**`, a fixture in `ui/test/tokens/` (new file only) | `test:tokens` fails on a planted low-contrast pair in a Debrief pack | 2 |
| 2c | The Sources tree as an `ExplorerProvider` behind Debrief's current sidebar (adapter), so the provider is tested before the frame swaps | `apps/debrief/app/src/app/sources/explorer-provider.ts` and its spec | the provider pages and cancels (shared contract test from `explorer.spec.ts`'s fakes) | 1.5 |

Gates: Debrief's lint, unit tests and the touched e2e specs in Chromium; W0 baseline unchanged (these steps change no
pixels).

### Phase 3: shared changes in the frozen set (after both tags)

Only what Debrief's adoption needs and the shared copy lacks; each lands with its adoption by the pages frame apps in the
same change and their e2e green.

| # | Package | Owned files | Failing test first | Days |
|---|---|---|---|---|
| 3a | `planLayout` row share as an input option (off by default); the dock passes it | `ui/projects/paddock-ui/src/lib/layout-plan.ts` and spec, `workbench/dock/src/dock-layout.ts` | Debrief's short-window case (1024x768, wrapped filter bar) fails in the shared planner spec; the pages-frame spec's results are byte-identical | 1 |
| 3b | Menu gaps found in 1a/2a (for example disabled items with a reason, the Reset item's group) | `ui/projects/paddock-ui/shell/menu/**`, `crates/paddock-menu` if the native menu needs it | the Debrief `menu-model.spec.ts` case that the shared model cannot express | 1 |
| 3c | Frame slots Debrief needs (filter bar above the stage, welcome, import toast, tour overlay) if `<pd-workbench>`'s slots do not cover them | `workbench/frame/**`; `shell/src/frame/**` only if the title bar needs a slot | a slot test in `frame.spec.ts` | 1 |

Gates: `ui` lint, unit, build, the gallery, shell-demo and workbench-demo e2e in Chromium, plus Dyno's, Spotter's and
Pitwall's affected e2e (they are on the shell by then) and `affected.mjs`'s list.

### Phase 4: Debrief switches (Debrief 1.2 M3a and M3b)

| # | Package | Owned files | Failing test first | Days |
|---|---|---|---|---|
| 4a | M3a: `<pd-dock>` and `WorkbenchDock` replace Debrief's dock; perspectives (`profiling`) as contributions; pop-outs through `workbench/panels` with Debrief's shared state; windowing on the unsaved port; delete `dock/`, `layout-plan/`, `popout/` (generic half), `windowing/` (generic half) | `apps/debrief/app/src/app/{dock,layout-plan,popout,windowing}/**`, `app.config.ts` lines, popout and dialog window bootstraps | `dock*`, `popout`, `perspectives`, `profiling-perspective`, `drag-ghost`, `windowing`, `narrow-window`, `explorer-resize` e2e green; the scenario table green; W0 dock shots within tolerance or explained | 2.5 |
| 4b | M3b: `<pd-workbench>` frame, activity bar, Explorer with the Sources provider, sash and chosen width through the shared frame (Debrief's prefs key migrated once), palette with Debrief's providers, assistant pane with Debrief's renderers and its built-in engine adapter; the deletions listed below | the deleted files and their specs | `activity-bar`, `palette`, `keyboard-regions`, `ai-chat`, `ai-entry-points`, `home-narrow`, `search-everything`, `explorer-resize` e2e green; Sources with 10,000 files renders only what is in view; W0 shots within tolerance or explained | 3 |
| 4c | Menu model: Debrief's menus as contributions on `shell/menu`; delete `shell/menu/` (if Debrief 1.2 M2c has not already) | `apps/debrief/app/src/app/shell/menu/**` | `menus.e2e.ts`, `app-menu.spec.ts` (rewritten as wiring) | 1.5 |

4b deletes `shell/{activity-bar,activity-overflow,sidebar*,explorer-names,explorer-fit,regions,narrow-window,title-bar,status-bar}.ts`,
`palette/`, the frame of `search-everything/` and the frame of `ai/chat/`.

### Phase 5: Rust side and deletion (Debrief 1.2 M4)

`paddock-workbench` registered in Debrief's `src-tauri`; Debrief's table entries for moved commands deleted; `debrief://`
panel events renamed to `paddock://panels`; capabilities grant the workbench sets; delete `panel_windows.rs`,
`drag_ghost/`, `ui_dispatch.rs`. Failing test first: a `MockRuntime` test that registers every command and fails while
Debrief's own panel commands remain. Gates: `cargo test` for Debrief's app crate, `prod-csp`, `app-load`, `native-feel`,
`popout` and `drag-ghost` e2e. About 1.5 days.

### Phase 6: safety net and test moves (Debrief 1.2 M5)

- W0 baseline (retaken in Phase 0) before and after, both engines, on the Mac it was taken on; every difference fixed or
  explained in Debrief's CHANGELOG.
- The full Debrief suite, Chromium and WebKit, once on the combined result (rule 3); the real-app smoke on three OS.
- **Test moves (rule 43).** Shared behaviour is tested once, in `ui/` and `crates/`: Debrief deletes its specs that test
  dock, planner, pop-out, palette, Explorer-frame and assistant-pane behaviour (`dock/*.spec.ts`, `layout-plan.spec.ts`,
  `popout/*.spec.ts` except `shared-state`, `palette/*.spec.ts`, `shell/{activity-bar,regions,narrow-window}.spec.ts`,
  and the Rust tests of the deleted files) after checking each case exists in the shared spec (ported in Phase 1 if
  missing). Debrief keeps its wiring and domain tests: view contracts, the Sources provider, palette providers, shared
  state, perspectives, the Events minimum (`GridMetrics`, `message-column`), and the real-app e2e flows. Record the test
  count and run time before and after in `apps/debrief/docs/notes/1-2-baseline.md`.
- About 2 days (inside M5's 2.5).

## 6. Effort and order

| Phase | When | Agent-days | Debrief 1.2 package |
|---|---|---|---|
| 0 | now | baseline retake 0.5; package 0a 2.5 | new |
| 1 | before the tags (workbench-only) | 5.5 | new |
| 2 | before the tags (Debrief-only) | 6.5 | 2a is the workbench half of M1 (3); 2b and 2c are new (3.5) |
| 3 | after both tags | 3 | new |
| 4 | Debrief 1.2 waves 4-5 | 7 | M3a (2.5), M3b (2.5, here 3), 4c is part of M2c (1.5) |
| 5 | Debrief 1.2 wave 6 | 1.5 | M4 (2; the shell half stays there) |
| 6 | Debrief 1.2 wave 9 | 2 | inside M5 (2.5) |

About 28.5 agent-days in all (±50 %), of which about 14 can be done before the tags (W2). Against Debrief 1.2's plan:
its M1-M5 are about 21 days (M1 3, M2a 2, M2b 3.5, M2c 3, M3a 2.5, M3b 2.5, M4 2, M5 2.5), of which the workbench part
(M1's half, M3a, M3b, the menu part of M2c, M4, M5) covers about 13.5 of this plan's days. The rest, about 15 days
(0a, the baseline retake, Phase 1, 2b, 2c, Phase 3), is new, so **Debrief 1.2's total grows by about 15 agent-days**,
about half of it Paddock-side work done before the tags. Phases 1 and 2 run in parallel (disjoint files:
`ui/projects/paddock-ui/workbench/**` against `apps/debrief/**`); 1a, 1b and 2a, 2b, 2c are five agents at most, so the
coordinator runs them within the load limit and behind the release work (rule 1).

## 7. Conformance checks to add (numbers allocated at merge, C31)

1. **Release freeze** (package 0a, decided W5). One joint freeze for the releases in flight; frozen paths derived from
   the apps' own `Cargo.lock` files, `paddock-workspace-hack` and the `paddock-ui` entry-point closure, plus a declared
   list with reasons; blocks locally before the push to `main` (`release-freeze --range origin/main..HEAD`, which derives the source from
   merge ancestry), and reports in CI on dispatch and push, where a merge of a declared release branch is exempt and runs
   both apps' gates. Makes rule 1's "don't change
   shared code the release depends on" checked instead of remembered.
2. **One dock, one planner.** `dockview` is imported only under `ui/projects/paddock-ui/workbench/dock/`, and no file
   outside `ui/projects/paddock-ui/src/lib/` defines `planLayout` or a layout-plan module; an app's copy fails (Debrief's
   copy is waived until Phase 4 with a dated waiver, ADR 0021).
3. **Moved folders stay deleted.** A `moved` list in `paddock.conformance.json` (`apps/debrief/app/src/app/{dock,layout-plan,popout,palette}`
   and the deleted `shell/*` and Rust files) must not reappear.
4. **Hand-sized panes use `PdSash`.** An element with `role="separator"` and pointer or key handlers in an app's template
   outside `paddock-ui/sash` fails (rule 47's "hand-roll only what neither provides").
5. **Workbench apps run the layout scenario table and the hit test.** Every app whose UI imports
   `@decryp7/paddock-ui/workbench/dock` has an e2e that calls the scenario runner and `expectNothingCovers` on its main
   window (like C33 for axe).
6. **Contributions lint over every workbench app's manifest**: every view declares its layout, `min <= pref`, every
   activity item and palette provider resolves (the shared lint, run in each app's gate, not only in the demo).

## 8. Risks to Dyno 1.0 and Spotter 1.0, and other risks

| Risk | Mitigation |
|---|---|
| A workbench change reaches the pages frame through the root entry (`planLayout`) or the shell frame parts | those paths are frozen (section 4); Phase 3 changes them only after both tags, with the pages-frame results byte-identical |
| A workbench crate change regenerates `paddock-workspace-hack`, which Dyno's and Spotter's builds read through their own `Cargo.lock` | Phase 1 adds no Rust dependency; a package that would must wait for the tags; the reviewer checks the hack is unchanged |
| `feat/e2e-hit-test` adds new checks to Dyno's and Spotter's gates days before their tags | lands after review (W1) only with its two app specs green on the release branches; a failure is fixed there, not waived |
| The shared copy misses behaviour Debrief learned since 2026-10-25 | Phase 1a's file-by-file diff, with each Debrief spec run against the shared dock first |
| The W0 baseline is stale after the Explorer change, so M5 would report false differences | retake it in Phase 0 |
| Debrief's themes do not carry `--pd-*` tokens, so shared parts render with defaults | Phase 2b before Phase 4; the token test over Debrief's packs |
| Debrief's first-paint JavaScript grows when the workbench entry points are added | lazy contributions (workbench.md §9); C29 budget in Phase 2a and 4 |
| Two agents edit `workbench/dock` at once (Phase 1a and `feat/dock-translucent-redock`) | 1a starts after that branch lands or rebases on it; one owner for `workbench/dock/**` at a time |
| The move slips and delays Pacenotes | Pacenotes' stage 1 is pure Rust and needs no workbench; its UI starts after Debrief 1.2 (decided 2026-10-29), so a slip in Phases 4-6 delays Debrief 1.2 and Pacenotes' stage 2 alike, and is visible in Debrief 1.2's plan |

## 9. Decisions (user, 2026-10-29)

| # | Question | Decided |
|---|---|---|
| W1 | When `feat/e2e-hit-test` lands | after its review, **before** the Dyno and Spotter tags (Phase 0, item 3) |
| W2 | Phases 1 and 2 before the tags | **yes**, within load: they touch only workbench-only and Debrief-only paths, gated by package 0a |
| W3 | The shared Explorer minimum | **240 px** for every workbench app (Phase 1b) |
| W4 | Timing of Phases 4-6 | **inside Debrief 1.2**, as its plan schedules M3a, M3b, M4 and M5 |
| W5 | The release-freeze check | **add it now**, as its own work package (package 0a) |

Related decision for Pacenotes (same day): its UI and its shared-code changes wait until Debrief 1.2 has shipped, so
Pacenotes' first screens are built on the workbench after this move's Phases 4-6; see its
[plan](../../apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md) §4.1.
