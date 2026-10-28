# Angular Material and CDK audit: addendum (2026-10-28)

**Question.** The user asked to "check other places which should use the material libraries". Which places in
`ui/projects/**` and `apps/*/app/src/**` should use Angular Material or the CDK (rule 47: a Material component wherever
one fits, the CDK otherwise) and are **not** already listed in
[2026-10-28-material-cdk-audit.md](2026-10-28-material-cdk-audit.md)?

**Scope and method.**
- I read the code at `origin/main` 78d5b0f3c (after the WP-0 theme merge). I used `git archive` and grep over every
  `.ts` and `.html` source except specs, e2e and the gallery. No builds and no test runs.
- I checked what the in-flight branches already change (`git diff origin/main...<branch>`) and left it out:
  `feat/material-wp1-forms`, `feat/material-wp2-shell-pages`, `feat/material-wp4-frame-nav`, `fix/material-state-layer`,
  `fix/grid-header-on-top`, `feat/dialog-api` (it puts the shared dialog host on `cdkTrapFocus`), and the uncommitted
  `feat/icons-mat-registry` worktree (its conformance check C46 flags `<svg>` in templates).
- **Verified** below means I read the code at the cited line. CDK behaviour marked *(read in source)* was checked in the
  installed `@angular/cdk` 22.2.4 `fesm2022` sources. Statements about Material docs that I did not fetch in this
  sweep are marked *(unverified)*.
- Counts come from grep. They are approximate: a few `title=` hits are component inputs, and a few `<button>`s are
  legitimately rows or tabs.

**Status of the audit's waits.** `fix/shell-gaps` and `fix/shell-about-styles` are now ancestors of `origin/main`, so
WP-W2 (about) and WP-W3 (undo toast) can start. `feat/dock-translucent-redock` has not merged, so the dock items below
still wait.

Effort: S under a day, M one to three days, L more. "WP" is the audit's work package, or a new one from §8.

## 1. Summary

### Counts of new findings

| Area | New findings (IDs) | Verdict keep or retire into shared | Sizing gaps in existing WPs (§5) |
|---|---|---|---|
| Shared library (`ui/projects/paddock-ui`, demos) | 15 (A1 to A15; A16 is already covered) | 3 more listed as staying custom | 75 `title`, about 200 hand buttons |
| Cross-app (share first, rule 42) | 6 (X1 to X6) | X5 and X6 retire into shared | – |
| Debrief | 22 (DB1 to DB22) | DB12 keep; DB4 retire; 6 more staying custom | 364 `title`, 176 text-like inputs, 36 textareas, about 970 hand buttons |
| Dyno | 8 (DY1 to DY8) | DY8 retire; 5 staying custom | 104 `title`, 30 text-like inputs, about 190 hand buttons |
| Spotter | 5 (SP1 to SP5) | SP4 keep; SP5 retire | 41 `title`, 28 text-like inputs |
| Pitwall | 7 (PW1 to PW7) | 3 staying custom | 27 text-like inputs, about 130 hand buttons |
| Pitcrew | 6 (PC1 to PC6) | PC6 retire; 1 staying custom | 31 `title`, 41 text-like inputs |
| Icons cross-check | 4 items for the icons branch | – | – |

### Top 10 by value

1. **X1 One clipboard mechanism.** 32 direct `navigator.clipboard` calls bypass the shared `copyText`. Five apps keep
   their own copy of it. Move it onto the CDK `Clipboard` as a fallback and add a conformance check. (S/M, new WP-8)
2. **X2 One list keyboard mechanism on the CDK key managers.** Debrief's `list-navigation.ts` and Spotter's
   `row-nav.ts` already wrap `FocusKeyManager`/`ActiveDescendantKeyManager`. Pitwall's `row-nav.ts`, Debrief's
   `roving-tabindex.ts`, five Dyno views, the settings window nav and two app sidebars do the same by hand. (M, new WP-9)
3. **A1 Shell status popovers** (notifications and service health) → `cdkConnectedOverlay`. They are the same
   pattern as the jobs popover the audit moves in WP-3. (S, WP-3)
4. **X3 Visually hidden class** defined about 35 times (`.sr`, `.sr-only`, `.visually-hidden`,
   `.pd-visually-hidden`) → `.cdk-visually-hidden` from the CDK a11y styles. (S, WP-8 plus each app WP)
5. **A3/X4 Hidden live regions → `LiveAnnouncer`.** 2 in the library and 8 in Debrief. (S, WP-8 and WP-DB)
6. **PW1 Pitwall's `pw-select` (24 users) and DB5 Debrief's `kind-filter`** are hand-made selects on CDK listbox and
   overlay → `mat-select` (and `mat-select multiple`). They are the same case as `pd-select` in WP-1. (M, WP-PW and WP-DB)
7. **Hand-built segmented groups** → the shared segmented control (WP-1 puts it on `mat-button-toggle-group`). Dyno
   has about 12 `role="group"` segments, Debrief 14 and Pitwall several. The audit counted only their radio groups.
   (M total, app WPs)
8. **Steppers:** Pitcrew's register wizard and Spotter's editor steps → `mat-stepper`. (M, WP-PC and WP-S)
9. **Chips:** removable chip inputs (Debrief perfmon processes, Pitcrew labels and string lists) → `mat-chip-grid`.
   Pressed filter chips (Debrief alerts and monitor, Dyno history, Pitwall discover, the shared assistant) →
   `mat-chip-listbox`. (S each)
10. **Hand spinners → `mat-progress-spinner`.** 2 in the library, 7 in Debrief, 3 `@keyframes spin` in Dyno, 1 in
    Pitwall and a spinning icon in Pitcrew. (S, each package)

## 2. Shared library (`ui/projects/paddock-ui`)

| ID | Where (file:line) | Now | Material / CDK | Verdict and why | Size | WP |
|---|---|---|---|---|---|---|
| A1 | `shell/notify/src/notifications-status.ts:21,31`; `shell/notify/src/service-status.ts:25,34` | An absolutely placed `.pd-popover` with `(document:pointerdown)` outside-click and Escape handled by hand | `cdkConnectedOverlay` with `(overlayOutsideClick)` and `(detach)` | **Replace.** It is the same pattern as `jobs-status.ts`, which the audit already moves. All three status-bar popovers should share one overlay config (flip at the window edge, z-order above the dock). | S | WP-3 (widen its scope to these two files) |
| A2 | `src/lib/clipboard.ts:6-13` (`copyText`) | `navigator.clipboard.writeText` only; returns false when it is refused | CDK `Clipboard.copy()` as the fallback | **Replace the fallback** (decision D-A). CDK `Clipboard` copies through a hidden read-only textarea and `execCommand('copy')`, then puts the focus back *(read in source)*. Today a refused async clipboard leaves the user with "select it and press Ctrl+C". Keep the signature. | S | new WP-8 |
| A3 | `src/lib/job.ts:72`; `workbench/assistant/src/assistant.ts:143` | A visually hidden `role="status" aria-live` element bound to a signal | `LiveAnnouncer.announce(text, 'polite')` | **Replace.** `pd-job` renders one live region per job row, so a list of jobs gives several competing live regions. `LiveAnnouncer` keeps one per window. Keep the visible text. | S | WP-8 (job.ts is WP-3's file: do it in WP-3) |
| A4 | `scss/_base.scss:162` `.pd-visually-hidden`, plus copies in `src/lib/metric.ts:60`, `src/lib/job.ts:102`, `workbench/assistant/src/assistant.ts:240` (`.sr-only`) | Four hand definitions | `.cdk-visually-hidden` (CDK a11y styles, which Dyno already loads) or the `cdk.a11y-visually-hidden()` mixin | **Replace.** One definition. Keep `.pd-visually-hidden` as an alias that `@extend`s the CDK class if the public CSS name must stay. | S | WP-8 |
| A5 | `shell/src/frame/regions.ts:24,46` | A `FOCUSABLE` selector string for F6 region focus | CDK `InteractivityChecker.isFocusable`/`isTabbable` | **Replace.** The selector also matches controls inside hidden containers and misses `summary` and `[contenteditable]`. The CDK checker tests visibility as well. | S | WP-8 |
| A6 | `workbench/frame/src/frame.ts:165` | `this.doc.dir === 'rtl'` read on every sash drag | `inject(Directionality).value` | **Replace (small).** `Directionality` reads the same `body`/`html` `dir` that `shell/i18n/src/boot.ts:68` sets *(read in source)*, and it is what the Material components read, so they all agree. | S | WP-4 follow-up or WP-8 |
| A7 | `shell/settings/window/src/settings-window.ts:100,322-326` | A hand-made Up/Down/Home/End roving focus over the settings page buttons. WP-2 converts the controls but not this. | `mat-nav-list` + `FocusKeyManager` (the audit's sidebar pattern) | **Replace.** It is the same component class as the pages sidebar in WP-4. | S | WP-9 |
| A8 | `workbench/assistant/src/assistant.ts:182` | Toggle "chips" are `<button class="chip" aria-pressed>` | `mat-chip-listbox multiple` with `mat-chip-option` | **Replace.** The listbox semantics fit a set of on/off context chips better than separate pressed buttons. | S | WP-W4 (owns `workbench/assistant/`) |
| A9 | `workbench/assistant/src/assistant.ts:193-196` | Composer `<textarea rows="3">` | `matInput` + `cdkTextareaAutosize` (`cdkAutosizeMinRows`/`MaxRows`) | **Replace.** It grows with the question, the way Debrief's composer does by hand (DB9). | S | WP-W4 |
| A10 | `workbench/assistant/src/assistant.ts:151`; `shell/notify/src/waiting-for.ts:147`; `shell/notify/src/service-status.ts:66`; `view/src/scene.ts:87` | `<details>`/`<summary>` disclosures | `mat-expansion-panel` (or `cdkAccordionItem` when unstyled) | **Decision D-B.** A native `<details>` is accessible already. `mat-expansion-panel` adds the theme, animation and a consistent header. My recommendation: use Material for section-level disclosures and keep `<details>` for a one-line "Technical details" inside a notice or popover, where a panel header is too heavy. | S | WP-3 or WP-W3 (notify), WP-6 (scene), WP-W4 |
| A11 | `shell/notify/src/waiting-for.ts:70` `.pd-spinner`; `workbench/assistant/src/tool-cards.ts:51` `.spin` | CSS spinners | `mat-progress-spinner mode="indeterminate" [diameter]="14"` | **Replace.** Check that reduced motion is still respected (Material's own handling is *unverified*: e2e with `prefers-reduced-motion`). | S | WP-3 / WP-W4 |
| A12 | `shell/mcp/dialog/src/mcp-key-dialog.ts:24,40`; `shell/channels/dialog/src/channel-secret-dialog.ts:30` | Plain `<input>` (key display, secret) | `mat-form-field` + `matInput` (D1); the key's Copy goes through A2 | **Replace** after `feat/dialog-api` merges (it rewrites these files). The audit's WP list has no package for the dialog contents. | S | new WP-W5 |
| A13 | `workbench/dock/src/dock-host.ts:115` | A hand `role="menu"` `<ul>` with `(keydown)` arrow handling, placed by `menu-placement.ts:31-35` | `cdkMenu` opened by `cdkContextMenuTrigger` (the shell menus already do this) | **Replace, after `feat/dock-translucent-redock`.** The menu is our own Angular template, not dockview's, so the audit's dock "keep" does not cover it. | S | new WP-W6 |
| A14 | `workbench/dock/src/switcher-overlay.ts:20` | A `role="listbox"` with `aria-activedescendant` by hand | `ActiveDescendantKeyManager` (D3's pattern) | **Replace the keys**, after the dock branch. | S | WP-W6 |
| A15 | `ui/projects/shell-demo/src/app/tray-popover.ts:95` | A hand `role="progressbar"` bar | `mat-progress-bar` | **Replace** (demo, but it is what new apps copy). | S | WP-3 |
| A16 | `shell/src/frame/status-bar.ts:83` | "More" toggle with `aria-expanded` | Already in WP-4 (overflow → `cdkMenu`) | Covered; listed only so it is not counted twice. | – | WP-4 |

Stays custom in the library (in addition to the audit's §6):
- The dock's imperative glyphs and tabs (`dock-glyph.ts`, `group-actions.ts`, `panel-tab.ts`): dockview owns that DOM.
  Only their icons move (§6).
- ResizeObserver measuring (`status-bar.ts:114`, `pages/src/frame.ts:123`, `path-text.ts:74`, the dock):
  `BreakpointObserver` watches viewport media queries, not element sizes, so it does not fit.
- `appearance.store.ts:23` `matchMedia` for the colour scheme: `MediaMatcher` would be the same thing behind one more
  layer. No gain.

## 3. Cross-app findings (share first)

| ID | Where | Now | Target | Verdict and why | Size | WP |
|---|---|---|---|---|---|---|
| X1 | Direct `navigator.clipboard`: Debrief 7 (for example `inspector/inspector.ts:367`, `inspector/context-section.ts:151`, `plugins/registry/registry-view.ts:225`, `store-pane/sql-console.ts:286`, `dock/angular-panel.ts:197`, `sources/paste/paste-dialog.ts:76` (read)), Dyno 10 (`why-slow-page.ts:203`, `process-details.ts:237`, `network-page.ts:489`, `hash-search-dialog.ts:98`, `diagnostics-page.ts:156`, `counters-page.ts:398`, `backup-page.ts:224`, `issues-view.ts:345`, `unusual-view.ts:225`, `shared/copy-menu.ts:29`), Pitwall 5 (`native-feel.ts:247`, `settings-page.ts:273`, `apps-page.ts:421`, `key-secret-dialog.ts:139`, `shared/ui/fact.ts:57`), Pitcrew 5 (`updates-page.ts:240`, `engine-section.ts:281`, `logs-page.ts:111`, `processes-panel.ts:91`, `shared/ui/copy-menu.ts:29`), Spotter 2 (`shared/clipboard.ts:11`, `shared/ui/copy-menu.ts:29`), library 1 (`workbench/dock/src/angular-panel.ts:252`). Private copies of `copyText`: `apps/debrief/.../tools/clipboard.ts`, `apps/spotter/.../shared/clipboard.ts`. Debrief already uses CDK `Clipboard` in `ai/manual/manual-download.ts:203`. | Several mechanisms, and several call sites swallow the failure (`.catch(() => undefined)`) | Every copy goes through the shared `copyText` (A2) | **Replace.** It is one mechanism, gives the same failure message everywhere and has the CDK fallback. Add a conformance check: no `navigator.clipboard` outside `src/lib/clipboard.ts` (reading the clipboard in the paste dialog excepted). | M (about 30 call sites, mechanical) | WP-8 plus app WPs |
| X2 | Keyboard list and row navigation. CDK-based: Debrief `shared/list-navigation.ts:1-143` (FocusKeyManager, ActiveDescendantKeyManager), Spotter `shared/ui/row-nav.ts:1`. By hand: Pitwall `shared/ui/row-nav.ts:8` (8 users), Debrief `shared/roving-tabindex.ts:13` (5 users), Dyno `issues-view.ts:310`, `unusual-view.ts:199`, `backup-page.ts:212`, `alarms-active.ts:188`, `recorder/snapshot-list.ts:57`, Debrief `jobs/jobs-panel.ts:306`, `markers/glyph-picker.ts:141`, Spotter `layout/sidebar.ts:164`, Pitcrew `layout/sidebar.ts:202`, library A7 | Four helpers and about ten one-off handlers | One shared `pdListNavigation` directive in paddock-ui on `FocusKeyManager` (roving tabindex) or `ActiveDescendantKeyManager` (focus kept in an input), seeded from Debrief's `list-navigation.ts` | **Replace.** The CDK owns wrap, Home/End, typeahead and skipping disabled items. The shared directive is tested once (rule 42). The app sidebars could instead move to the shared pages sidebar (WP-4) when the app adopts the shell. | M | new WP-9 |
| X3 | Visually hidden classes: about 35 definitions. Debrief has 27 component-local `.sr`/`.sr-only` (for example `shell/sidebar.ts:214`, `jobs/jobs-panel.ts:149`, `timeline/runs-view.ts:143`) plus `styles.scss:498`; Dyno `styles.scss:422`; Pitwall `styles.scss:701`; Pitcrew `styles/_base.scss:93`; shell-demo 2 | Copies, and some differ (`clip` against `clip-path`, with or without `white-space`) | `.cdk-visually-hidden` | **Replace.** One tested definition. | S per app | WP-8 / app WPs |
| X4 | Visually hidden live regions in Debrief: `ai/runtimes/runtime-panel.ts:47`, `ai/models/model-manager.ts:91`, `ai/manual/manual-download.ts:127`, `profiling/flame/flame-chart.ts:64`, `jobs/jobs-panel.ts:52`, `notifications/notifications-panel.ts:41`, `onboarding/tour-overlay.ts:42`, `share/dashboard/dashboard-view.ts:61` | Hidden `aria-live` elements bound to a signal | `LiveAnnouncer` | **Replace.** It is the same as A3. Visible `role="status"` regions stay declarative. | S | WP-DB |
| X5 | Copy menus copied into apps: `apps/dyno/.../shared/copy-menu.ts:40-43`, `apps/spotter/.../shared/ui/copy-menu.ts:40-43`, `apps/pitcrew/.../shared/ui/copy-menu.ts:40-43` (hand `role="menu"`, `document:mousedown`) | Three copies of the library's copy menu | The shared copy menu, which WP-4 puts on `cdkMenu` | **Retire the copies.** The audit mentions only Spotter's. | S each | WP-D, WP-S, WP-PC |
| X6 | Toast hosts not named in the audit: Pitwall `shared/ui/toast-host.ts:5` (a stack), Pitcrew `layout/toasts.ts` | Own stacks | The shared notification host (stays custom, audit §6) | **Retire into the shared shell**, as the audit already says for Dyno and Spotter. Not `MatSnackBar`: it shows one toast at a time. | – | WP-PW, WP-PC (with the shell) |

## 4. Apps

### 4.1 Debrief (`apps/debrief/app/src/app/`)

| ID | Where | Now | Material / CDK | Verdict and why | Size |
|---|---|---|---|---|---|
| DB1 | `ui/dialog-host.ts:7,109,121-132` (107 files use the dialog service) | A hand Tab-cycling focus trap | `cdkTrapFocus` (what `feat/dialog-api` gives the shared host) | **Replace, or retire into the shared dialog API** when Debrief adopts it. Until then, a one-line `cdkTrapFocus` swap removes the hand trap. | S |
| DB2 | `ui/context-menu-host.ts:12,58-59` (3 users) | A hand `role="menu"` with arrow keys, Escape, outside click and edge clamping through `el.style.left/top` | `cdkContextMenuTrigger` + `cdkMenu` | **Replace.** Debrief already uses `@angular/cdk/menu` in 9 files. | S |
| DB3 | `shared/click-outside.ts:7` with its popovers: `workspace/filter-bar.ts:59,95,100,110,117`, `workspace/facet-popover.ts:31`, `lenses/lens-bar.ts:19,26`, `grid/events-grid.ts:124-127`, `windowing/background-work.ts:14,28`, `analysis/query-field.ts:52,103` | Absolutely placed `.popover` elements and a home-made outside-click directive | `cdkConnectedOverlay` (`overlayOutsideClick`, flip); the filter bar's "More" (`role="menu"`, line 100) → `cdkMenu` | **Replace.** Delete `ClickOutside` when the last user has moved. | M |
| DB4 | `shell/status-bar.ts:66` `details-pop`; `health/service-status-chip.ts:50` | Debrief's own status popovers | The shared status bar and service chip (A1) | **Retire into the shared shell.** | S |
| DB5 | `ui/kind-filter.ts:1-2` (3 users) | A multi-select dropdown on `cdkListbox` + `cdkConnectedOverlay` | `mat-select multiple` | **Replace.** It is a select. The audit's "cdkListbox stays" fits the inline listboxes, not this dropdown. | S |
| DB6 | `shared/switch.ts:5-8` (18 users) | A native checkbox as `role="switch"` | `mat-slide-toggle` | **Replace.** The audit counts Debrief's checkboxes but not this component. One component change converts 18 call sites. | S |
| DB7 | `shared/tabs.ts:13,67-71` (6 users) and the heap view tabs `profiling/heap/heap-view.ts:96-101` | A hand tab strip with arrow keys | `mat-tab-nav-bar` / `mat-tab-group` | Covered by the audit's "22 role=tab". Do `tabs.ts` first: it is the one component behind most of them. | S |
| DB8 | `shared/roving-tabindex.ts` (5 users), `jobs/jobs-panel.ts:306`, `markers/glyph-picker.ts:141` | Hand roving focus | WP-9's shared directive; the glyph picker → `cdkListbox` (horizontal) | **Replace.** | S |
| DB9 | `ai/chat/chat-composer.ts:23,342`; `analysis/query-field.ts:301` | Height computed from `scrollHeight` and written to `el.style.height` | `cdkTextareaAutosize` | **Replace.** The CDK handles min and max rows and resizes on input and on window resize. | S |
| DB10 | `workspace/filter-bar.ts:66`, `profile/profile-form.ts:41`, `projects/bookmarks/highlight-editor.ts:17` | `<datalist>` suggestions (an OS popup that ignores the theme) | `mat-autocomplete` | **Replace.** | S |
| DB11 | `analysis/query-field.ts:214`, `live/ql-input.ts:108` | Completion popups: `ActiveDescendantKeyManager` (good) in a hand-placed popup | Keep the key manager; place the popup with `cdkConnectedOverlay` | **Partial**, like D3 for the palette: `mat-autocomplete` would fight the caret-anchored completion. | S |
| DB12 | `ai/folder-setup/folder-setup-wizard.ts:41-43` | `<ol class="steps">` showing the current step (the engine drives the flow) | `mat-stepper` header (linear, not editable), or keep | **Keep the indicator.** Engine-owned state (`folder-setup.store`) would have to drive `selectedIndex`, and the gain is visual only. `ai/setup/setup-wizard.ts` is a state machine (`state().kind`), not a stepper. | – |
| DB13 | Chips: `tools/perfmon/perfmon-view.ts:41` (removable process chips); `tools/monitor/monitor-live-charts.ts:26` (single window choice), `:45` (legend toggles); `live/alerts-view.ts:22-24` (single filter); `analysis/moment-view.ts:93` (action chip) | Hand `.chip` buttons with `aria-pressed` | Removable → `mat-chip-grid` + `mat-chip-row`; single → `mat-chip-listbox`; multiple → `mat-chip-listbox multiple`; action → `matButton` | **Replace.** | S |
| DB14 | Spinners: `metrics/chart-card.ts:28`, `tools/monitor/monitor-live-charts.ts:39`, `shell/status-bar.ts:40`, `health/waiting-for.ts:14`, `profile/extractor-editor.ts:82`, `windowing/background-work.ts:25`, `timeline/timeline-header.ts:47` (6 `@keyframes spin`) | CSS spinners | `mat-progress-spinner` | **Replace.** | S |
| DB15 | `store-pane/store-pane.ts:78-79` | Previous/Next paging buttons with offset and total | `mat-paginator` | **Replace.** The "Show more" buttons (`timeline/runs-view.ts:113`, `store-pane/sql-console.ts:128`) are incremental loading, not paging: they stay buttons. | S |
| DB16 | Hand sort headers: `analysis/query-results-view.ts:33,99`, `profiling/hotspots/hotspots-view.ts:58`, `profiling/heap/heap-types.ts:30`, `timeline/runs-view.ts:75` | `aria-sort` by hand | `matSort` + `mat-sort-header` (sorting stays in the engine; leave `dataSource.sort` unset) | **Replace**, with these tables (audit: "grids follow pd-table"). The events grid (`grid/events-grid.ts:159`) stays on the pd-table path. | S each |
| DB17 | `problems/problems-panel.ts:71-98` | A `role="tree"` on FocusKeyManager, files fold and unfold | `mat-tree` (`levelAccessor`) | **Replace** (WP-5's pattern). | S/M |
| DB18 | `workspace/virtual-list.ts:41` (users `workspace/facet-popover.ts:55`, `grid/events-grid.ts:129`); `profiling/hotspots/hotspots-view.ts:18`; `about/notices-text.ts:19` / `about/about-dialog.ts:127` | Hand windowing | `cdk-virtual-scroll-viewport` (fixed size) | **Replace** for these lists (facet values, column choices, hotspots, notices). The About dialog moves to the shared about anyway. The **events grid** keeps `grid/scroll-map.ts`: it maps 50M rows into a 10M px spacer, which the CDK viewport cannot do. | S each |
| DB19 | `onboarding/tour-overlay.ts:31` | A popover placed next to the toured element by hand | CDK `Overlay` + `flexibleConnectedTo(target)` | **Replace the positioning**; the spotlight and the step logic stay. | S |
| DB20 | `ui/help-hint.ts:10-21` (14 users) | A "?" `<button>` with `title` that toggles an in-flow note | `matIconButton` + `matTooltip` for the button. The note stays in flow (it is a toggletip, not a tooltip). | **Partial.** | S |
| DB21 | 14 `role="toolbar"` (for example `notes/notes-panel.ts:27`, `ai/chat/chat-panel.ts:41`, `jobs/jobs-panel.ts:27`) | `role="toolbar"` without the toolbar's arrow-key model | Either WP-9's directive (horizontal `FocusKeyManager`) or drop the role | **Fix the semantics.** `mat-toolbar` is styling only and does not help. | S |
| DB22 | Reordered lists without drag: `profile/string-list.ts:20`, `investigate/playbooks/playbook-editor.ts:40`, `dock/layout-dialogs.ts:68` | Move up/down buttons only | Add `cdkDropList` + `cdkDrag`, keeping the buttons (as `profile/pipeline-editor.ts:285-289` already does) | **Optional**, for consistency with the pipeline editor. | S |

Stays custom in Debrief: the treegrids (`profiling/tree/call-tree-view.ts:37`, `profiling/heap/heap-dominators.ts:55`,
`config-diff/config-diff-view.ts:169`, which is virtualised and paged): `mat-tree` has no treegrid with columns, so they
wait for `pd-table`'s grouping and roving grid (WP-W1). Also staying: chart and timeline tooltips that follow the
pointer (`timeline/timeline-tooltips.ts:23-69`, `profiling/flame/flame-chart.ts:66`), the native drag sources and drops
(`grid/events-grid.ts:185`, `notes/notes-panel.ts:22`, `ai/chat/chat-composer.ts:47`, which carry `dataTransfer` and
OS file drops that `cdkDrag` does not handle), `<input type="color">` (2, no Material colour picker), and the SQL
console history keys.

### 4.2 Dyno (`apps/dyno/app/src/app/`)

| ID | Where | Now | Target | Verdict | Size |
|---|---|---|---|---|---|
| DY1 | Segmented `role="group"` button groups: `processes/processes-page.ts:38`, `cpu/cpu-detail.ts:72`, `network/network-page.ts:80,87,92`, `dashboard/history-panels.ts:34,131,180`, `dashboard/tiles.ts:32`, `inventory/inventory-page.ts:60`, `counters/counters-page.ts:37` | Hand segments | The shared segmented control (WP-1 → `mat-button-toggle-group`) | **Replace.** The audit counts only Dyno's 4 radio groups. | M |
| DY2 | `extras/extras-maintenance.ts:52` | Weekday chips with `aria-pressed` | The shared `pd-day-picker` (WP-1 → multiple toggles) | **Replace** (share first). | S |
| DY3 | `dashboard/history-panels.ts:44,49`; `issues/issues-view.ts:98` | Pressed chip toggles and a "clear filter" chip | `mat-chip-listbox multiple`; `mat-chip` with `matChipRemove` | **Replace.** | S |
| DY4 | Up/Down row handlers in `issues-view.ts:310`, `unusual-view.ts:199`, `backup-page.ts:212`, `alarms-active.ts:188`; `recorder/snapshot-list.ts:16,57` (`role="listbox"` by hand) | Hand keys | WP-9's directive; the snapshot list → `cdkListbox` | **Replace.** | S |
| DY5 | Spinners: `backup/backup-page.ts:166`, `issues/issues-page.ts:66`, `extras/extras-styles.ts:33` | `@keyframes spin` | `mat-progress-spinner` | **Replace.** | S |
| DY6 | Sort headers: `throttling/throttling-page.ts:62`, `extras/extras-apps.ts:23-29` | `aria-sort` and header buttons by hand | `matSort` on the `mat-table` the audit already plans | **Replace** with those tables. | S |
| DY7 | `alarms/rule-dialog.ts:48-51` | A number with −/+ buttons | `mat-form-field` + `matInput type="number"` with `matIconButton` suffixes | **Replace** (D1). | S |
| DY8 | `shared/copy-menu.ts` | See X5 | Shared copy menu | **Retire.** | S |

Stays custom in Dyno: the illustrations (`inventory/memory-slots.ts`, `device-art.ts`, `chip-package.ts`,
`benchmarks/bench-art.ts`), `shared/ring.ts`, `shared/sparkline.ts`, the recorder timeline, the tray-item preview ring
(`settings/tray-items-editor.ts:39`), and `cdkDrag` boards (already CDK).

### 4.3 Spotter (`apps/spotter/app/src/app/`)

| ID | Where | Now | Target | Verdict | Size |
|---|---|---|---|---|---|
| SP1 | `features/editor/editor-page.html:9-12,226-232` (steps Source → … → Review, Back/Next) | A hand step list with `aria-current="step"` and reachable steps | `mat-stepper` (`linear`, header buttons for reachable steps; the store drives `selectedIndex`) | **Replace** (decision D-C on timing: Spotter 1.0 is close). It gives the step header semantics, keys and themed states. | M |
| SP2 | `features/watches/watches-page.html:40-43` | `aria-sort` headers by hand | `matSort` on the `mat-table` the audit plans | **Replace.** | S |
| SP3 | `features/editor/pattern-helper.ts:53`, `preview-panel.ts:66`, `field-form.ts:22`, `source-step.ts:125` | `<details>` | D-B | As decided in D-B. | S |
| SP4 | `features/dashboard/events-feed.ts:38` | "Load more" | Stays a button (incremental) | **Keep.** | – |
| SP5 | `layout/sidebar.ts:164-168`, `shared/clipboard.ts`, `shared/ui/copy-menu.ts` | Hand keys, private `copyText`, copy menu | The shared shell sidebar (WP-4) or WP-9; X1; X5 | **Retire into shared.** | S |

### 4.4 Pitwall (`apps/pitwall/app/src/app/`)

| ID | Where | Now | Target | Verdict | Size |
|---|---|---|---|---|---|
| PW1 | `shared/ui/select.ts:13` (24 users) | A themed dropdown on `cdkListbox` + `cdkConnectedOverlay` | `mat-select` (through the shared `pd-select` wrapper from WP-1) | **Replace.** It is the same component as `pd-select`. | M |
| PW2 | `features/requests/requests-page.ts:113,119` | A "Columns" popover with `role="menu"` and Escape by hand | `cdkMenu` with `cdkMenuItemCheckbox` (as `pd-table`'s column menu) | **Replace.** | S |
| PW3 | `shared/ui/row-nav.ts:8` (8 users) | Pure arrow/Home/End functions | WP-9's directive | **Replace.** | S |
| PW4 | `features/models/discover-tab.ts:30-32`; `features/playground/playground-page.html:43` | Pressed tag chips; suggestion chips | `mat-chip-listbox`; `mat-chip-set` with `matChipAction`, or `matButton` | **Replace.** | S |
| PW5 | `features/models/presets-tab.ts:94` | `<datalist>` | `mat-autocomplete` | **Replace.** | S |
| PW6 | `features/benchmarks/benchmarks-page.ts:55,191` | CSS spinner | `mat-progress-spinner` | **Replace.** | S |
| PW7 | `features/engines/engine-card.ts:81,111`; `features/models/library-tab.ts:216` | Expandable rows with `aria-expanded` | `mat-table` detail rows (`multiTemplateDataRows`) | **Replace** with the audit's Pitwall tables. | S |

Stays custom in Pitwall: `shared/ui/toast-host.ts` until the shared shell (X6), `shared/ui/sparkline.ts`, and the
tray popover.

### 4.5 Pitcrew (`apps/pitcrew/app/src/app/`, paused app)

| ID | Where | Now | Target | Verdict | Size |
|---|---|---|---|---|---|
| PC1 | `features/register/register-wizard.html:11-28,149-159` | A hand linear stepper (done, current, back-navigable) | `mat-stepper linear` | **Replace.** This is the cleanest stepper fit in the family. | M |
| PC2 | `features/register/register-wizard.html:60`; `features/runners/config/string-list.ts` | Removable label chips with an input | `mat-chip-grid` + `mat-chip-row` + `matChipInputFor` | **Replace.** | S |
| PC3 | `features/register/register-wizard.html:159` | `<mat-icon svgIcon="pc:loader" class="spin">` | `mat-progress-spinner` | **Replace.** | S |
| PC4 | `features/activity/activity-page.html:36` | `aria-sort` by hand | `matSort` | **Replace** with its table. | S |
| PC5 | `features/insights/insights-page.html:55` | "Show more" `aria-expanded` link | `mat-expansion-panel` or keep (D-B) | D-B. | S |
| PC6 | `layout/sidebar.ts:202-206`, `layout/toasts.ts`, `shared/ui/copy-menu.ts` | Hand keys, toast stack, copy menu | The shared shell (WP-4 sidebar, notify, copy menu) | **Retire into shared** when Pitcrew resumes. | S |

Stays custom: `shared/ui/update-progress.ts` (a phase display for the update flow, not a wizard).

## 5. Sizing gaps in the audit's work packages

The audit's per-package rule ("each package converts the `title` tooltips and the buttons in its own files, and uses
`mat-form-field` for fields") covers these, but its app rows don't count them. The app packages are bigger than §2.2
of the audit suggests:

| App | `title` → `matTooltip` | Text-like inputs (text, search, number, url, email, password) → `matInput` in `mat-form-field` | `<textarea>` | Hand-styled `<button>` (not matButton, not a CDK menu item or tab) |
|---|---|---|---|---|
| Debrief | 364 | 149 + 27 number | 36 | 90 icon-only, 880 other |
| Dyno | 104 | 20 + 10 number | 1 | 52 icon-only, 142 other |
| Pitwall | 3 (already 76 `matTooltip`) | 11 + 16 number | 3 | 25 icon-only, 106 other |
| Pitcrew | 31 | 27 + 14 number | 1 | 21 icon-only, 111 other |
| Spotter | 41 | 24 + 4 number | 2 | 21 icon-only, 81 other |
| paddock-ui | 75 (audit: 76) | 26 (most in WP-1, WP-2, WP-6) | 7 | 28 icon-only, 170 other (WP-2 and WP-4 convert part of it) |

Recommendation: split **WP-DB** by feature folder into at least four sub-packages, not two or three, and give **WP-D**
an M/L estimate rather than M.

## 6. Icons cross-check (for `feat/icons-mat-registry`)

- **Registries in place.** `MatIconRegistry.addSvgIconLiteralInNamespace` is already used by `pd`
  (`icons/src/register.ts:35`), `debrief` (`shared/material/debrief-icons.ts:92`), `dy` (`shared/icons.ts:106`), `sp`
  (`shared/icons.ts:83`), `pw` (`shared/icons.ts:91`) and `pc` (`shared/icons.ts:108`). Wrapper components still exist:
  `debrief-icon` (`shared/icon.ts:10`), `dy-icon` (`shared/icon.ts:10`), `pd-icon` (`shell/src/frame/icon.ts:56`),
  `pd-glyph` (`icons/src/glyph.ts:11`), and `sd-icon` in the shell demo. There are no icon fonts anywhere.
- **Template `<svg>` icons that C46 will flag (good).** In the library: `shell/dialogs/src/dialog-host.ts`,
  `shell/jobs/src/jobs-status.ts`, `shell/notify/src/{notification-host,notifications-panel (2),notifications-status,waiting-for}.ts`,
  `src/lib/{card,job (3),metric,states (2),status-pill}.ts`, `workbench/dock/src/dock-host.ts:101`,
  `workbench/panels/src/popout-title-bar.ts:50`. In Debrief: `popout/popout-title-bar.ts:36-46` (6), `dock/dock-host.ts:85`,
  `live/severity-badge.ts:17`, `sources/source-swatch.ts:13`.
- **Imperative SVG icons that C46 will not see**, because they are not in a template (`document.createElementNS`):
  `workbench/dock/src/dock-glyph.ts:5`, `workbench/dock/src/group-actions.ts:7`, `workbench/dock/src/panel-tab.ts:78`, and
  Debrief's copies `dock/group-actions.ts:7`. These can take their SVG from the registry with
  `MatIconRegistry.getNamedSvgIcon('pd', name)`, which returns an `SVGElement` *(unverified in this sweep; check the
  API)*. Then one registry draws them too. A C46 extension could flag `createElementNS(..., 'svg')` outside an allow-list.
- **Not icons; they need the `svg-ok` marker:** sparklines and charts (`src/lib/sparkline.ts`, `view/src/scene.ts`,
  Debrief `home/home-view.ts:72-76`, `analysis/patterns-view.ts:39`, `markers/marker-editor.ts:75`, the timeline files,
  `profiling/threads/thread-timeline-view.ts`, `profiling/heap/heap-growth.ts`, `tools/monitor/monitor-live-charts.ts`,
  `share/dashboard/dashboard-tile.ts`, `share/visuals/timeline-svg.ts`), user-defined marker glyphs
  (`markers/marker-glyph.ts:36-46`, sanitised in `markers/glyph-model.ts:114-148`), Dyno's illustrations and
  `ring.ts`/`sparkline.ts`, Pitwall's, Pitcrew's and Spotter's sparklines and charts, and `panel-snapshot.ts`.

## 7. Considered, and it stays as it is

- **`BreakpointObserver`:** nothing in the family uses viewport breakpoints. Every responsive decision measures an
  element (ResizeObserver), which is correct for docked panels.
- **CDK `Platform`:** six Mac checks parse `userAgent` (`shell/feel/src/native-feel.ts:255`, Debrief
  `extensions/keybinding.ts:3`, Spotter `app.config.ts:29`). They are pure, testable and take the window they act on
  (dialog windows). `Platform.MAC` is a singleton for the current window. No gain.
- **`SelectionModel`:** selection lives in SignalStores (`diagnostics/snapshots.store.ts:32`). Moving it into a mutable
  CDK model would bypass the store.
- **`cdkObserveContent` / `ScrollDispatcher`:** no hand-rolled mutation or scroll dispatching that they would replace.
- **`mat-sidenav`, `mat-toolbar`, `mat-grid-list`, `mat-card`:** the frame's resizable sashes, the OS title bar drag
  region, the draggable widget boards (`cdkDrag` already) and `pd-card` (audit §6) have no better Material form.
  `mat-toolbar` adds no keyboard model (see DB21).
- **`mat-datepicker`:** apart from the audit's `datetime-local` there are no date inputs.
- **`matRipple`:** off by design (state-layer fix).

## 8. New work packages

| WP | Scope | Owns (exclusive) | Size | Order / waits |
|---|---|---|---|---|
| **WP-8 Shared a11y utilities** | A2 `copyText` on a CDK `Clipboard` fallback; A4 visually hidden → CDK; A5 `InteractivityChecker`; A6 `Directionality`; a conformance check banning `navigator.clipboard` outside `src/lib/clipboard.ts` | `src/lib/clipboard.ts`, `scss/_base.scss` (the one class), `shell/src/frame/regions.ts`, `workbench/frame/src/frame.ts` (one line, coordinate with WP-4), a new `conformance/src/checks/C4x-clipboard.mjs` | S/M | After WP-0. Before the app packages, so they can adopt it. |
| **WP-9 Shared list keyboard** | X2: a `pdListNavigation` directive on `FocusKeyManager`/`ActiveDescendantKeyManager`, seeded from Debrief's `list-navigation.ts`; adopt in the settings window nav (A7); then the apps adopt it in their own packages | new `ui/projects/paddock-ui/src/lib/list-navigation.ts` (+ spec), `shell/settings/window/src/settings-window.ts` (after WP-2 merges) | M | After WP-2 |
| **WP-3 (widened)** | Add A1 (notifications and service popovers), A3 (`job.ts` announcer), A11, A15 | plus `shell/notify/src/{notifications-status,service-status,waiting-for}.ts`, `ui/projects/shell-demo/src/app/tray-popover.ts` | S → S/M | WP-3's slot (now unblocked: `fix/shell-gaps` has merged) |
| **WP-W4 (widened)** | Add A8 (chips), A9 (autosize), A10 and A11 for the assistant | `workbench/assistant/` | S | After `feat/dialog-api` |
| **WP-W5 Dialog contents** | A12: Material fields in the shared dialogs that `feat/dialog-api` rewrites (MCP key, channel secret, and any other dialog body with plain inputs) | `shell/*/dialog/` | S | After `feat/dialog-api` |
| **WP-W6 Dock menus** | A13 dock menu → `cdkMenu`; A14 switcher → `ActiveDescendantKeyManager`; dock icons through the registry (§6) | `workbench/dock/src/{dock-host,menu-placement,switcher-overlay,dock-glyph,group-actions,panel-tab}.ts` | S/M | After `feat/dock-translucent-redock` |
| **App packages (WP-DB, WP-D, WP-S, WP-PW, WP-PC)** | Add §3 and §4 to each; adopt WP-8 and WP-9 | as in the audit | see §5 | as in the audit |

## 9. Decisions for the user

- **D-A (clipboard).** Should the shared `copyText` fall back to the CDK `Clipboard` (`execCommand('copy')`, which is
  deprecated in browsers but still the CDK's tested path)? Or should it stay async-only and just become the one
  mechanism? I recommend the fallback plus the conformance check. The consolidation (X1) is worth doing either way.
- **D-B (`<details>`).** Should section disclosures become `mat-expansion-panel` while one-line "technical details"
  stay native `<details>`? Or should everything become Material (rule 47 read strictly)? I recommend the split. There
  are 28 `<details>` in total.
- **D-C (Spotter stepper timing).** Should SP1 (`mat-stepper`) land in Spotter 1.0 or in 1.0.x? I recommend 1.0.x:
  it is a visible change on the main editor flow and needs its own review (rule 22), and 1.0 already carries WP-S.

## Sources

- Code: `origin/main` 78d5b0f3c, read with `git archive`/grep on 2026-10-28; the in-flight branches with
  `git diff origin/main...<branch>`.
- CDK sources read: `@angular/cdk` 22.2.4 `fesm2022/clipboard.mjs` (`PendingCopy`: hidden textarea,
  `execCommand('copy')`, focus restored), `fesm2022/_directionality-chunk.mjs` (reads `body.dir`/`documentElement.dir`),
  `fesm2022/text-field.mjs` (`CdkTextareaAutosize`), `fesm2022/_a11y-module-chunk.mjs` (`LiveAnnouncer`).
- Component docs (not fetched in this sweep; the same Markdown sources as the audit's list), *unverified* here:
  https://github.com/angular/components/blob/main/src/cdk/clipboard/clipboard.md,
  https://github.com/angular/components/blob/main/src/cdk/a11y/a11y.md,
  https://github.com/angular/components/blob/main/src/cdk/text-field/text-field.md,
  https://github.com/angular/components/blob/main/src/material/stepper/stepper.md,
  https://github.com/angular/components/blob/main/src/material/chips/chips.md,
  https://github.com/angular/components/blob/main/src/material/expansion/expansion.md,
  https://github.com/angular/components/blob/main/src/material/autocomplete/autocomplete.md,
  https://github.com/angular/components/blob/main/src/material/progress-spinner/progress-spinner.md,
  https://github.com/angular/components/blob/main/src/material/paginator/paginator.md,
  https://github.com/angular/components/blob/main/src/material/icon/icon.md.
