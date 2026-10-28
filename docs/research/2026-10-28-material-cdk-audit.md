# Angular Material and CDK audit: what we hand-roll and what to move (2026-10-28)

**Question.** Which hand-rolled UI components and behaviours in `ui/projects/paddock-ui` and the apps can use Angular
Material or the CDK instead? Is `pd-table` → `mat-table`/`CdkTable` with `CdkVirtualScroll` sound?

**Policy (the user, 2026-10-28):** "use Material components as much as possible since they are tested", then "move
components that are hand built to them". So the default verdict here is **replace**. Material comes first; the CDK is
used where Material has no component. **Keep** needs a concrete reason Material cannot meet: a missing capability, the
CSP, accessibility, or measured performance. AGENTS.md already says the same in "UI building blocks" (CDK for behaviour
and accessibility, Material for complex inputs, themed through `--pd-*`).

**Out of scope, already decided:**
- Dialogs are OS windows with the CDK focus trap (ADR 0026, `feat/dialog-api`).
- `pd-table` moves to `mat-table` + `MatSort` + `CdkVirtualScroll` (`fix/grid-header-on-top`).

## 1. What is in the tree (verified on `origin/main` c70adad77)

| Fact | Evidence |
|---|---|
| Angular 22.2.3, Material 22.2.3, CDK 22.2.4 | `package-lock.json` (`node_modules/@angular/{core,material,cdk}`); `ui/package.json` `^22.2.x` |
| `@angular/aria` not installed (npm latest 22.2.4, published 2026-10-21) | lockfile; `registry.npmjs.org/@angular/aria` |
| paddock-ui uses the CDK only: menu (shell menus), overlay, listbox (`pd-select`), scrolling (`pd-table`) | `shell/menu/src/menu-view.ts`, `select/src/select.ts`, `table/src/table.ts`, `workbench/{explorer,frame}` |
| paddock-ui does **not** list `@angular/material` as a peer | `ui/projects/paddock-ui/package.json` peerDependencies |
| A Material bridge exists but **no app loads it**: `scss/_material.scss` maps `--mat-sys-*` and the tooltip, slide toggle, checkbox, radio, form field, select, autocomplete, option, menu, tabs, datepicker, progress, chip, snack bar and dialog tokens to `--pd-*`, and builds `css/material.css` (plan X37) | `ui/projects/paddock-ui/scss/_material.scss`; no `material.css` reference in any `angular.json` (only `ui/test/tokens/controls.spec.ts` compiles it) |
| Debrief, Pitwall and Pitcrew each `@include mat.theme(...)` with their own overrides. Dyno has no Material theme (only the prebuilt CDK overlay and a11y css), though it uses `matTooltip`. Spotter has none. | `apps/{debrief,pitwall,pitcrew}/app/src/styles.scss`; `apps/dyno/app/angular.json` |
| On main only Spotter imports `@decryp7/paddock-ui` (empty state, skeleton, segmented control, status pill, tray popover, chart theme). The other apps still use their own copies. | `grep "from '@decryp7/paddock-ui'" apps/*/app/src` |
| Material already ships in released apps: Pitwall (76 `matTooltip`, 18 `mat-slide-toggle`), Pitcrew (15 `mat-slide-toggle`), `mat-icon` everywhere | grep counts |
| CSP in every app: `style-src 'self' 'unsafe-inline'`, no `unsafe-eval` | `apps/*/app/src-tauri/tauri.conf.json` |

**CSP.** Material positions overlays and sizes things by setting `element.style` through the CSSOM. CSP's `style-src`
restricts `style=""` attributes in markup and `<style>` elements, not CSSOM writes. The family rule ("bind styles with
Angular style bindings only, no inline style attributes") concerns our templates, and Material templates do not emit
`style="..."` markup. The evidence is practical: `matTooltip`, `mat-slide-toggle` and CDK overlays already run in
released Pitwall and Pitcrew under this CSP. Reading: no blocker. Verified only by these shipped apps, not by a CSP
test of every new component, so each package's e2e run must show no CSP console errors.

## 2. Inventory and verdicts

Effort: S under a day, M one to three days, L more. Risk is the chance of a visible or behavioural regression.

### 2.1 Shared library (`ui/projects/paddock-ui`)

| Ours (files) | Material / CDK candidate | Verdict and reason | Effort | Risk |
|---|---|---|---|---|
| `pd-table` (`table/`, 1,014 lines): sticky header, sort, filter, column menu, group rows, virtual scroll, roving grid keys | `mat-table` + `MatSort` + `CdkVirtualScrollViewport` | **Replace (decided, `fix/grid-header-on-top`).** See §3 for the sticky-header caveat. Grouping and the roving cell grid have no Material equivalent and stay ours on top of `mat-table` (group rows through `when` predicates on `matRowDef`). | L | M |
| `pd-select` (`select/`, CDK listbox + overlay) | `mat-select` | **Replace.** Same ARIA combobox/listbox pattern, typeahead, flip, form integration, and Material maintains and tests it. The bridge already themes `mat-select` and options. Keep the `pd-select` API as a thin wrapper so consumers don't change. | S | L |
| `pd-field` (`forms/`): label above, hint, error, `pdFieldControl` ids | `mat-form-field` + `matInput` + `mat-label`/`mat-hint`/`mat-error` | **Replace internals, keep the API.** `mat-form-field` owns the ids, `aria-describedby` and error state. **D1:** `appearance="outline"` with `floatLabel="always"`; design rule 11 in `docs/design-guide.md` changes to match. | M | M |
| `pd-segmented-control` (`src/lib/segmented-control.ts`): 2 to 4 options, radio-group keys | `mat-button-toggle-group` (single) | **Replace.** Material's toggles are buttons with `aria-pressed` in `role="group"` ([docs](https://github.com/angular/components/blob/main/src/material/button-toggle/button-toggle.md)). That is a different but valid pattern from our radio group, and arrow keys move between toggles in recent versions (unverified, the docs page doesn't say: check in the e2e). Keep the `SuSegmentedControl` API. Consumers: Spotter, `shell/notify/notifications-panel`, `view/review`. | S | L |
| Schedule editors (`schedule/`): `pd-day-picker`, and native `type="time"` in schedule and pause editors | multiple `mat-button-toggle-group`; `mat-timepicker` | **Replace.** `mat-timepicker` (combobox, `interval`, min and max) needs a `DateAdapter`; `provideNativeDateAdapter` is enough for 24-hour or AM/PM ([docs](https://github.com/angular/components/blob/main/src/material/timepicker/timepicker.md)). The value is a `Date`, so the editor converts to and from the engine's `HH:MM` at its edge. Native `type="time"` is drawn by the OS and ignores the theme, so the visual gain is real. | M | M |
| Settings field and window (`shell/settings/window/`): `.pd-switch` buttons with `role="switch"`, checkboxes, `pd-badge` | `mat-slide-toggle`, `mat-checkbox`, static `mat-chip` | **Replace.** Pitwall and Pitcrew already run `mat-slide-toggle`, and the bridge scales it for compact density. | M | L |
| Shell pages: mcp, channels, setup, storage, packs, service, diagnostics, i18n (`shell/*/page`, `shell/diagnostics`): checkboxes, switches, `<progress>`, a radio group for language | `mat-checkbox`, `mat-slide-toggle`, `mat-progress-bar`, `mat-radio-group` | **Replace.** Native `<progress>` ignores the tokens; `mat-progress-bar` covers determinate, indeterminate and buffer modes. | M | L |
| `pd-job` (`src/lib/job.ts`): hand-made `role="progressbar"` | `mat-progress-bar` | **Replace the bar only.** Phases, ETA, Cancel and the live region are ADR 0008 domain and stay. | S | L |
| Jobs status popover (`shell/jobs/src/jobs-status.ts`): an absolutely positioned `div`, Escape and outside-click handled by hand | `cdkConnectedOverlay` (CDK Overlay) | **Replace.** Gains flip at window edges, outside-click handling and the overlay z-order. | S | L |
| Pages sidebar (`shell/pages/src/sidebar.ts`): links with hand-rolled arrow, Home and End keys, rail tooltips via `title` | `mat-nav-list` + CDK `FocusKeyManager`; `matTooltip` | **Replace the behaviour.** `mat-nav-list` gives the list semantics and states, `FocusKeyManager` the keys, `matTooltip` the rail names. | S | L |
| Activity bar (`workbench/frame/`): badges, CDK menu fly-out, More overflow | `matBadge`; CDK menu (kept) | **Replace the badge, keep the overflow.** `matBadge` puts its description into `aria-describedby` ([docs](https://github.com/angular/components/blob/main/src/material/badge/badge.md)). Our WCAG 2.5.3 rule keeps the count in the accessible name, so set `matBadgeDescription` so it isn't announced twice. No Material component does the overflow. | S | L |
| Status bar (`shell/src/frame/status-bar.ts`): measured compact or overflow form, tooltips via `title` | `matTooltip`; overflow through `cdkMenu` | **Replace the tooltips and the overflow menu, keep the measuring.** No Material component does it. | S | L |
| Copy menu (`src/lib/copy-menu.ts`): hand-rolled `role="menu"` | `cdkContextMenuTrigger` + `cdkMenu` (as the shell menus already use) | **Replace.** The whole shell menu system is already CDK. | S | L |
| Path text (`src/lib/path-text.ts`) and middle text (`table/src/middle-text.ts`): full value in `title` | `matTooltip` + `matTooltipDisabled` when not truncated | **Replace the tooltip.** Keep the middle-truncation measuring, which Material lacks. `middle-text` waits for `table/`. | S | L |
| 76 `title` attributes across the library | `matTooltip` | **Replace, in the package that owns each file.** Our design rule wants themed, in-app tooltips; native `title` is drawn by the OS and is slow to appear. | S per package | L |
| Explorer (`workbench/explorer/`): hand-rolled WAI-ARIA tree, lazy and paged, drag data | `mat-tree` (flat, `levelAccessor`) + `cdkDrag` | **Replace.** `mat-tree` with `levelAccessor`/`childrenAccessor` has the tree pattern and keyboard; `treeControl` does not ([docs](https://github.com/angular/components/blob/main/src/material/tree/tree.md)). Lazy children work by updating the data source on expansion. A "load more" row is our own node type. Virtual scroll is not built in, which our paged sections don't need. | M | M |
| View tree (`view/src/tree.ts`): flat pre-order list with depth, lazy `expand` | `mat-tree` (`levelAccessor`, which matches the flat pre-order model exactly) | **Replace.** | M | M |
| View table (`view/src/table.ts`): sort header buttons, Previous/Next paging, row actions | `mat-table` + `matSort` (`matSortChange` sends the `sort` action) + `mat-paginator` (`page` sends the action) | **Replace.** Sorting and paging stay server-side (the plugin orders the rows), which Material supports by leaving `dataSource.sort` unset. | M | L |
| View matrix (`view/src/matrix.ts`): real `<table>`, sparse cells, cell buttons | `<table mat-table>` with a header-cell first column | **Replace.** It is the same native table, now styled and tested. | S | L |
| View form (`view/src/form.ts`): text, number, choice, toggle, path, secret fields | `mat-form-field`/`matInput`, `mat-select`, `mat-slide-toggle`, `mat-checkbox` | **Replace** after the form controls package. The path and secret pickers stay host pickers. | M | L |
| View node progress (`view/src/node.ts`) | `mat-progress-bar` | **Replace.** | S | L |
| Command palette (`workbench/palette/`): combobox, `aria-activedescendant`, focus kept in the input | `mat-autocomplete` (panel kept open) or CDK `ActiveDescendantKeyManager` | **Replace the key handling with the CDK `ActiveDescendantKeyManager` (D3)** and keep our panel. `mat-autocomplete` was rejected: it would have to be forced open and lives inside an overlay of its own. | S/M | M |
| Dock (`workbench/dock/`): panel tabs, switcher overlay, splitters | dockview owns the tabs and splitters; `mat-tab` doesn't fit drag-out panels | **Keep.** It is dockview's model, and `feat/dock-translucent-redock` is changing it. | – | – |
| Toasts: notification host (`shell/notify/`) | `MatSnackBar` | **Keep.** "Only one snackbar can ever be opened at one time", and a new one dismisses the old ([docs](https://github.com/angular/components/blob/main/src/material/snack-bar/snack-bar.md)). The host stacks one toast per cause with counts and hover-hold (ADR 0010). Waits for `fix/shell-gaps` anyway. | – | – |
| Undo toast (`shell/undo/undo-toast.ts`) | `MatSnackBar` with an Undo action (announces through `aria-live` and never takes focus, which matches our rule) | **Replace (D4)** after `fix/shell-gaps`. Place it so it doesn't overlap the notification stack. | S | M |
| Assistant (`workbench/assistant/`): checkboxes, `role="alertdialog"` confirm | `mat-checkbox` | **Replace the checkboxes.** The confirm moves to the dialog API (waits). | S | L |
| Tray popover, card, metric, status pill, skeleton, empty/error states | none | **Keep.** No Material equivalent: window chrome or domain presentation. `pd-skeleton` could use `mat-progress-bar` only for indeterminate loading, which is a different meaning. | – | – |
| Charts and sparkline (`chart/`, `src/lib/sparkline.ts`, `echarts/`) | none | **Keep** (ECharts/SVG). | – | – |
| Buttons (`.pd-btn*` on native `<button>`) | `matButton` / `matIconButton` | **Replace (D5).** Each package converts the buttons in its own files; the bridge maps the button tokens. | L in total, S per package | M |
| Dialogs (`shell/dialogs`, `shell/confirm`, DialogFrame) | — | **Not re-litigated** (ADR 0026). | – | – |
| `shell/about` (update checkbox, `<progress>`) | `mat-checkbox`, `mat-progress-bar` | **Replace** after `fix/shell-about-styles` merges. | S | L |

### 2.2 Apps (`apps/*/app/src`, grep counts on main)

| App | Hand-rolled → target | Keep | Notes |
|---|---|---|---|
| **Dyno** | `dy-switch` (`shared/switch.ts`, a hand-made `role="switch"`) → `mat-slide-toggle`; 7 native `<select>` (settings, tray-items-editor, rule-dialog, add-folder-dialog, widget-board, counters, reports) → `mat-select`; checkboxes (process-dialogs, cleanup, add-folder, bench, extras-files) → `mat-checkbox`; 4 radio groups (cleanup-confirm, settings, alarms rule-dialog, add-folder) → `mat-radio-group`; 3 hand-rolled tab lists (process-details, alarms-page, extras-page) → `mat-tab-group`/`mat-tab-nav-bar`; 3 `type="time"` (alarm-settings, temporary-dialog, extras-maintenance) → `mat-timepicker`; `memory-slots` `role="tooltip"` → `matTooltip`; lifespan progress bar → `mat-progress-bar`; about 20 plain report `<table>`s → `mat-table` | `dy-popover` (it measures and resizes the tray window), `dy-ring`, `dy-chart`, `dy-sparkline` | Waits: the processes, alarms and network grids (`role="gridcell"`) → shared `pd-table` after `fix/grid-header-on-top`; `dy-toasts`, `dy-dialog` and the confirm → shared shell; the command palette → the shared palette. |
| **Spotter** | checkboxes (settings-page, editor-page, field-form, extraction-panel) → `mat-checkbox`; settings `role="switch"` → `mat-slide-toggle`; 3 `<select>` (pattern-helper, field-form, source-step) → `mat-select`; 3 `type="time"` (settings, schedule-editor, editor-page) → `mat-timepicker`; `layout/operations` `<progress>` → `mat-progress-bar`; `sp-extension-chips` → `mat-chip-set`; 4 tables (preview-panel, extraction-panel, watches, watch-detail) → `mat-table` | `sp-avatar`, status dot, charts | `sp-toasts`, `sp-confirm-dialog` and the copy menu wait for the shared shell. Spotter's own schedule editor could become the shared `pd-schedule-editor` (share first, rule 42). |
| **Pitwall** | 17 radio groups → `mat-radio-group` or `mat-button-toggle-group`; 4 `type="range"` → `mat-slider`; tabs → `mat-tab-group`; 8 progress bars → `mat-progress-bar`; checkboxes and radios; tables | tray popover | Already uses `matTooltip` and `mat-slide-toggle`; drops its own `mat.theme`. |
| **Pitcrew** | 12 `<select>`, 12 radios, 12 checkboxes, 6 `type="time"`, tab list, tables | – | Paused app. |
| **Debrief** | 72 `<select>`, 63 checkboxes, 19 radios, 22 hand-rolled `role="tab"`, 4 `role="tooltip"`, 1 `role="slider"`, `datetime-local` → `mat-datepicker` + `mat-timepicker`, 5 `<progress>` and 6 progress bars | timeline, charts, dockview | It already uses `cdkListbox` (26), `cdkTree` (1) and `cdkDrag`, so those stay. Its own grids follow `pd-table`. Largest package: split by feature folder. |

## 3. `pd-table` → `mat-table` / `CdkTable` + `CdkVirtualScroll`

This is decided and in progress, so here is only what the work on `fix/grid-header-on-top` should keep in mind.

- **The sticky header inside `cdk-virtual-scroll-viewport` is a known, open problem.**
  [angular/components#18240](https://github.com/angular/components/issues/18240) (opened 2020-01-21, open, P3,
  `area: cdk/scrolling`): with `sticky: true` on the header row, the header scrolls away after about 10 rows. Commenters
  trace it to the `translateY` the viewport puts on `.cdk-virtual-scroll-content-wrapper`, which carries the header
  along with the rows. The workarounds in the thread are hacks: a negative `translateY`, `transform: none !important`
  (heavily downvoted), and a scroll nudge. The older request
  [#11621](https://github.com/angular/components/issues/11621) ("virtual scroll: sticky header/footer", 2018) was closed
  with the maintainer saying the table would take "a different approach", and nothing shipped. The CDK scrolling guide
  says nothing about sticky content
  ([scrolling.md](https://github.com/angular/components/blob/main/src/cdk/scrolling/scrolling.md)).
- **What I recommend for the branch:** keep the header **outside** the viewport. Render `mat-header-row` in its own
  header table, or as a `mat-table` with only header rows, above a viewport that holds the body table. Sync horizontal
  scroll and column widths (shared `table-layout: fixed` widths from our column model). The alternative is
  `position: sticky` with `top` set to minus `getOffsetToRenderedContentStart()` on every `renderedRangeStream` tick:
  it is cheaper to build but is exactly the fragile pattern the issue describes. The branch name suggests the first
  approach. Either way, add a regression test that scrolls 1,000 rows and asserts that the header's bounding top equals
  the viewport's.
- **What `mat-table` doesn't give us and stays ours:** group rows (`matRowDef` with a `when` predicate is the supported
  hook), a roving cell grid with arrow keys (no Material table keyboard model; `@angular/aria` has a Grid pattern with
  roving or active-descendant focus ([guide](https://angular.dev/guide/aria/grid)) but it isn't installed and does no
  virtualization, so this is for later), the filter field, the column menu (on `cdkMenu`) and middle truncation.
- **Related:** sticky **columns** have their own open offset bug when two are sticky
  ([#18002](https://github.com/angular/components/issues/18002)). Use at most one sticky column.

## 4. Theming: one shared Material theme from `--pd-*`

The bridge (`scss/_material.scss`) already maps the Material 3 system tokens and the main component tokens. What's
missing:

1. **paddock-ui owns `mat.theme`.** `entry-material.scss` should `@include mat.theme((color: (theme-type: color-scheme,
   primary: ...), typography: ..., density: -2))` (or `-3` for compact) **before** the bridge, so that the apps delete
   their own `mat.theme` and hand overrides (Debrief, Pitwall, Pitcrew, counted in plan X37). The `--mat-sys-*`
   overrides then win because they sit after it in the same file. Density comes from `data-density` via
   `mat.theme-overrides` per density selector if the per-token sizes aren't enough.
2. **Bridge tokens for the components this migration adds:** table (`--mat-table-*`), sort header, paginator, tree,
   button toggle, badge, list and nav list, expansion, timepicker, slider and chip set (the bridge has only three chip
   tokens). The `controls.spec.ts` token test is extended to check that every value is a `var(--pd-…)`.
3. **Every app loads `css/material.css`** after `paddock.css` in `angular.json` (and `scripts/new-app.mjs` adds it for
   new apps).
4. **`@angular/material` becomes a peer of `@decryp7/paddock-ui`** and a dependency of the `ui` workspace (the gallery
   and demos).
5. Removing the old classes: `.pd-select`, `.pd-switch`, `.pd-check`, `.pd-segmented`, `.pd-tabs` and `.pd-chip(s)` in
   `_controls.scss` are deleted once their last user has moved (the final package), and so are `.pd-btn*` (D5) and
   `.pd-input`/`.pd-textarea` (D1). `.pd-kbd`, `.pd-field-grid` and `.pd-with-unit` stay.

## 5. Migration plan: work packages

Rules for every package: each owns only the files listed, so packages don't conflict. Public components keep their
`pd-*`/`Su*` API, so consumers outside the package don't change. Run only the affected tests plus the gallery e2e for
the touched components (rule 3). Check for no CSP console errors. Each package converts the `title` tooltips and the buttons
(`matButton`, D5) in its own files, and uses `mat-form-field appearance="outline" floatLabel="always"` for fields (D1).

| WP | Scope → target | Owns (exclusive) | Consumers | Effort | Order / waits |
|---|---|---|---|---|---|
| **WP-0 Foundation** (running on `feat/material-theme`) | §4 points 1 to 4, plus the button tokens (D5) | `ui/package.json`, `ui/projects/paddock-ui/package.json`, `package-lock.json`, `scss/_material.scss`, `scss/entry-material.scss`, `ui/test/tokens/controls.spec.ts`, `scripts/new-app.mjs`, `ui/angular.json` | every app | S | **First** |
| **WP-1 Form controls** | `pd-select` → `mat-select`; `pd-field` → `mat-form-field` (D1); segmented → `mat-button-toggle-group`; day picker → multiple toggles; schedule and pause time → `mat-timepicker` | `select/`, `forms/`, `schedule/`, `src/lib/segmented-control.ts`, `docs/design-guide.md` (rule 11, D1) | Spotter (segmented), settings, view form, notifications panel, review | M | After WP-0. **Spotter 1.0** (segmented control) |
| **WP-2 Shell pages' controls** | switches, checkboxes, radios, `<progress>`, badge → Material | `shell/settings/window/`, `shell/{mcp,channels,setup,storage,packs,service,i18n}/page/`, `shell/diagnostics/` | every app on the shared shell (Dyno and Spotter 1.0 per the shell-in-releases plan) | M | After WP-0 |
| **WP-3 Jobs** | `pd-job` bar → `mat-progress-bar`; jobs popover → `cdkConnectedOverlay` | `src/lib/job.ts`, `src/lib/job-view.ts`, `shell/jobs/` | every app (jobs in the status bar) | S | After WP-0 |
| **WP-4 Frame navigation** | sidebar → `mat-nav-list` + `FocusKeyManager`; activity badge → `matBadge`; status-bar tooltips and overflow; copy menu → `cdkMenu`; path text → `matTooltip` | `shell/pages/`, `shell/src/frame/`, `workbench/frame/`, `src/lib/copy-menu.ts`, `src/lib/path-text.ts` | every app | S/M | After WP-0 |
| **WP-5 Trees** | explorer and view tree → `mat-tree` (`levelAccessor`) | `workbench/explorer/`, `view/src/tree.ts` | Debrief (workbench), plugin views | M | After WP-0 |
| **WP-6 Declarative views** | view table → `mat-table` + `matSort` + `mat-paginator`; matrix → `mat-table`; form → Material fields; node progress | `view/src/` except `tree.ts` | plugin views (Pitwall and Debrief plugins) | M | After WP-1 (the form uses its controls) |
| **WP-7 Palette** | key handling → `ActiveDescendantKeyManager` (D3) | `workbench/palette/` | workbench apps; Dyno's palette later | S/M | After WP-0 |
| **WP-S Spotter** | §2.2 Spotter row; load `material.css` | `apps/spotter/app/src/**`, `apps/spotter/app/angular.json` | Spotter | S/M | After WP-0. **Spotter 1.0** |
| **WP-D Dyno** | §2.2 Dyno row except the waits; add `material.css` | `apps/dyno/app/src/**`, `apps/dyno/app/angular.json` | Dyno | M | After WP-0. **Before Dyno 1.0 (D2)** |
| **WP-PW Pitwall** | §2.2; drop its own `mat.theme` | `apps/pitwall/app/**` | Pitwall | M | Pitwall 1.1 (release order) |
| **WP-PC Pitcrew** | §2.2 | `apps/pitcrew/app/**` | Pitcrew | M | Paused app; later |
| **WP-DB Debrief** (2 to 3 sub-packages by feature folder) | §2.2 | `apps/debrief/app/src/app/<feature>/**` per sub-package; one of them owns `styles.scss`/`angular.json` | Debrief | L | Debrief 1.2 |
| **WP-W1** (waits for `fix/grid-header-on-top`) | `middle-text` tooltip; Dyno and Debrief grids → shared `pd-table` | `table/src/middle-text.ts`; the apps' grid pages | Dyno, Debrief | M | After that branch merges |
| **WP-W2** (waits for `fix/shell-about-styles`) | about and updates dialogs → `mat-checkbox` and `mat-progress-bar` | `shell/about/` | every app | S | After that branch |
| **WP-W3** (waits for `fix/shell-gaps`) | undo toast → `MatSnackBar` (D4); the notification host stays | `shell/undo/` | every app | S | After that branch |
| **WP-W4** (waits for `feat/dialog-api`) | assistant confirm → dialog API; assistant checkboxes | `workbench/assistant/` | workbench apps | S | After that branch |
| **WP-Z Cleanup** | delete the old `_controls.scss` classes; README and CHANGELOG | `scss/_controls.scss`, `CHANGELOG.md` | – | S | Last |

**Parallelism.** After WP-0 (an hour or two), WP-1, 2, 3, 4, 5, 7, S and D can all run at the same time: no two own
the same file, and none touches `table/`, `shell/dialogs`, `shell/confirm`, `workbench/dock`, `shell/about` or
`shell/notify`. With the five-agent limit, run WP-1, WP-2, WP-S, WP-D and WP-4 first (what Dyno and Spotter 1.0 ship),
then WP-3, WP-5, WP-7 and WP-6. `CHANGELOG.md` and `public-api.ts` are integration points: each package writes its
CHANGELOG lines in its brief's hand-back, and the integrating agent adds them.

**Recommended order** (biggest win, lowest risk first):
1. WP-0, because the theme bridge already exists and only needs wiring.
2. The switches, checkboxes, radios and progress bars in WP-2, WP-S and WP-D: low risk, many hand-rolled instances
   deleted, and tested ARIA.
3. `mat-select` and `mat-timepicker` (WP-1, WP-S, WP-D): the native `<select>`/`type="time"` controls ignore the theme
   today.
4. Tooltips and the jobs overlay (WP-3, WP-4).
5. Trees (WP-5).
6. Views (WP-6) and the palette (WP-7).
7. The large app packages (Pitwall, Debrief) in release order.

## 6. What stays custom, and why

- **Dock, panel tabs, splitters:** dockview's model; drag-out panels have no Material equivalent.
- **The notification stack:** `MatSnackBar` shows only one at a time (docs quoted above).
- **The tray popover and Dyno's popover:** window chrome that measures and resizes the OS window.
- **Charts, sparklines, ring, metric, card, status pill, skeleton, empty and error states:** no Material equivalent.
- **The `pd-table` additions on top of `mat-table`:** grouping, the roving cell grid, column menu and filter, middle
  truncation.
- **Dialogs:** OS windows (ADR 0026).

## 7. Risks

- **Visual change.** Material's geometry (form field notch, toggle sizes, ripples) differs from our drawings. Every
  converted screen gets the independent review of rule 22 (ER-23). Ripples can be switched off globally with
  `MAT_RIPPLE_GLOBAL_OPTIONS` to match our state colours.
- **Density.** Material's minimum touch targets and densities must reach our 28 px control height. The bridge already
  sets `--mat-form-field-container-height` and scales the slide toggle (`zoom: 0.75`). The rest is checked by the token
  test and the gallery e2e.
- **Timepicker values are `Date`s.** Converting at the edge must keep the engine's time zone semantics (pause windows
  "in one time zone"). Add a unit test across midnight and DST.
- **Bundle size:** a few tens of kB per Material module. Not measured here; WP-0 should record bundle sizes before and
  after with the existing bundle budgets.
- **Sticky header with virtual scroll** (§3).

## 8. Decisions (the user, 2026-10-28)

- **D1:** use `mat-form-field appearance="outline"` with `floatLabel="always"` everywhere. Design rule 11 in
  `docs/design-guide.md` changes to match (the label sits in the outline's notch, always floated), and WP-1 owns that
  edit. No bridge override moves the label.
- **D2:** migrate Dyno's controls (WP-D) **before** Dyno 1.0.
- **D3:** the palette keeps our panel and uses the CDK `ActiveDescendantKeyManager` for highlight and keys.
- **D4:** the undo toast moves to `MatSnackBar` once `fix/shell-gaps` has merged (WP-W3). The notification stack stays
  custom.
- **D5:** use `matButton`/`matIconButton` everywhere, as part of each package's sweep over its own files. `.pd-btn*`
  goes in WP-Z once the last user has moved. WP-0 adds the button tokens to the bridge.

WP-0 (the shared theme) is running on `feat/material-theme`.

## Sources (fetched 2026-10-28)

- Snack bar, "Only one snackbar can ever be opened at one time":
  https://github.com/angular/components/blob/main/src/material/snack-bar/snack-bar.md
- Timepicker (DateAdapter, interval, combobox pattern):
  https://github.com/angular/components/blob/main/src/material/timepicker/timepicker.md
- Tree (`levelAccessor`/`childrenAccessor`, accessibility):
  https://github.com/angular/components/blob/main/src/material/tree/tree.md
- Button toggle (accessibility): https://github.com/angular/components/blob/main/src/material/button-toggle/button-toggle.md
- Badge (accessibility): https://github.com/angular/components/blob/main/src/material/badge/badge.md
- CDK scrolling: https://github.com/angular/components/blob/main/src/cdk/scrolling/scrolling.md
- Sticky header with virtual scroll: https://github.com/angular/components/issues/18240 and
  https://github.com/angular/components/issues/11621; sticky columns: https://github.com/angular/components/issues/18002
- Angular Aria (patterns: Autocomplete, Listbox, Select, Multiselect, Combobox, Menu, Menubar, Toolbar, Accordion, Tabs,
  Tree, Grid): https://angular.dev/guide/aria/overview, https://angular.dev/guide/aria/grid. Its stability label was not
  shown on the page (unverified).
- The material.angular.dev pages render client-side and could not be fetched. The component docs above are the same
  Markdown sources from the angular/components repository (main branch, Material 22.x).
