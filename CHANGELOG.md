# Changelog

Conventional Commits; one semver tag for the whole repo.

## Unreleased

### Quit in the menu bar: `QUIT_CONTRIBUTIONS`, and clicks with no window open (`paddock-menu`, `paddock-shell`, `@decryp7/paddock-ui/shell`)
- Fix: no shell app had Quit in its menu bar: nothing contributed `pd.app.quit` (only the constant existed), so Cmd+Q did nothing. `@decryp7/paddock-ui/shell/menu` now exports `QUIT_CONTRIBUTIONS` (the command `pd.app.quit`, `Mod+Q`, last in the app menu, group `quit`) and `provideShellQuit(invoke)`; the command calls the new `ShellCore.quitApp()` (the `APP_QUIT` token, the shell's `app_quit` path). Register both in the main window beside `SETTINGS_CONTRIBUTIONS`; Dyno and the shell demo do. An app that contributes its own `pd.app.quit` (Pitwall's branch) drops it and registers these (one command id, one definition).
- `ShellCore` has a new member `quitApp()`; the UI of a shell app that implements `ShellCore` by hand adds it.
- Fix: with the last window closed to the tray (destroyed), the menu bar still shows the last model but a click had no target window and was dropped. `paddock-menu` now hands a click on a shown item to a host-side handler when no window is the target (`NativeMenu::on_no_window`, `on_no_window_by_default`, `route::runs_without_window`). The shell installs its policy unless the app set a handler: Quit runs the shell's quit (as the `app_quit` command; `Builder::menu_quit_without_window` replaces it for an app with its own quit path), any other item brings the main window back as a Dock click does (the click itself is not forwarded: the new window is not listening yet).

### `paddock-jobs`: `WalkOptions::with_modified`
- `WalkOptions` has a new field `with_modified` (default false) and `WalkEntry` a new field `modified: Option<SystemTime>`: the walk reads a file's modification time from the same `stat` as its size, so a caller needs no second look at each file (Dyno's file finder). Code that builds a `WalkEntry` by hand adds `modified: None`.

### `paddock-jobs`: the `wire` module
- New `paddock_jobs::wire` (`WireSnapshot`, `WireState`, `WirePhase`): the postcard-safe form of a `JobSnapshot` for apps whose jobs run in an agent process and are relayed to the UI as `paddock://jobs` events (Dyno is the first). It converts both ways without loss.
### `pd-sunburst` in `@decryp7/paddock-ui/chart` (docs/plans/pd-sunburst.md)
- New `PdSunburst` (`pd-sunburst`) and `PdSunburstCentre`, a drillable ring chart for hierarchies (first consumer: Dyno's disk map) and the pure
  `sunburstLayout`: SVG with tree semantics (`treeitem`, `aria-level`, `aria-setsize`, `aria-posinset`, `aria-expanded`), colour by branch over
  the ring-ordered chart tokens, hatched hidden space and a dashed "not counted yet" ghost, folding by angle and count with a segment cap,
  a live mode that coalesces updates to four per second, the tree keyboard model, and loading, empty and error states. Exports are one
  declared line each in `chart/public-api.ts`; a gallery section shows every state. Also exported: `branchSlots`, `slotColour` and `PdArc` (a list beside the chart uses the chart's own colours), and `pd-sunburst` has a `highlight` input. `@decryp7/paddock-e2e` gains `clickShape`, `hoverShape` and `pointInShape` (real pointer events on a point inside an SVG shape). The tween between centres is a 100 to 180 ms cross-fade
  (`tweenLayouts` is not built yet); `pd-table` tree rows (spec section 7) are a separate change.

### Dyno: the disk map screen on the shell
- New nav entry `Disk map` (`/disk-map`): scan a volume or folder as a job with live phase-labelled progress and Cancel, ring chart (`pd-sunburst`) beside a list or a `mat-table`, drill-down, capacity strip with hidden space, Full Disk Access and "Unavailable" states with a reason, Settings limits (items kept per scan, large file size).
- The engine says why each unreadable folder was refused (`DiskNode.unreadableReason`). On macOS "operation not permitted" (EPERM) is `needsFullDiskAccess` while Full Disk Access is missing and `protected` ("macOS protects this folder") when it is already granted; "permission denied" (EACCES) is `needsAdmin`, except a privacy-protected folder of the user's own home while Full Disk Access is missing (a path hint). Windows and Linux: either refusal is `needsAdmin`; any other error is `other`. The Full Disk Access button shows only when a refused folder has that reason. `PROTOCOL_VERSION` is 2 for the new field (jobs-1.0 bumps to 2 as well: one bump covers both while unreleased).

### `providePagesPalette`: the command palette on the pages frame
- `ShellPages.go(id)` is new: like `show` but resolves once the navigation is done. The pages frame treats focus inside the command palette as "move to the page" after a navigation.
- `<pd-command-palette />` keeps the keys typed between the shortcut and the input getting the focus (printable keys and Backspace edit the query, Escape closes, Enter waits for the input and then accepts the row of what was typed), and draws the listbox only while it has options (the empty text is said in the live status).
- New `providePagesPalette(options?)` (with `goToPages?: false | { prefix }`), `PagesPaletteOptions` and `GO_TO_PAGE_PROVIDER` in `@decryp7/paddock-ui/workbench/palette`: the shared palette (`PaletteStore`, `<pd-command-palette />`, Mod+Shift+P for the commands, Mod+P for "Go to…") for a window on the pages frame, which has no workbench ports. It routes `openPalette` to the store and contributes a plain-text provider that lists the pages the window offers now (`ShellPages.visible`, sidebar order, ranked by the typed text) and shows the one accepted, then moves the focus to the page (`#pd-main`). Put `<pd-command-palette />` beside `<pd-shell-frame />`.
- Dyno uses it: Mod+P goes to any page of the sidebar; Mod+Shift+P lists the shell's commands and Dyno's own (the pages' buttons: Refresh history, Read the system logs now, Export for Debrief, Check now, Run again, Export Markdown, Save bundle, Add rule, and Capture the last ten minutes). A command shows its page first, then runs.

### `@decryp7/paddock-e2e`: a truncation must say its full text
- `isSilentlyClipped` now fails an ellipsis unless the element says its full text (`isAcceptedEllipsis`: a non-empty `data-full-text`, title, aria-label or Material tooltip message) and is not in a button, link, tab or `.num`. New exports: `isAcceptedEllipsis`, `unacceptedTextClips`, `textCutOffWithoutTooltip`, `ELLIPSIS_NEVER_WITHIN`, `ELLIPSIS_NEVER_SELF`; the checks read facts in the page and decide in Node, with no `new Function`.
- `pd-table`'s truncating text cell sets `data-full-text`.

### Tab nav bars: links size to their label
- `@decryp7/paddock-ui` scss (`_material.scss`): `mat-tab-link` in a `mat-tab-nav-bar` has no 160 px minimum and `--pd-space-3` side padding, so eight tabs fit a 900 px window. Tab rows in apps that used the minimum get narrower tabs.
### `pd-table`: one tooltip for a row's actions (`PdActionTip`), and the focus follows a reordered row
- `PdActionTip` (`pdActionTip`, from `@decryp7/paddock-ui/table`; moved from Dyno's `dyActionTip`) puts one tooltip left of a cluster of icon buttons, named after the button the pointer or the keyboard is on. pd-table's row actions use it, so a tooltip never covers the next row's actions.
- A focused row keeps the focus when the app puts the rows in another order (live values): the table asks for the row again by its key once the browser dropped the focus, whether or not the element is still there. A click outside the grid forgets the row.

### `@decryp7/paddock-e2e`: `expectNoSilentClipping`
- `textClippedWithoutEllipsis(page)`, `expectNoSilentClipping(page)` and the pure `isSilentlyClipped(probe)`: fail when an element cuts its own text (hidden overflow, narrower than the text) without `text-overflow: ellipsis`.

### Dyno: pages no longer clip at 900 and 1024 px
- Network's Listening table keeps the exposure badge whole at 900 px; truncated names in the dashboard feed, widgets and the Network service column show their full text in a tooltip. The layout e2e now also measures every state (detail panels, tabs, second views), every Settings page (with the advanced fields) and every dialog, and accepts an ellipsis only on the clipping element when it has a tooltip or a label and is not a control, a tab or a number.
- Benchmarks, Health, Extras (tabs and Maintenance) and Network wrap or shrink to the page instead of being cut by `.pd-page`'s `overflow-x: hidden`; the primary "Run benchmarks" button is always whole. `e2e/layout.spec.ts` now fails on any page content wider than the page, and on text cut without an ellipsis, at 900, 1024 and 1280 px, for every page and every Settings page.


### `providePaddockMaterial`: tooltips wait 500 ms
- `providePaddockMaterial()` provides `MAT_TOOLTIP_DEFAULT_OPTIONS` with `showDelay: 500` (`PADDOCK_TOOLTIP_OPTIONS`), the design guide's delay for the tooltip every icon-only button carries. Every app that provides it gets the delay without setting it per tooltip.

### `pd-table` uses `PdContextMenuKeys`
- The grid's own Shift+F10 / Menu key handler is gone: pd-table's rows use the shared `isContextMenuKey` (the same keys, one definition).
- Removed: `isMenuKey` from `@decryp7/paddock-ui/table`; use `isContextMenuKey` from `@decryp7/paddock-ui/material`.

### `PdContextMenuKeys`: a row's context menu from the keyboard
- New directive `pdContextMenuKeys` (and `isContextMenuKey`) in `@decryp7/paddock-ui/material` (with the other Material behaviour; not in the root entry): Shift+F10 or the Menu key on a focused host fires a `contextmenu` event there, for `matContextMenuTriggerFor` or any listener (macOS sends none for those keys). It works like pd-table's own row handler, which will use it in a follow-up.

### `@decryp7/paddock-ui/schedule` exports its time-field building blocks
- New exports: `PdKeepTimeOnEscape` (the `pdKeepTime` directive: Escape in a closed time field keeps the time), `TIME_PROVIDERS`, `timeToDate`, `dateToTime`, `timeOptions` and `TimeProblems`, so an app can put a `mat-timepicker` in its own form with the editors' behaviour (24-hour times, the half-hour list, a problem shown once the field was left). Dyno's `dy-time-field` is the first user.
### The shell owns the start (ADR 0029; shared API, breaking)
- `paddock-shell`: `Builder::tray(f)`, `Builder::launch(LaunchArgs)` and `Builder::add_to(tauri::Builder)` open the tray
  and the main window from Tauri's own `setup`, after every plugin is set up (a window built inside a plugin hook
  deadlocked the main thread: Spotter started with no window). A start that cannot open its window is logged and exits
  with code 1. `Builder::setup` no longer opens windows. New `paddock_shell::testing::start` (feature `mock-runtime`):
  the start test every app runs. The shell's plugin hooks are marked; a Dock click with no window and a file opened from
  Finder open their window after the hook.
- `paddock-shell`: `logging::install` (and `install_in`): the app's `paddock-log` in its folders with the level from
  Settings; every app on the shell calls it first in `run`. A shell built with `build()` that has a tray or a launch
  fails the start ("tray/launch need Builder::add_to"); `testing::start` names a start that panicked.
- `paddock-commands`: `plugin_hook` (`plugin_hook_scope`, `in_plugin_hook`, `INSIDE_PLUGIN_HOOK`).
- `paddock-window`: `after_plugin_hook` (window work after the hook, in order), `exit_with`, `HeadlessTray`; windows and
  the tray popover refuse to be built inside a plugin hook with the new `WindowErrorCode` `insidePluginHook`.
- `paddock-dialogs`, `paddock-panels`: dialogs, pop-outs and the drag ghost refuse to be built inside a plugin hook, with
  the new `DialogErrorCode` / `PanelErrorCode` `insidePluginHook`.
- `paddock-workbench`: its plugin hooks are marked.
- Spotter, the shell demo and the workbench demo use the new start; conformance C50 checks every app on the shell.

### `pd-table`: rows an app orders itself, and trees of rows
- `PdTable.focusRow(key)` focuses a row once it is drawn (after an app closed a panel opened from it). `PdColumn.sortStart` (`asc` default, `desc` for columns of amounts) sets the direction of a header's first click. The body tracks
  rows by key and columns, so a column change never leaves recycled row views with the old cells. `tree` implies the app's order
  and no filter field, and is reported with `console.error` in dev mode together with `groupBy`. The indent is `--pd-tree-indent`. Group summaries are ellipsised and end-aligned cells clip at their end (`safe flex-end`); the
  count says "1 row".
- `order="source"` shows the rows in the order the app gives them (the table only shows the sort and reports it through `sort`),
  for a list the engine or a tree orders (processes under their parent, sorted within each level).
- `tree` (`PdRowTree`: `level`, `expanded`, optional `spans`) makes the rows a tree grid: `aria-level`, `aria-expanded`,
  places among siblings, an indent per level on the first cell, and a spanning heading row. Right unfolds a folded row and on an
  unfolded one moves to its first child, Left folds an unfolded row or goes up to the parent (`rowToggle` asks the app to fold; it
  owns the fold state, so it can keep it). Space opens a row like Enter. `treePlaces` and `parentIndex` are exported. `sortClearable="false"` keeps a header cycling between ascending and descending only. The row that has the focus keeps it
  when the columns change (a side panel opened and the table narrowed), once the rows are drawn again.

### The dialog frame's filled button uses the page primary's colours
- `.pd-frame-btn--filled` takes `--pd-accent-fill` with `--pd-on-accent`, the same as `.pd-btn--primary` and Material's filled button, so a dialog's primary and a page's primary are one colour in both schemes (`--pd-accent-control` is for toggles and checkmarks).

### Rust developer and CI tool, phase R0 (ADR 0022)
- New shared crates `paddock-slots` (numbered job slots over OS file locks, with a holder that always ends: job folder gone,
  release, or `--max-hold-secs`), `paddock-repo-files` (the walk, its caps and a bounded-thread file hash) and
  `paddock-repo-graph` (the project graph from Cargo and npm, the `History` port and its git adapter).
- New `tools/paddock-dev` (its own workspace and lockfile) with `graph affected`, `ci slot acquire|release|status|hold` and
  `self hash`, and the `./dev` / `dev.cmd` wrappers that build the tool or take it from `~/.paddock-dev/bin` (five newest kept).
  The binary's name is a hash of the inputs in `inputs.txt`, the compiler, the build-affecting `CARGO_*` and `RUSTFLAGS`
  variables and the text of every cargo config it reads (all `.cargo` folders up to the root, `$CARGO_HOME`, the tool's
  `include` files). `./dev` says how to add `tools/` to a sparse checkout.
- `ci.yml` gets a `paddock-dev` leg on Linux, Windows and macOS (fmt, clippy, nextest `ci` profile, cargo deny, lockfile check).

### `pd-table` on Angular Material's table (ADR 0027, decision 5)
- `pd-table` is built on `mat-table`: a header-only table above the CDK virtual scroll viewport and the body table inside it,
  one `role="grid"`, so the header never moves with the rows (also on fast scrolls, jumps and End). `matSort` and
  `mat-sort-header` sort (`aria-sort`; Material's arrow, hidden at rest), `mat-menu` gives the column and row menus (the
  "..." button, right-click and Shift+F10), `matTooltip` the full text of a cut cell and the buttons' names, `matIconButton`
  the row actions (24 px, the family's hover fill in a 6 px shape). Group rows, middle truncation, `hideBelow`, the row
  heights and the grid keyboard stay the family's own. The public API is unchanged.
- One tab stop for the grid (the active row): the sort headers and the row's buttons are not tab stops. Up from the first row
  reaches the sort headers and the arrows move along them; Right reaches the row's enabled buttons, Left and Escape go
  back, Up and Down on a button go to the row above or below; Shift+F10 or the Menu key opens the row's menu. A grouped
  table is a tree grid: group rows say whether they are expanded, Right expands a collapsed group and on an expanded one
  moves to its first row, Left collapses an expanded group, Left on a row moves to its group row, and the rows say their
  place (`aria-posinset`/`aria-setsize`: groups among groups, rows among their group's count).
  The keyboard focus follows End and Home in a virtual table once the row is drawn, and a pending focus is dropped when the
  lines change or the focus leaves the grid. A row stays marked while its menu is open. A focused button shows Material's
  focus indicator alone; a cut cell's tooltip (and its description) exists only while the pointer is on it; the loading
  placeholders are hidden from assistive technology.
- Every body is drawn in the viewport: above 100 lines only the rows in view; below, or with `virtual="false"`, every row,
  a frame after a change.
- A sortable header's accessible name is its label (the button inside the column header); "Sort by …" is its description.
- `@decryp7/paddock-e2e`: `headerTrack` samples a grid's header frame by frame through a fast scroll, a jump and End.
- Popup menus built with `mat-menu` keep the family's item size and keyboard ring (`_controls.scss`).

### Sticky table headers stay above their rows (ADR 0027)
- New tokens `--pd-z-table-corner` (4), `--pd-z-table-header` (3), `--pd-z-table-sticky-column` (2) and
  `--pd-z-table-row-action` (1): the layers inside a table, header on top, a sticky column's header cell above both. `pd-table`'s grid is its own stacking context and its header uses `--pd-z-table-header`;
  `pd-view-table`'s header and `pd-view-matrix`'s sticky row headings and the shell's diagnostics tables use the ladder.
- The apps' tables whose sticky header had no z-index (Debrief: alerts, tail status, query results, SQL console, store pane,
  markers, keyboard settings, runs, dashboard tiles, parser editor, format wizard; Dyno: Charted counters) set one, so
  muted or translucent rows and hover actions no longer paint over the header; Pitwall's model library keeps its sticky
  action column (2) below the header (3).
- Conformance C43: a header or footer row that sticks (top, bottom, `inset-block-*`, the `inset` shorthands) needs a
  z-index of at least 3, a sticky column's header cell at least 4, and a sticky column must sit below the app's lowest
  header (`/* conformance: not-a-header */` for sticky panels); run on Paddock's CI with the other repo-wide checks. The
  headers at 1 (Dyno, Spotter, Pitcrew, the workbench explorer) and the sticky action bars (Dyno cleanup, Debrief's setup
  wizards and schedules, Spotter's editor) are at the header level.
- `@decryp7/paddock-e2e`: `headerCovers`, `expectHeaderOnTop` and `isOpaqueColor` (scroll in steps and a jump of more than
  two viewports, `elementFromPoint` across the header row in the first frame and again once a row reaches the header with
  the row under it hovered, opaque background); the gallery runs it over `pd-table` in both themes, Chromium and WebKit.

### Frame navigation on Material and the CDK (WP-4 of the Material migration, rule 47)
- The pages sidebar is a `mat-nav-list`; the CDK `FocusKeyManager` moves focus with Up, Down, Home and End (wrapping). Tab still reaches every link.
- Count badges (sidebar and activity bar) are `matBadge` (aria-hidden: the count stays in the link's name). In the sidebar the count is at the row's end (on the icon in the rail); a count without a badge label stays in the link's name. The two page groups are `role="list"` of list items, not extra navigation landmarks. Tones set `--mat-badge-background-color`.
- Tooltips are `matTooltip`, not `title`: the rail's page names, the activity bar and its More menu, the status bar's More button, the part Retry button, the path text's Copy button. `pd-path-text` shows the full value only when the text on screen is shorter than it; the title bar no longer repeats the app name as a native title.
- The Copy menu (`PdCopyMenuHost`) is a CDK context menu (`cdkContextMenuTrigger` and `cdkMenu`), opened at the pointer by `PdCopyMenu` as before. The `.pd-badge`/`.pd-act-badge` classes and `[data-testid="page-badge"]` are gone: the e2e page object `badges` finds `.mat-badge-content`.


### Shell toasts keep a page-footer safe area
- New token `--pd-page-footer-safe` (56 px). `pd-notification-host` now sits above the status bar plus this safe area, so a toast no
  longer covers a page footer's primary action (a wizard's Next); the status-bar fallback is 24 px like the token.



### Updates and the undo toast on Material (WP-W2, WP-W3, rule 47)
- Updates (the About dialog itself did not change): its checkboxes are `mat-checkbox` and the download progress is `mat-progress-bar` (a
  phase without a total is indeterminate; with reduced motion it is a full, still bar, not a frozen partial one). The dialog needs
  `providePaddockMaterial()` and `css/material.css`, in the main window and in the dialog windows. The shell demo's `?hold` keeps a
  download running and `window.sdUpdatesDownload.progress()` plays the engine's job events (e2e and screenshots).
- Material checkboxes line up with the form column: one shared rule in `_material.scss` pulls the box back over the touch-target padding (not in table cells, which keep Material's centring).
- Breaking (shell/undo): `pd-undo-toast` draws nothing itself now and opens a `MatSnackBar` ("Done: ...", Undo action, close button;
  polite live region, no focus, the store's clock keeps the timeout, reduced motion follows Material and the global rule). Place it once
  in the shell and drop any wrapper that positioned it. MatSnackBar shows one at a time: another snack bar replaces the undo toast and the
  store then drops its toast (the edit stays undoable).
- One toast area: new `ToastArea` (`@decryp7/paddock-ui/shell`). The undo snack bar is bottom right, 360 px, on the notification host's
  baseline (status bar + `--pd-page-footer-safe`) and styled like a notification (surface-3, hairline, shadow; every Material snack bar looks like
  this now, with the action in the accent colour). It reports its height and `pd-notification-host` lifts itself above it, so the two never
  overlap at any width (under 600 px the snack bar stays at the right, with the host's 8 px gutter up to 480 px). The apps' shells need `providePaddockMaterial()` and `css/material.css` (the shell demo has both).

### One shared Material theme (WP-0 of the Material migration, rule 47)
- `css/material.css` (`scss/entry-material.scss`) is now the whole Material look: one Material 3 `mat.theme` (density -3) whose system
  and component tokens all resolve to `--pd-*`, so theme, accent, contrast and density follow our tokens and an app carries no
  Material theme of its own. New bridge tokens: table, sort header, paginator, tree, button toggle, badge, list, expansion, timepicker,
  slider, chips, tabs, progress bar and every `matButton` variant (filled = primary, outlined = secondary with the surface-2 fill, text,
  tonal, elevated; danger is the class `pd-button-danger` on any of them; no ripple).
- New secondary entry `@decryp7/paddock-ui/material`: `providePaddockMaterial()` makes every `mat-form-field` `appearance="outline"` with
  `floatLabel="always"` and turns ripples off. `@angular/material` is an optional peer dependency (`^22.2.4`) of `@decryp7/paddock-ui`.
- `npm run test:tokens` measures the contrast of every bridged Material component in light and dark for all accents (text 4.5:1,
  icons, borders and indicators 3:1). It found the snack bar action (accent on the inverse surface, 1.9:1 to 2.4:1): it now takes the
  surface ink. `npm run build:css` resolves `@angular/material` from the repo's `node_modules`.
- Review fixes: keyboard focus is Material's strong indicator in `--pd-focus-ring` (2 px, 2 px outside; no more hidden rings), the shape
  rules win over Material's own CSS (28 px menu items and options, 4 px panel padding, popover radius), form fields get a 12 px floated
  label, a centred input and room for the suffix icons; type scale without tracking and with the family's line heights, family font
  stack; muted hints; segmented-look button toggle; `@angular/material` and `@angular/cdk` are 22.2.4 in every workspace.
- Round 2: the floated label now renders at 12 px (Material scales it by 0.75, so the size is asked for / 0.75; the e2e measures the rendered
  width), the field's own 2 px outline in `--pd-focus-ring` is the keyboard ring (no ring over the floated label), calendar month names
  are sentence case (`monthLabel` in `providePaddockMaterial`, the heading's upper-casing undone in CSS), icon buttons and the paginator select
  follow the density, and `@angular/material` / `@angular/cdk` are hoisted to one root copy again in `package-lock.json`.
- Gallery: a "Material controls" section shows every bridged component (`e2e/material.spec.ts`: axe, token checks, real overlays).
- State layers: bridge-only CSS (no `mat.theme`) left `--mat-sys-*-state-layer-opacity` undefined, so the opacity computed to 1 and every
  hovered or focused Material icon button drew a solid disc. The bridge now sets the hover, focus and pressed opacities from the new
  public tokens `--pd-hover-alpha` / `--pd-pressed-alpha` (0.05 / 0.09; `--pd-hover` and `--pd-pressed` are built from them; Material's
  own defaults are 0.08 / 0.12 / 0.12) and the icon button's ink to `--pd-text`. The material e2e checks every icon button's rendered state
  layer (alpha and pixels) in light and dark, and a token test compiles the bridge alone. `e2e/material.spec.ts` type-checks again.

### Form controls on Material (WP-1 of the Material migration, rule 47)
- `pd-select` is a `mat-select` in an outline `mat-form-field` (keys, typeahead and ARIA are Material's; closed, the arrow keys now
  choose the next option and Enter or Space opens the list; the chosen option has its check mark). New optional input `label` (the label
  in the outline's notch). A `<label for>` that names it through `inputId` is read from the document: its id becomes `aria-labelledby`,
  and a label with no id becomes the select's `aria-label` text (no id is written onto a label the select does not own).
- `pd-field` keeps its inputs and `pdFieldControl`, and is an outline `mat-form-field` with the label always floated in the notch
  (design rule 11, D1), `mat-hint` and `mat-error`. Whatever look the marked control brings (`pd-input`, a dialog's own input style) is
  neutralised by the `pdFieldControl` marker. A native `<select pdFieldControl>` gets Material's arrow and a textarea its Material class.
  A field with no control to outline (a picker made of a button) keeps the classic layout: the label above, the content in a labelled
  group, the hint or error below. New inputs: `error` (an error the host knows without a form state) and `suffix` (a template drawn at the
  end of the outline, for a picker's toggle). `PdField.describedBy` is now a signal fed by Material.
- `pd-segmented-control` is a single `mat-button-toggle-group`: Home and End no longer jump (Material has no such key) and the
  `nextEnabled` helper is removed from the public API.
- `pd-day-picker` is a multiple `mat-button-toggle-group`; the schedule editor's mode and interval presets are button toggles too.
- The schedule and pause editors use `mat-timepicker` instead of `type="time"`, with a toggle button, and show and read times in
  **24 hours** (`17:45`, like the engine's `HH:MM` and the preview), a visible change from the platform's time field. The engine still gets
  `"HH:MM"`, converted at the edge on a fixed reference day (`time-value.ts`, tested across midnight, DST and half-hour zones). Text the
  picker cannot read keeps the last good time and shows "Enter a time such as 09:00." once the field is left; any change of the schedule
  from outside the field clears it. Buttons are `matButton`/`matIconButton`, tooltips `matTooltip`.
- Escape closes an open `pd-select` or time list and stops there, so a dialog around it stays open.

### Shell pages' controls are Angular Material (WP-2 of the Material migration, rule 47)
- Settings window and fields, AI access (MCP), Notification channels, Set-up, Storage, Knowledge packs, Background service, Report a
  problem and Language: the hand-made switches are `mat-slide-toggle`, checkboxes `mat-checkbox`, the language radio group
  `mat-radio-group`, native `<progress>` `mat-progress-bar`, the "Needs restart" and "Advanced" badges static `mat-chip`s, native
  buttons `matButton` (filled = primary, outlined = secondary, text for quiet actions) and `matIconButton`, and `title` tooltips
  `matTooltip`. Public APIs and `data-testid`s are unchanged, but a toggle's test id is now on the Material host: tests reach the control
  with `getByRole('switch' | 'checkbox')` inside it. A button at a limit uses `disabledInteractive` (focusable, `aria-disabled`).
- `.pd-switch` stays in `_controls.scss` until its last users (the view form and the gallery) have moved.


### Shell: About, health chip, notices and jobs popover fixes
- About, Updates and What's new are styled again: `ABOUT_STYLES` selected the pre-`pd-` class names (b88a387a1). A new spec
  (`shell/testing/src/template-classes.spec.ts`) checks that every class a shell template uses has a selector in its styles, so the
  rename drift cannot come back unnoticed (it also covers host classes and `src/lib`, and a class binding cannot satisfy itself).
- The service health chip is sentence-cased for labels such as "your sources" (the value after the colon stays lower case, a rule
  now in the design guide §10) and says what is wrong from the cause code Rust emits (`CAUSE_WORDS`: "not installed", "needs
  approval", "needs pairing" ...). `cause-codes.spec.ts` reads the Rust sources and fails for a cause code without words, a
  non-literal `.code(` that is not listed, or a word for a code nothing emits. No network cause code exists on the Rust side yet.
- Notification centre titles wrap (two lines, full text in the tooltip) instead of cutting at one line, and the hover buttons no longer reserve room.
- Jobs: `pd-job` has a `flat` input (a row without border or background) and `pd-jobs-list` a `flat` input, which the status popover
  uses (no card in a card); a phase named like the job's title is not said twice ("Downloading" in the card and the status item).

### Shell toasts keep a page-footer safe area
- New token `--pd-page-footer-safe` (56 px). `pd-notification-host` now sits above the status bar plus this safe area, so a toast no
  longer covers a page footer's primary action (a wizard's Next); the status-bar fallback is 24 px like the token.

### Shared dialog API, round 2 of the reviews (frame)

- A refused answer's alert is measured with its margins, so the refitted window keeps the body's bottom padding and does
  not scroll (e2e: body `scrollHeight <= clientHeight` in the rejected state).
- `layoutActions` returns the decision's `tone`: `danger` (and Cancel focused) only when a destructive answer is the sole
  affirmative; a primary beside a destructive (the save prompt) is `info` and focuses the primary.
- Busy: the pressed button keeps its full colour with its spinner and may say `busyLabel` ("Creating…"); the inert body
  dims. Rejected: the alert has an error icon and no border; Try again shows its focus ring after the script moved focus.
- Contents: pd.mcp.key reads the key count when it opens (its contract adds `mcp_get`) and shows the key limit as a
  validation message with Create key disabled; Create key says "Creating…" and the channel secret's Save "Saving…".
  pd.storage.confirm's button repeats its heading ("Delete logs?" / "Delete logs"). sd.inspector says "Running".
  Report a problem breaks long file names without hyphens. Set-up says "Get Pitwall" when Pitwall is not installed. The
  channel secret's Show button is as tall as its input. `SuEmptyState`'s description uses `text-wrap: pretty` (no widows).
- Round 3 polish: Report a problem keeps a file name on one line with an ellipsis (the full name is its tooltip); the
  Settings window's footer says "Changes are saved as you make them." until a change has its own notice, and its
  content ends where Close does; a busy dialog dims only its controls, not its heading and text; the demo save prompt's
  refusal is one a retry can fix.
- The dialog harness takes `interact` (what the user does once a dialog booted, by id): the shell demo's boot follows
  About's releases link, so `files_open_url` is sent and the unexercised count stays at 32.

### Shared dialog API: About opens its releases page in the system browser

- `pd.about` may call `files_open_url` (`paddock-updater` depends on `paddock-files` for the marker): the releases link asks
  the engine to open the page, since a webview window opens no links itself, and says why when it was refused.
  `AboutBackend.openUrl` (Tauri: `files_open_url`; memory: `openedUrls`, and `opener` for a browser build).

### Shared dialog API, review fixes (Rust)

- `Measure` has an optional `viewportHeight` (the page's `window.innerHeight`, wire type `DialogMeasureWire`). Rust adds
  the window's inner height less it (clamped to 0..=400 logical px) to the content and minimum content heights, so a
  webview shorter than its window no longer cuts off the footer.
- A modal's parent is disabled and re-enabled under the registry lock, on the main thread, and only shown modals hold
  it: a deadline that fires after a close, or a pending modal, can no longer leave the parent disabled.
- **Fixed.** The deadline/close race that could leave a parent disabled with no dialog: the registry records only the
  desired state (a parent is disabled iff a shown modal of it is open) and one queued main-thread task reads the latest
  of it and applies it (`Dialogs::parent_change`); no window API is called under the registry lock, and a close whose
  task runs before the show's still ends with the parent enabled. `Dialogs::mark_shown_with` and `remove_with` are gone
  (`mark_shown` returns the entry).
- The viewport delta is taken once, at the first fit (`dialog_ready`), kept on the registry entry (`viewport_delta`) and
  reused by later fits, so a later `dialog_fit` does not recompute it from a resized window.
- `Measure` has an optional `devicePixelRatio` (finite, 0.25..=8; `DialogMeasureWire`, `DialogMeasure`): the page reports
  CSS px, and the fit converts content, minimum content and viewport height to logical px by the webview zoom,
  `devicePixelRatio / scale_factor` (1 when either is missing, `zoom_of`, `Measure::logical`).
- `DialogInfo` has `pending` (the window is not on screen yet): an existing window that is still pending no longer
  unblocks its opener before its `paddock://dialog-shown` event (`WindowDialogBackend`); a closed window's early-shown
  mark is forgotten.
- Conformance C45 (C40 to C44 are taken by ADR 0024 and other branches): `__command(` and `__prefix(` are called nowhere outside `crates/paddock-commands` (test code and
  doctests excepted).
- The real-window probe boots the built shell-demo's real dialog frame (`PADDOCK_PROBE_DIST`) with a visible host and a
  modal dialog, asserts the dialog window is hidden until its `dialog_ready` arrives, and reports from a synchronous read
  and a timer (no animation frames). Its job `dialog-window-probe` (`nightly.yml`) is off by default and gates nothing:
  it runs on a manual run or with the variable `PADDOCK_REAL_WINDOW_PROBE=true`, because it opens real windows (ER-14)
  and the Mac host's displays sleep at night. Built with `--features tauri`, not `--all-features`.
- A destroyed window, and a host whose dialogs close, are forgotten in the applied parent state (`Dialogs::forget_applied`);
  the applied state is recorded only after `set_enabled` returned Ok (`Dialogs::applied`), so a failed call is retried.
- The webview zoom is kept on the dialog entry from `dialog_ready` and changes only when the page's `devicePixelRatio`
  changes at an unchanged scale factor (`zoom_after`); a monitor move keeps it, so a fit is not off by
  `dpr_old / scale_new`.

- **Breaking.** `paddock_commands::DialogCallable` is a struct with a private field (its old variants are now
  `DialogCallableKind`, read with `kind()`): a contract can only name a `dialog_callable::<name>` marker, `prefix` or
  `plugin`; `DialogCallable::Command("x")` no longer compiles (compile_fail doctest).

- `DialogRequest` has an optional `theme` (`'light' | 'dark'`, the opener's `data-theme`, else its system scheme; sent by
  the window backend): a dialog window's background before its page paints is that theme's `--pd-surface-2`, a test
  compares Rust's two values with `_tokens.scss`. Without it Rust falls back to the parent's OS theme.
- Every dialog page gets `<html data-platform="macos|windows|linux">` from an initialization script, so the footer's
  button order follows the shell's platform, not the user agent.
- New event `paddock://dialog-shown` (`DIALOG_EVENTS.shown`, payload `DialogShown { label, dialogId }`) when the window is
  shown. `DialogSession` has an optional `shown` promise: a windowed modal blocks its parent (the scrim over a macOS
  parent) once its window is on screen, not when it is opened; one closed before that never blocks. `FakeDialogIo.shown()`
  plays the event.

- `scripts/affected.mjs` maps a change to a generated file under `ui/**/generated/` (`.ts`, `.json`) to the crates that
  name it (all crates when none does), so a stale generated file fails branch CI, not the merge.

- `paddock-dialogs/tests/show_probe.rs`: the ADR 0026 section 6 real-window probe as a test target (`harness = false`,
  runs only with `PADDOCK_REAL_WINDOWS=1`; `tauri` gets the `wry` feature in this crate's dev-dependencies).

### Shared dialog API, part 7: dialog content polish after the visual review

- `pd.about` shows its facts in a two-column label and value grid, says the system in words ("macOS (Apple silicon)", "Web browser") and links the releases page; the shared About, Updates and What's New styles use the `pd-` class names the templates use (buttons are 8 px apart again).
- `pd.updates`, `pd.setup` and `pd.wb.dock.layout-manage` are modeless forms (medium, fitted to their content) instead of fixed-size tools; their generated contracts and the shell demo's manifest changed. Updates keeps the automatic-check box, the channel and the install-on-quit box on separate rows and says "Not checked yet" once. Set-up has one filled button (the first step to do, or Repair) and names the missing Pitwall once. Manage layouts has a designed empty state with a Save current layout action (also in the footer once layouts exist), which closes with `{ save: true }` and runs the dock's Save layout flow (`LayoutManageResult.save`).
- `sd.inspector` is a modeless form, shows "Queued" once and the uptime as "12 min 34 s". Report a problem keeps its Size column on one line and breaks long names and `***`; the channel secret's Show button is as tall as its input. Labels are muted 12/500 and intro text muted across these dialogs, with curly quotes.

### Shared dialog API, part 6: the command recorder and the nightly check (ADR 0026 §3, §6; package 1.2b)
- Every command a dialog window sends through `shellInvoke` is recorded for its dialog (`dialogCommandLedger`;
  `dialogWindowConfig({ baseCommands })` names what it may send); an undeclared one is reported at once. The boot harness
  records too; `PADDOCK_DIALOG_COMMANDS_SEEN` writes `dialog-commands-seen.json`.
- `scripts/dialog-commands.mjs` (`npm run check:dialog-commands`, nightly only): every declared command was sent, or is
  listed in the app's `dialog-commands-unexercised.json` with a reason, whose count may only fall
  (`dialog-commands-unexercised.count`); every dialog was opened (`openEveryDialog`'s list).
- `@decryp7/paddock-e2e`: `openEveryDialog`, `measureDialogWindow`. ESLint `dialog-content-footer` checks every component
  that is a dialog content, whatever its file name.
- workbench-demo's dialog windows may send the start-up handshake (the recorder found the base set lacked it).

### Shared dialog API, part 5: macOS attach at show and the configured deadline
- macOS: an owned dialog window is built without a parent and attached with `addChildWindow:ordered:` when it is shown
  (`objc2-app-kit`, a macOS-only dependency of `paddock-dialogs`, already in every app's tree through Tauri), instead of
  built with its parent and hidden again.
- The shell plugin passes `paddock_startup::Config::dialog_show_deadline` to `Dialogs::with_show_deadline`.

### Shared dialog API, part 4: test kit and checks (ADR 0026 §6; package 1.2b, first part)
- ESLint `paddock/dialog-frame-footer` (in the preset for the templates of `*dialog*` components): no `footer`, foot or
  actions row, and no Close, Cancel or OK button in a dialog content.
- The boot harness reports a content that draws its own footer (`ownFooter`), a missing ready handshake and a ready
  measure whose minimum exceeds its content.
- `@decryp7/paddock-e2e`: `dialogFitProblems` (ADR 0016 check 1: scales 1, 1.25 and 1.5 on 1920x1080 and 1366x768; footer
  buttons inside the window, on top, at most 24 px above its bottom); the shell demo runs it over every dialog.

### Shared dialog API, part 2: DialogFrame, declared actions and pd.confirm (ADR 0026 §2, §4.8, §5; packages 1.2a and 1.2c)
- **Breaking.** Every dialog renders in `DialogFrame` (`@decryp7/paddock-ui/shell/dialogs`): an optional header, a body that
  alone scrolls and a footer the frame draws from the content's declared actions (`DialogContent.dialogActions`, a signal of
  `AnswerAction | RunAction | DismissAction`; `dialogStatus`, `dialogMinBodyHeight`, `dialogHeader`). `layoutActions` (pure,
  tested as a table) orders them natively per OS (macOS and Linux: Cancel left of the primary; Windows: primary, then
  Cancel), allows one primary, one destructive and one dismiss (none for a required choice; the frame adds Cancel/Close),
  never makes a destructive action the Enter target, and picks the first focus (a form's first field, marked
  `data-autofocus`; a destructive decision's Cancel). Busy (spinner, `aria-busy`, inert body; abortable work is aborted on a
  dismissal, other work finishes first, "Finishing…") and rejected states (`DialogActionError`'s message, Try again) are the
  frame's. An invalid declaration renders one Close and is logged.
- **Breaking.** Outcomes: `DialogRef` (what a content injects) has `answer(value)` and `dismiss()`; the opener's
  `DialogHandle.closed` and `ShellCore.openDialog` resolve a `DialogOutcome` (`answered` with a value, or `dismissed`),
  never an ambiguous `undefined` (`answerOf`, `answered`, `DISMISSED`). On the wire an answer is `{ answered: true, value }`.
- A dialog window measures itself synchronously once the content and `document.fonts.ready` are there (no frame wait,
  the 1.1-0 spike) and sends `DialogIo.ready`, then `fit` at most once per frame when its natural size, the fonts, theme or
  density change; decisions and forms shrink as well as grow. In the page a dialog stays hidden until ready or the show
  deadline (Loading with Close), and a modal traps the focus (CDK). Focus returns to the opener. The `keys.ts`
  heuristics (`hasCloseAction`, `findDefaultAction`) are gone.
- The 13 shared dialogs and the shell demo's 5 and the workbench demo's 2 declare their actions and lose their footers.
  Destructive actions say what they destroy (Remove channel, Revoke key, Clear cache / Delete data); Updates' job controls
  read Stop checking / Stop download.
- New `@decryp7/paddock-ui/shell/confirm` (`pd.confirm`, `defineConfirmDialog`, `confirm()`; contract
  `paddock_shell::DIALOGS`): the one shared confirmation (heading, message, details, a primary or destructive affirmative).
- `shell/testing`: `renderFramed` and `createFramed` render a content in the real frame for its spec.

### Shared dialog API, part 1: contracts in Rust (ADR 0026 §1, §3; package 1.1a)
- **Breaking.** Dialog contracts are Rust tables: `paddock_dialogs::dialogs!` (a `Shape` for kind and modality, the width
  steps `Narrow` 400 / `Medium` 560 / `Wide` 720, a tool's `Px(n)`, a viewer's or tool's preferred `height`, `instanced`,
  `required_choice`, `commands`). `command_table!` emits a `dialog_callable` module beside each leaf table (one leaf table per
  module) with a marker per command open to dialogs and `ALL`; `paddock_commands::prefix` and `plugin` build the other entries.
  A misspelt or closed command, a prefix matching nothing, a modal viewer or a required choice that is not modal does not compile.
- **Breaking.** `Contracts::parse`, `ContractFile`, `Dialogs::from_json` and `paddock_shell::Builder::dialog_contracts` are gone:
  `Contracts::from_tables`, `Dialogs::from_tables` and `Builder::dialogs(&[&app::DIALOGS])`; the shell registers its own tables
  (`paddock_shell::SHELL_DIALOGS`: channels, MCP, storage, diagnostics, updater, set-up) and the workbench has
  `paddock_workbench::DIALOGS`. `paddock_shell::dialog_commands` re-exports every shell crate's markers for an app's table.
- **Breaking.** `defineDialog(contract, ui)`: the contract is the generated `DIALOG_CONTRACTS['<id>']` of the owner
  (`generated/dialog-contracts.ts`), the second argument the title, result, providers and loader. `DialogSpec`, `minWidth`/`minHeight`,
  `dialogMinSize`, `KIND_POLICY`, `FALLBACK_WIDTH`, `dialogContract`, `dialogContractFile`, `serializeDialogContracts` and the
  JSON contract files are gone; `DialogRequest` carries no sizes (Rust takes them from the contract). The kind table is generated
  (`DIALOG_KIND_TABLE`, `DIALOG_WIDTH_STEPS`; `fitsContent` and the new `growsOnly` read it), so TypeScript and Rust agree on
  which kinds are fitted. `fakeDialogContract` (`shell/testing`) makes contracts for specs.
- `pd.report-problem` is a modeless form, 720 wide; the shared decisions and forms take the width steps (400 or 560).
- The manifest's dialogs carry `width` and `height` instead of `minSize`.
- `@decryp7/paddock-e2e`: `dialogContracts(...paths)` and `axeEveryDialog(page, paths)` read generated `dialog-contracts.ts` files
  (`parseDialogContractsModule`); conformance C33 reads an app's generated file (or the legacy JSON until Debrief moves).
- The package bin `paddock-check-dialog-commands` and `npm run check:dialogs` are deleted (ADR 0026 §3: the compiler checks
  that a command exists and is open to dialogs; the invoke-layer recorder of package 1.2b replaces the rest).

### Shared dialog API, part 3: the window lifecycle (package 1.1b)
- **Breaking.** `dialog_fit_height` is gone: a dialog window reports `dialog_ready { measure }` once and `dialog_fit { measure }`
  after that, with `measure = { content, minContent }` (the dialog's own inner logical size; the wire type `DialogMeasureWire`
  in `shell/generated/dialogs.ts`). `tauriDialogIo.ready` and `.fit` send them. `BASE_COMMANDS` lists `dialog_ready` and
  `dialog_fit`; apps list the two commands in their table instead of `dialog_fit_height`.
- A dialog window is created hidden (pending), at the placed size, on the dialog shell's surface colour, and shown and
  focused when its page is ready, so the user never sees the 520 px guess or the jump. If the page has not reported within
  `Dialogs::show_deadline` (1 s, `with_show_deadline`; `paddock-startup` `Config::dialog_show_deadline`) it is shown at the
  placed size and a late ready fits it once. The deadline runs through an injectable `Scheduler`.
- The show mechanism per OS is chosen in one function, `paddock_dialogs::show_mechanism(os)`: method d (hidden, show on ready)
  on every OS (the 1.1-0 spike measured macOS, Linux and Windows); the off-screen method is gone.
- The minimum content becomes the OS minimum. The size cap is 75 % of the work area less the dialog window's own
  outer-minus-inner decorations, applies to the preferred size only and never pushes below the minimum content. Physical
  sizes are rounded up once. A decision keeps its offset only, a form its width and offset, a viewer and a tool their size.
- A modal's parent is disabled when the dialog is shown, not when it is created; opening a dialog that is still pending
  creates no window and does not show it early. A scale-factor change refits the window from its last measure.

### Dyno on the shared CI kit (WP-C4)
- Dyno is a leg of `ci.yml` (`apps/dyno/paddock.ci.json`: Rust on Linux, Windows and macOS with clippy on Windows too, the frontend,
  Chromium e2e in two shards, WebKit e2e) and releases through the shared `app-release.yml` from a thin `dyno-release.yml` (nightly
  04:11); `dyno.yml` and Dyno's copies of `scripts/release/*`, `macos-verify-bundle.sh`, `commit-lint`, `export-diagrams.sh`,
  `check-index.mjs` and `ci-prune.sh` are deleted. Dyno's host clean-up ends only the test programs of its own build folder.
- New `app-release.yml` input `bundle-check`: an app's own script run on Linux and macOS after the installers are built, with the bundle
  folder as its argument (Dyno's run-path check; on macOS the bundled `dyno-agent` measures once, which replaces the old Rust-leg step).

### Dialog boot harness waits on the dialog's load, not on a turn budget
- `bootDialog`/`assertDialogBoots` (`@decryp7/paddock-ui/shell/testing`) wait for `whenStable` (the shell's start is a
  pending task), with no turn budget: a slow first `import()` on a busy runner no longer reads as "never leaves
  Loading" (CI run 6563). `DialogHarnessOptions.maxTurns` is removed; a loader that never answers is now reported by the
  runner's test timeout.

### Spotter on the shared CI kit (WP-C3)
- Spotter is a leg of `ci.yml` (`apps/spotter/paddock.ci.json`) and releases through the shared `app-release.yml` from a thin
  `spotter-release.yml`; `spotter.yml` and Spotter's copies of `scripts/release/*`, `macos-verify-bundle.sh`, `commit-lint`,
  `export-diagrams.sh` and `check-index.mjs` are deleted. Kit additions: `paddock.ci.json` `testBrowser` (the pinned Chrome for
  Testing on the host legs, `<PREFIX>_TEST_BROWSER`/`_REQUIRE_TEST_BROWSER`, and `_NO_SANDBOX` on Linux) and `display` (xvfb for the
  tests), `e2e`/`e2eShards` (end-to-end legs, one per shard, in parallel), `e2eArtifacts` (failure report upload), `e2eWebkit` (a WebKit leg of its own), a `webkit` input on `ci.yml`'s manual run (and the Spotter release dispatch passes it),
 `tauri-bundle` `--prepare` (`app-release.yml` input `prepare`: a command after `npm ci`, e.g. building the UI library).

### Pitwall on the shared CI kit (WP-C2)
- Pitwall is a leg of `ci.yml` (`apps/pitwall/paddock.ci.json`) and releases through the shared `app-release.yml` from a thin
  `pitwall-release.yml` (plus the WebKit e2e); `pitwall.yml` and Pitwall's copies of the release scripts, bundle verify,
  commit lint, diagram export and check-index are gone. `pitwall-server --version` is started from the built macOS app.
- New contract field `rustChecks` (Linux lines that run only with the Rust half: Pitwall's coverage guard and time-zone tests).
- New contract field `lastPackages` (`app-gates` input `last-packages`): packages tested apart and last on Linux, left out of host legs
  (Pitwall's Tauri shell, whose build overwrites the server binary the other tests start).
- Pitwall's server plans loads against a `MemoryReader` (`Serving::set_memory_reader`, the host by default); its tests pin a roomy machine.
  The eight-slot slots tests answered 503 `insufficient_memory` on a Windows runner with less than about 11 GiB free (run 6456, job 12127).
- Every memory reading of `pitwall-server` (dashboard sample, limit status, fit checks in management and updates) goes through that one reader.
- New contract field `windowsClippy` (`app-gates` input `windows-clippy`, default off): clippy also on the Windows leg. Pitwall sets it, so
  the server's Windows-service code is linted in CI as before. A failed `prepare` now stops the later `app-gates` steps, which otherwise all report.

### Windows CI: tests that measured the runner's disk, and a toolchain step that says why it failed
- Four tests that timed out on a slow Windows runner (run 6461) prove their behaviour without bulk disk work: the log
  tail is read from a synthetic 1 GiB source that counts the bytes read (a real 1 GiB `set_len` file is a gigabyte of
  zeros on NTFS), the walk's cancel test uses the new `WalkOptions::cancel_check_items` (default `CANCEL_CHECK_ITEMS`; breaking for `paddock-jobs` callers that build `WalkOptions` with a struct literal, which must add the field or use `..Default::default()`)
  with a few hundred files instead of 12,000, the assistant store's bounds are `StoreLimits` (`ConversationStore::open_with`,
  defaults `MAX_CONVERSATIONS` and `MAX_FILE_BYTES`, pinned by a test) and the damaged-package property builds its two
  packages once instead of 256 times.
- `setup-rust` keeps its channel sync, and a failed `rustup toolchain install` now says why (on Windows, Git Bash's exit
  127 means rustup was ended with an NTSTATUS crash code, run 6460). Pinning the toolchain stays an open ADR 0022 decision.
- cmd steps wrote `echo CARGO_BUILD_JOBS=8>>"%GITHUB_ENV%"`, which cmd reads as a redirect of handle 8, so the variable was never
  set on Windows; it is now `(echo CARGO_BUILD_JOBS=8)>>"%GITHUB_ENV%"`, and a conformance test rejects a digit directly before
  a redirect in any workflow or action.

### No transition release, no update bridge (ADR 0014, last amendment)
- Every app release from the monorepo is signed with the shared `PADDOCK_TAURI_*` key only; the
  `<APP>_TRANSITION_SIGNING_KEY` path is gone. Nothing is written to an app's old repository:
  `conformance/release/publish.mjs` loses its `--newest` (bridge) mode, and an app's updater lists only paddock's
  `<app>-latest`. Copies installed before the monorepo are reinstalled once by hand; every release's First launch notes
  say so.

### Debrief's CI on the shared actions
- `debrief.yml` runs its Linux jobs in the paddock-ci image, compiles every Rust job through sccache (setup-rust,
  sccache-stats), builds host legs from a stable checkout into a kept folder set as cargo configuration (never
  `CARGO_TARGET_DIR`), keeps npm's and Angular's caches, and runs the end-to-end tests in four shards (Chromium on
  branch runs, WebKit too on full runs). Installers and the real-app smoke run only on main, `debrief-v*` tags, the
  nightly run and a manual run with `bundles: true`. Its W0 shell baseline is an opt-in project
  (`npm run e2e:shell-baseline`), not part of CI. `conformance/test/ci-actions.test.mjs` checks these rules.

### The other six apps imported (ADR 0014)
- Pacenotes, Sighter, Spotter, Pitcrew, Dyno and Pitwall are in `apps/<app>` with their history (tags `pitcrew-v*`,
  `pitwall-v*`). Their UIs are workspaces of the root npm lock; Spotter uses `@decryp7/paddock-ui` from `ui/dist`
  (tsconfig paths) instead of the registry's 0.1.
- Each of Spotter, Pitcrew, Dyno and Pitwall has `.gitea/workflows/<app>.yml` (its build.yml, gated by
  `affected.mjs --ci --app <app>`, releasing from `<app>-v*` with `<app>-latest`); their updaters read paddock's
  `<app>-latest` only (no update bridge, below). Pacenotes and Sighter are docs only.
- One root `lefthook.yml` (a `root:` per app); Renovate ignores Dyno's plugin template and example.
- Pitwall: the fake engine counts a start only once it serves, so a port lost at the real bind (a parallel test) is
  not a second model load (the flaky `a_dead_engine_is_forgotten_and_reloaded`).

### Debrief imported (ADR 0014, Debrief first)
- Debrief's history is in `apps/debrief` (tags `debrief-v1.0.0`, `-v1.0.1`, `-v1.1.1`), with `test/x50-shell-baseline`
  and `perf/test-speed` (rebased) merged first. Its UI is the `debrief-app` workspace of the root npm lock; its
  notices generator reads the workspace's production closure from that lock.
- Its CI is `.gitea/workflows/debrief.yml` (from its build.yml, gated by `affected.mjs --ci --app debrief`), with
  `debrief-bench`, `-corpus`, `-fuzz`, `-mutants` and `-runner-check`. A `debrief-v*` tag releases here and moves
  `debrief-latest`, the app's one updater endpoint.
- `affected.mjs`: an app's own workflows (`<app>.yml`, `<app>-*.yml`, `release-<app>.yml`) mark only that app; an app with
  `workflow` in its `paddock.ci.json` is not a ci.yml leg. `conformance/release/publish.mjs` takes `<app>-vX.Y.Z` and
  `<app>-latest` tags and `--replace`.

### Monorepo frame (ADR 0014 §11 steps 1 and 2)
- One npm workspace at the root (`package.json`, one `package-lock.json` for `ui`, `eslint`, `icons`, the two demos and
  `apps/*/app`); the five package locks are gone, with every resolved version of `ui/`'s lock kept. `npm ci` runs at the
  root; `ui/angular.json` names dockview's styles by package path; the icon pipeline resolves the Tauri CLI.
- The `decryp7` registry is defined once, in the root `.cargo/config.toml` (crates/ and the demos had their own copies).
- `scripts/affected.mjs` (with tests): the projects a change reaches, from the Cargo and npm graphs, Tauri `frontendDist`,
  config paths and declared doc inputs, with the rust/ui halves and a nextest filter per app. `ci.yml` starts with a plan
  job and runs only the affected jobs, plus one matrix leg per app (`apps/<app>/paddock.ci.json`, the new `app-gates`
  action; the shell and workbench demo jobs are its first legs). `nightly.yml` runs everything with WebKit every night.
- Root `renovate.json`, `CODEOWNERS` (Gitea's Go regular expressions) and a root `AGENTS.md` for the monorepo; the rule
  changes of ADR 0014 §10 in `docs/rules.md`, effective per app from its import. Branch inventory:
  `docs/plans/monorepo-handoff.md`; `pre-monorepo` tags the 0.3.0 head.
- `scripts/publish.mjs` links every package it publishes to the `paddock` repository in Gitea (a failed link is a
  warning; the dry run prints "would link").

## 0.3.0 - 2026-10-25 (the plugin platform and workbench release)

The family release. Highlights: the survey work packages X21 to X54 (shared crates and packages the apps copied until now), the workbench
(`paddock-workbench`, `paddock-panels`, `paddock-view`, the workbench demo app), the out-of-process plugin platform (`paddock-plugin-api`,
`-host`, `-package`, `-test`, `cargo-paddock-plugin`, `paddock-ipc`), the i18n and accessibility kits in `@decryp7/paddock-ui` and
`@decryp7/paddock-e2e`, the shared lint preset, and a much faster CI. Versions: `@decryp7/paddock-ui`, `@decryp7/paddock-e2e` and
`paddock-shell` are 0.3.0; `paddock-commands`, `-download`, `-health`, `-jobs`, `-startup`, `-window`, `-dialogs`,
`-menu`, `-settings` and `-updater` are 0.2.0 (the last four expose types of the bumped crates); the crates new since 0.2.0 are published as 0.1.0 (the six former scaffolds - archive, bench, cli, live, mcp, metrics - have real code and are published too).

### Fixed
- paddock-mcp: stopping the loopback server ends connections whose client stays connected at once on Windows too (reads are cut into 100 ms
  slices that notice the stop; shutting a socket down does not wake a blocked read there, so `Serving::stop` waited about two minutes).
  paddock-pitwall's `FakePitwall::kill` wakes its accept loop once and joins it instead of connecting again (each refused connection costs
  about two seconds on Windows).
- WebKit (macOS WKWebView): every rule that sets `user-select` sets `-webkit-user-select` too (notification text, service-status code, pop-out title bar,
  drag ghost); reduced motion turns transitions off (`0s`) instead of 0.01 ms, which WebKit ran as real transitions and painted inherited colours late; the
  shell demo's key recorder focuses its button on click (WebKit does not). The test kit's `pressTab`/`tabKey` press Option+Tab in WebKit on macOS (full
  keyboard access is off by default). `ui/playwright.config.ts` has `gallery-webkit`, `shell-demo-webkit` and `workbench-demo-webkit` projects with
  `PADDOCK_WEBKIT=1`; CI runs them on the Mac runner for pushes to main and v* tags (job `e2e-webkit`, guarded by a conformance test). `prune-build-folders.sh` counts apparent size too (a late-allocating file system reported a full folder as empty).

### Changed
- The shared UI has a lint step: `npm run lint` in `ui/` runs `@decryp7/paddock-eslint` (rules 27 to 39 plus the angular-eslint template and
  accessibility rules) over every project, and the `ui` CI job runs it before the specs. All 155 findings are fixed (explicit OnPush in test
  hosts, no `any`, `pd-` font sizes and shadows from tokens, signal reads before the first await, labelled a11y exceptions with reasons).
  `pd-select` takes its value as a `model`; the glyph's `dots` input is no longer aliased; `pd-job`'s `(cancel)` output keeps its name with a
  reasoned exception. `@decryp7/paddock-eslint` resolves its peer dependencies from the linted app first (a linked preset works); the style
  rule accepts any `var(--token)` font family; rule 29 does not flag a read inside the operand of the first `await`; a spec may build a probe store.

### Fixed
- sccache in CI uses a local disk cache on every runner (host: `~/.paddock-ci/sccache`, 20G; containers: the `/sccache` mount, else a folder of the
  container) instead of the shared Dragonfly, which filled up and broke the Mac job's cache server. `setup-rust` pins sccache 0.19.0 on every
  OS, and a server that cannot start or answer leaves `RUSTC_WRAPPER` unset so the job builds without the cache. The Redis backend stays an
  option (`sccache-backend: redis`) and is unused by `ci.yml`.
- sccache hit rate on the host runners (macOS 2%, Windows 0%): sccache hashes every `CARGO_*` variable and the jobs set `CARGO_TARGET_DIR`
  to a per-job folder, so no key ever matched. The host jobs now set their build folder with `--config build.target-dir=...` on every cargo
  step (cargo configuration is not hashed) and `RUSTC_WRAPPER` is sccache itself: an earlier `rustc-wrapper.cmd` broke every Windows build,
  because cmd.exe's 8191-character command line is shorter than rustc's arguments for windows-sys. The new `stable-workspace` action moves a host job's checkout to
  `~/.paddock-ci/src/<name>` and links the hashed work folder to it, because sccache also keys on the compile's physical working folder
  (the workspace crates missed on every run; a symlink alone does not help, cargo canonicalizes). The macOS build folder now outlives the job.
- The Node scripts in `conformance/` and `scripts/` run when they are started through a symlink (`isMain`, compares real paths). Node gives
  `import.meta.url` the resolved path and `process.argv[1]` the given one, so after `stable-workspace` linked the macOS work folder the Chrome
  for Testing installer exited 0 without installing or exporting anything and the real-browser tests failed with "CHROMIUM_TEST_PATH names no
  Chrome for Testing". The same held for a script reached through `node_modules`. `install-test-browser` also fails when the installer printed
  no `CHROMIUM_TEST_PATH`.

### Added
- X51 review 5 fixes for `<pd-view>` (2 P3). A drawing label keeps 4 units from every box and every edge (its own included) and sits in the clear slot
  nearest the middle of its edge (`nearestSlot`, `chooseSlot`; "needs db" was 1 unit from its curve). A radial canvas is the bounds of what is drawn
  (boxes, edge paths, labels) plus the usual 16 padding, with no extra margin.
- X51 review 4 fixes for `<pd-view>` (1 P2, 1 P3; `ui/docs/review/x51-view/FIXES.md`). Edge labels are placed by one rule (`placeLabels`): a
  bowed edge's label sits at the apex of the bow on the outward side, and every label is checked against every box, every edge path and
  the labels before it, walking distance, place and side deterministically when it collides; the canvas grows to hold a label. A radial
  edge prefers a bow that does not cross another edge near an arrowhead (the shell to chromium edge now bows). `<pd-view>` scroll regions
  carry `data-layout="done"` once measured, and the e2e `open()` waits for it and for `document.fonts.ready`. Contact sheet `review-5/`.
- X51 review 3 fixes for `<pd-view>` (1 P2, 3 P3; `ui/docs/review/x51-view/FIXES.md`). The inset focus ring of a sortable header, of "Show as
  a list" and of a tree row no longer runs over their text (the e2e ring check now also asserts that). A scroll region is the containing
  block of its visually hidden text, so that text can no longer widen the page at 320 px (`SCROLL_CSS`). A second radial sample with
  spoke-to-spoke edges shows the bows (covered by the edge-crossing tests). Contact sheet `review-4/`: the pointer is moved off the view
  before every non-hover shot.
- `<pd-view>` has no status-to-glyph map of its own (rule 40): tones and icons resolve through `statusGlyph`, so errors draw the circle-x. `TONE_GLYPH` is replaced by `TONE_STATUS` and `toneGlyph`.
- X51 review 2 fixes for `<pd-view>` (0 P1, 6 P2, 9 P3; `ui/docs/review/x51-view/FIXES.md`). Edges never cross a box: back, sideways and
  long edges run in a lane outside the boxes and between the rows, radial edges bow or go round the outside (`route.ts`, a test over
  every layout and the sample, and over the real SVG paths); matrix headings keep a minimum width at 320 px; focus rings of sort headers,
  "Show as a list", scroll regions and the review filter are drawn inside their container (an e2e check over every focusable element);
  hover is a visible fill on rows and drawing boxes; a region that scrolls fades the side that continues; a drawing opens at its middle;
  the narrow "Changes data" badge is "Edits"; the review filter scrolls on one row; class members are padded evenly; the scatter shows
  its unit in the title only. Rule 19: `pd-view` reports (dev mode) a `needsAi` node drawn by a host that provides neither
  `VIEW_NEEDS_AI` nor `VIEW_OPEN_AI_SETTINGS`, and the hint reads "Set up local AI in this app's settings.". The contact sheet is
  `ui/docs/review/x51-view/review-3/`, every section from one commit.
- X51 review 1 fixes for `<pd-view>` (3 P1, 13 P2, 12 P3; `ui/docs/review/x51-view/FIXES.md`). The radial graph puts its hub in the middle
  and the rest on rings that cannot touch, in a canvas fitted to the boxes (a test over every layout: no two boxes overlap); graph
  boxes carry a status glyph and the status word in their name and in "Show as a list", which also lists captions and members;
  `@decryp7/paddock-ui/states` has `pd-needs-ai-card`, `pd-needs-ai` is a container over it, and a `needsAi` node draws the app's own
  component (`VIEW_NEEDS_AI`) or the same card with the plugin's reason and, only when the host provides `VIEW_OPEN_AI_SETTINGS`, the app's
  "Open AI settings" (the plugin's own `setup` action of a `needsAi` node is never sent to the plugin, rule 19); inline kinds pack in a row, words break only when they must,
  headings sit under the view title, stale is a warning with Reload, loading is a skeleton and busy a visible status, one filled
  button per view, one `pd-view-writes` badge, tree status on the mark only, numbers right-aligned with their unit (`Column.align`,
  `Column.unit`, `ChartSpec.unit` in `paddock-view`), chosen paths and secrets shown by name (`ViewHost` pickers resolve `{ value,
  name }`), class diagrams with a stereotype and a member compartment, the review list fits 320 px, scroll regions are tab stops
  only while they overflow. The contact sheet is `ui/docs/review/x51-view/review-2/` as sheets by section.
- Review 2 verification fixes. `pd-table`: a required column's floor is `min(128px, 40%)`, so in a phone-width pane the rows fit the
  body (no sideways scroll inside the table, the last row action stays inside, header and rows share one edge with a scrolling body).
  Recent files: the file name shortens in the middle (the extension survives) through `pd-path-text`, and a bare name takes no space
  for an empty directory part.
- Review 2 of the files and storage UI. The clear confirmation no longer repeats its title and is sentence case ("Clear logs?",
  "Up to 4.8 MB is removed."); loose entries and the folded smallest items of a tree share one "Other items" group (no group of one that
  repeats its row, no glyph that looks like a menu); `pd-table`'s `groupMono` takes a predicate, a column track in `em` is documented as
  wrong (header and rows differ in font size, which put the tree's Size header 5 px off its values), the header keeps the body's
  scrollbar width and group names truncate; the Storage table counts "3 kinds of data"; the `reveal` arrow is inside the margin; the
  320 px shots show their subject (Settings closed before the Files shot, the table scrolled into view, the row menu open).
- X51 `@decryp7/paddock-ui/view`: `<pd-view>`, the renderer of the declarative views plugins return (ADR 0002, `paddock-view`). Every
  node kind (stack, text, table with sort and paging, lazy tree, graph, diagram, form, review list, matrix, chart, badge, progress,
  empty, needs-AI, link); an unknown kind says "Update the app to view this" for that node only. Documents are validated on receipt by a
  TypeScript mirror of the engine limits (`validateViewDoc`), patches (`applyViewPatch`: replace by id with the base etag checked, or a
  whole document) are applied by `ViewStore` (stale or invalid: inline error with Retry, the last good view stays), actions leave as
  `ViewAction` with the host keys `row`, `page`, `node`, `finding`, `reason`, `values`, `column`, `descending` (matrix cells add `row`,
  `col`, `rowId`, `colId`). Confirmations, the OS browser and the path and secret pickers are the host's (`VIEW_HOST`, refused by default).
  Gallery section, Playwright spec (axe, 320 px) and the review sheet `ui/docs/review/x51-view/`.
- `cargo-paddock-plugin` (X51, ADR 0001): the plugin author's tool, `cargo paddock-plugin new | build | pack | keygen | sign | verify
  | run | test` on `paddock-cli` conventions (ported from Debrief's `cargo-debrief-plugin`). `new` makes a project that depends on
  `paddock-plugin-api` alone; `pack` writes a deterministic `.paddockplugin` (optionally Ed25519-signed); `verify` checks checksums,
  signature and trusted keys (exit 4 on failure); `test` starts the built program against a fake host, compares it with its
  manifest and runs the conformance checks (exit 5 on findings). `paddock-plugin-test` gains `FakeHost::launch(program, args)`, which
  starts a plugin program through the same launcher as the app.

### Fixed
- Findings of the independent screenshot review (`ui/docs/screenshots/REVIEW.md`): the menu bar never wraps (it stays one line and
  the app name gives way first; the title bar clips nothing at 320 px); the demo's runner names truncate with an ellipsis and a
  tooltip, and State and the counts give way before the name; `pd-path-text` shows at most one ellipsis; `pd-job` shows an icon
  beside Done and Failed, a Retry on a failed job when the host offers one, and a Cancel that is never disabled (a step that
  cannot be interrupted says so, and that Cancel acts when it ends; rule 11); the jobs status item counts the same elapsed time as
  the job card; a status chip whose state is unknown is neutral, not danger; `pd-segmented-control` groups its counts. The gallery
  keeps its click counters out of sight, names the tray popover after the accent's app, labels the metrics, and the README points
  at images that exist (14 sections, seven accents).
- UI suites green on the integration branch: the reference specs of jobs and notify provide the undo store the demo context now
  injects; `UNDO_NOTICE_KINDS` lives in `undo-kinds.ts` so the registration file imports no store (the registration lint); the shell
  demo's menu bar test expects Edit (undo, X44); the channels retry test steps the demo engine by hand (`?channels-step=manual`)
  instead of racing its 400 ms timer; the dock's "attach before dockview has loaded" spec forgets the module itself instead of
  relying on a fresh worker.
- Order-dependent UI specs (X50, rule 5): the Angular builder runs Vitest with `isolate: false`, so spec files share a window per
  worker. `installTestKit` now puts back real timers, restores spies, undoes stubbed globals and empties the page and both
  storages before and after every test; the dock's attach spec holds the dockview import (`forgetDockviewForTest(load)`)
  instead of polling, and `DockPersistence.flush()` replaces sleeps in the dock's persistence specs.
- X50 workbench review 2: folded table rows grow (`PdColumn.foldBelow`, additive; `headerHidden` for glyph columns), the
  Explorer slides over the dock below 480 px, severity glyphs by shape (new `pd:error`), an open fly-out is not drawn as
  selected, neutral or status-toned activity badges, the menu opens below its row and is never painted before it is placed,
  marks on provider results, the active tab stays visible in an overflowed strip, pop-out titles use the panel's title,
  the Outline and Files follow the active document after a restore, the assistant's declined/failed/approval states, the
  composer clears on send, a streaming cue and the log following the user's own question; save and manage layout dialogs
  are real dialog windows with declared contracts, and pop-outs are saved with the layout.
- `paddock-assistant`: `Access::Destructive` (a write that cannot be undone); `ToolInfo.destructive` and `destructive` on the
  `toolStarted`/`approvalNeeded` events (generated TS). The approval of such a tool is a danger choice: danger-tinted card,
  "This cannot be undone.", Allow as a danger button (never the primary) and Decline the focused default. New shared glyphs
  `pd:symbol-function`, `pd:symbol-type`, `pd:symbol-module`; `namesPaletteProvider({ icon })` also takes a function per match.
- Status glyphs (review 4 follow-up, rule 40): `PdNeedsAiCard`'s `icon` is a status (`'info' | 'warning'`, was `'alert'`), the
  pitwall needs-AI state, the assistant's tool cards and the Knowledge packs page draw their status through
  `<pd-status-glyph>`, and `assertStatusGlyphs`/`statusGlyphRenderProblems` (`@decryp7/paddock-ui/testing`) check the drawn
  shapes in each shared component's status states, so a status glyph bound by `[name]` cannot bypass the map.
- X50 review 4: icon tiles have no seams (the macOS drop shadow blends in sRGB) and `icons/tools/scan.mjs` fails a build
  on a step above 3 levels in any tile column; a reopened assistant conversation shows its tool cards, citations, declined or
  stopped calls and failed turn as they were live (`StoredMessage` tool `outcome`, `callSummary`, `write`, `destructive`,
  `error`, `endMessage`, all optional; `ToolFinished.outcome`); the assistant log keeps the latest item in view when it gets
  shorter; the status-glyph lint also refuses status → glyph maps in TypeScript.
- X50 review 3: one status → glyph map (`PD_STATUS_GLYPHS`, `statusGlyph`, `<pd-status-glyph>` in
  `@decryp7/paddock-ui/icons`) with a lint over every template, so errors draw the circle-×, never the warning triangle;
  `pd-popmenu` reveals by opacity, so keyboard-opened menus take the focus (Shift+F10 in the Explorer focuses the first
  item and Escape returns to the row); native checks and radios take `--pd-accent-control`; `--pd-surface-code` for code;
  `ShellNotice.short` for a small window's one-line toast; tab strips scroll only to tab boundaries without a scrollbar; a
  bottom slide-out stays off a full-height side column; the assistant's stopped tool is neutral, deleting the open
  conversation switches away (Undo brings it back), the title shows on send; the workbench demo's icon has its own
  silhouette on a neutral near-black tile.
- `scripts/new-app.mjs`: a new app has an e2e smoke suite on `@decryp7/paddock-e2e` (`playwright.config.ts`,
  `e2e/smoke.spec.ts`, `npm run e2e`) and, from the workbench demo, its unit tests (a `test` target, `test-setup.ts`, jsdom and
  the contribution lint); a template without unit tests gets no `test` script. The workbench demo has its own Vitest target
  (`ng test workbench-demo`, part of `npm test`). Checked end to end once: a generated app's Rust tests, build, unit tests and
  e2e pass against this repository's crates and packages.

### Changed
- BREAKING (X51, unreleased crates): one `ProtocolVersion` and one `GrantRef`. `paddock-plugin-package` drops its own
  `ProtocolVersion` (and the `version` module) and re-exports `paddock_ipc::ProtocolVersion`; `paddock_ipc` gains `Skew`,
  `ProtocolVersion::skew` and `introduced_in`, which `paddock-plugin-api`'s `HostInfo` now builds on instead of a second copy of
  the rule. `Manifest::check_compatible` and `paddock_plugin_api::compat::protocol_of` are removed (use `HostInfo`).
  `paddock_view::GrantRef` is now the validated type of `paddock-plugin-package` (private field, `GrantRef::new`; still a plain
  string in JSON, postcard and TypeScript), re-exported by the package; the view validator no longer re-checks it.
- X21 review 3: the resting "..." button shows the full subtle text on a selected row (the rest mix was 2.71:1 on the tint; the
  token test now covers the selected tint and surface-2); paths are cut at segment boundaries and keep their trailing segments
  (`…/Debrief/plugins/viewer/plugin-1.bin`); the donut total is one line; chart loading shows muted bars and a label (never an empty
  chart); chart error groups message and Retry; the filter stays (disabled) while loading; from 4 series on `pd-chart` isolates a series on
  hover and the guide asks for a non-colour key; dark disabled fields get more lift.

- X21 review 2 (`ui/docs/review/x21-data/REVIEW.md`): `pd-middle-text` measures the room it is given (`flex: 1 1 0`), so paths are cut
  only as far as needed and grow back when the table widens; the donut lays out by the card's own width (list under the ring below
  400 px, the card grows to hold it), the total in its middle is drawn at the ring's centre (a tree-shaken loader needs
  ECharts' `GraphicComponent`), shares add up to 100, a two-way sort hint, no count while loading or empty and a stable action
  track, locale-formatted counts, titles as "Label (unit)", short fields narrow the control only, disabled fields read as
  read-only. Design guide: dark palette slot 6 is #7A70FF (slot 6 is at least 20 from every other slot under protan and deutan),
  the scope of the CVD rule is written down, and the row "..." button rests in `--pd-row-action-rest` (subtle text at 75%, 3:1) instead of 40% opacity.

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
- `paddock-chromium`: a page's first load waits (within its timeout, on events) until the tab's own first navigation, reported
  by `Page.enable`, has committed or stopped, so it cannot cancel the load (`net::ERR_ABORTED`); an abort the driver cannot name
  carries the tab's navigation, lifecycle and document network events; only documents' network events are logged.
  `discover::TEST_ISOLATION_FLAGS` keeps a test browser off the network (no proxy lookup, no host but loopback resolves). The
  fake browser has `UserAction::SlowStartPage`/`FinishStartPage`.
- `paddock-chromium` on Windows: every end of the browser (the time limit's included) goes through its `Supervised`, so the
  kill-on-close job closes with it; the Chrome for Testing self-test checks the version manifest and a headless page dump (Chrome
  prints no `--version` there). The real-browser test runs on macOS and Windows CI too (install action, `catalogue:`).
- Review fixes for the files and storage UI (independent review, 2 P1, 11 P2, 9 P3). Settings > Storage is the shared `pd-table`: one
  line per row, rows grouped by cache or the user's data with the aggregate size on the group row, hover icon buttons (What is inside,
  Show in the file manager) and a "…" menu ending in Clear in danger text; the cache button is a normal button. The size tree no
  longer counts a folder twice (its size is on the group row, its items are the rows, with `pd:` glyphs and a title and Close);
  lower bounds read "at least 3.9 GB", rounded down. `pd-table` gains `groupSummary`, `groupMono` and `countLabel`. Recent files lead
  with the file name and show the folder as a muted second line; `pd-path-text` keeps the separator beside the ellipsis. Glyphs: the
  page fold fixed, bigger `file-code` brackets, a lighter `archive` lid, a `reveal` glyph for the action. The contact sheet is
  1440 x 900 at 2x with 320 px, hover, menu and confirmation shots.
- `conformance/ci/pin-runtime.mjs`: pins any `paddock-runtime` catalogue (size and SHA-256 measured while downloading, `--version`,
  `--chrome-for-testing`); `install-test-browser.mjs --catalogue` and the action's `catalogue:` input install the build a catalogue
  pins. `paddock-chromium`'s waits (`start_page`, start-up, `wait_ready`) run on the injected clock; its real-browser test runs
  in the Linux CI job. The `paddock-ci` image (v2) carries Chrome for Testing 156.0.8078.47, the build `chromium.toml` pins
  (was 156.0.8078.4); `scripts/ci-image.test.mjs` keeps them equal.
- Files and storage follow-ups (plan X31/X32). `paddock-storage`: the size tree (`Storage::tree`, `storage_tree`, `StorageEvent::Tree`:
  depth 3, 12 items per folder with the rest folded, bounded in entries and time, no links, one volume) and, in `shell/storage`, a
  Folders button on the Storage page that shows it as a `pd-table` grouped by top-level folder. `paddock-files`: `Files::open_at_line` and
  `files_open_at_line` (a granted file in the editor at a bounded line, the editor found per OS, never run); `RecentEntry::folder`.
  `@decryp7/paddock-ui/icons`: four glyphs (`file`, `file-text`, `file-code`, `archive`) and `fileIcon()` in `shell/files`, which `pd-recent-files`
  draws at 16 px; a contact sheet for their independent review (`CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x3x`).
- `paddock-plugin-api`, `paddock-plugin-host` and `paddock-plugin-test` (plan X51, ADR 0001 to 0003), the platform half of
  Debrief's `debrief-plugin-api`, `-host`, `-registry` and `-test`. `paddock-plugin-api`: platform protocol 2.0
  (`PlatformRequest`, `HostCall`), `Plugin`/`Contributions` (tools with checked schemas, settings, views, MCP tools), the
  `Extension` mechanism (`Contract`: an app's own postcard enums routed by namespace, never decoded by the platform), version
  rules with refusals that say who must update, host files, output files, `serve`. `paddock-plugin-host`: installed
  `.paddockplugin` discovery (verified before every start; a changed program is quarantined), grants with default deny and
  the pure `sandbox_for`, a `Supervisor` per plugin (restart with back-off on the injected clock, quarantine through
  `paddock-guard`), guarded `Remote<C>` proxies, grant-checked file reads, outputs and elevated operations, `PluginHost` and
  `Status` for the Plugins view. `paddock-plugin-test`: `FakeHost`, conformance checks, golden helpers and the fixture plugin.
- `paddock-guard` (plan X51, ADR 0001), from Debrief's `debrief_core::guard` without its plugin types: `catch` (panic to a typed
  `Crashed`), `Quarantine` with a `QuarantinePolicy` (3 failures in 10 minutes by default) on the shared `Clock`, failures as
  `Panic`, `Hang` or `Garbled` (a typed error the owner returns is not one), manual `release`, status, an observer, quarantined
  owners kept in a versioned state file that tolerates a missing or damaged file, and `Guard::call`, the building block of
  guarded adapters (refuses a quarantined or hung owner, runs under a time budget on the clock, catches panics, abandons hangs).
- `paddock-view` (plan X51, ADR 0002): the declarative view model plugins return. `ViewDoc` with `Node`/`NodeKind` (stack, text,
  table, tree, graph, diagram, form, review list, matrix, chart, badge, progress, empty, needs-AI, link), `ActionRef` (with a
  label), `ViewAction`, `ViewPatch` and `apply_patch`; JSON (camelCase) and postcard codecs with a size limit and a decode depth
  guard; `validate` with the ADR limits (errors name the node path; http and https links only; every interactive node labelled);
  a never-panics property test and a cargo-fuzz target (`crates/paddock-view/fuzz`); generated TypeScript in
  `@decryp7/paddock-ui` (`view/generated/view.ts`, stale-checked by a test). The `<pd-view>` renderer is the next step.
- The shell demo desktop app serves Settings > AI access (MCP): `Builder::mcp` with a sample server (two read-only tools, one action), the settings in its config folder and the client keys in the OS credential store.
- `paddock-storage` and `@decryp7/paddock-ui/shell/storage` (plan X32): what an app keeps on disk and clearing it safely. Declared
  `Category`s (cache or the user's data, a confined folder, items with the reason they stay, an optional removal through the app),
  a bounded size walk, a scan that is the preview, `clear` that needs it, confirms data per category, re-checks every item before
  deleting it and never leaves the app's own folders (`Storage::within`), as jobs with progress and Cancel (`storage_*` commands and
  `paddock://storage` in the shell plugin, `Builder::storage`). `paddock_files::Files::show_folder`. UI: `StorageBackend`,
  `StorageStore`, the Settings > Storage page and its confirmation window; the shell demo's old generic page is now "Demo data".
- `paddock-mcp` follow-ups (plan X22): `notifications/cancelled` stops the running call (its token is cancelled, it gets no answer);
  the tool calls of one connection run concurrently up to `Limits::max_concurrent_calls` (default 4) and are answered in any order,
  while everything else stays in order; `McpControl` and the Tauri adapter (feature `tauri`: `mcp_get`, `mcp_save`, `mcp_issue_key`
  for the key dialog's window only, `mcp_revoke_key`, `mcp_audit`, `mcp_audit_clear`, the `paddock://mcp` event) with the settings
  in a versioned file, the client keys persisted through `paddock-secrets`, the audit log announced to every window;
  `paddock_shell::Builder::mcp`. The UI side is `@decryp7/paddock-ui/shell/mcp` (Settings > AI access (MCP), the key and revoke dialogs,
  the audit viewer).
- Set-up checklist, no app wiring (plan X19 follow-up). `paddock_health::notify::os::Toaster` gains `request_permission` and
  `open_system_settings`; `PluginToaster` implements them (the plugin's `request_permission`; macOS `open
  x-apple.systempreferences:com.apple.Notifications-Settings.extension`, Windows `ms-settings:notifications`, GNOME
  `gnome-control-center notifications`, otherwise the words say where to look), so `ToasterAccess` needs no closures. `LinkPort::new(link,
  app_id)` starts Pitwall through `paddock-siblings` (`Handoff::Open`); not installed and too old come back as the step's words.
- First-run set-up checklist (plan X19). `paddock_shell::setup` (`Builder::first_run`): the steps in order with their own checks
  (`SetupItem`), the user's answers (skipped, shown) in `setup.json` through `paddock-env`, actions run on a worker with the
  outcome on `paddock://setup`, and the commands `setup_state`, `setup_act`, `setup_repair`, `setup_skip`, `setup_finish`. The shell
  brings four steps over small ports: the background service (`paddock-service`; Install, Repair, or Open Login Items where the
  user's switch is off), Start at login (the `pd.login` setting), notification permission (`Toaster::permission` through
  `ToasterAccess`, Repair opens the system settings) and Pitwall pairing (`paddock-pitwall`'s `Link` through `LinkPort`, with the
  shared needs-AI view and the pairing code to compare); an app adds its own (Debrief's tour). Every step is skippable and a step
  with a problem always has a Repair. `@decryp7/paddock-ui/shell/setup`: `SetupBackend` (Tauri and in-memory), `SetupStore`,
  `defineSetup` (a modeless `tool` window `pd.setup` and the Settings > Set-up page), `SETUP_CONTRIBUTIONS` (Help > Set-up…) and
  `provideShellSetup` (the main window opens the window once on a first run); the components are in `shell/setup/page`.
- `paddock-siblings` (plan X45): hand-off between the apps, from Pitcrew's `debrief_link.rs` and `handoff.rs`. `AppSpec` / `FAMILY` hold
  what each app accepts (the hand-off rows of `docs/ecosystem.md`); `Handoff` is `Open`, `Import { files }` (absolute existing files,
  at most 32, the app's extensions) or `Show { item }` (plain words); `Siblings::open` refuses an unknown app, itself and anything
  the app does not accept before looking for it, finds the program (`PATH`, then the install places per OS, the pre-rename name as
  "needs update"), and starts it detached with the paths or one deep link of its scheme (`pitcrew://show/machine/mac-mini`);
  "not installed" is a plain `Outcome`. The receiving side parses with `Handoff::from_paths` / `from_link`. `testing` has a file
  system and a spawner that start nothing.
- Paddock Workbench Demo (plan X50 W8): `apps/workbench-demo`, the workbench demo as an installable app on the `paddock-shell` and `paddock-workbench` plugins (layouts in the config folder, pop-out panels with the drag ghost, the assistant on Pitwall with one read-only sample tool and a calm "Pitwall not running" state), its own icon, ad-hoc macOS sealing, a release workflow from `workbench-demo-v*` tags (`scripts/demo-release.mjs`, `scripts/demo/`, shared with the shell demo), a CI job, and the starter section in its README. `ui/projects/workbench-demo` picks the Tauri backends when `inTauri()` and boots main, pop-out and dialog windows; `workbenchInvoke` (`@decryp7/paddock-ui/workbench`) sends the plugin's commands to it.
- Tooling of the demo apps (X50 review 2): one release script for both (`scripts/demo-release.mjs <app> ...`, `scripts/demo/bundle.sh` and `verify-macos.sh` take the app as an argument; a new demo is a row of `APPS`), `scripts/new-app.mjs --template workbench-demo [--accent]` starts an IDE-style app, `ui/e2e/workbench-demo-desktop.spec.ts` plays the demo's main, pop-out and dialog windows over a faked Tauri IPC, the status-bar spinner is 14 px with a 2 px stroke and toasts are at most 360 px wide and one line above the status bar at 480 px and below.
- `paddock-workbench` plugin (plan X50 W7): `paddock_workbench::Builder` (`layouts`, `panels`, `window_hooks`, `assistant`, each optional) builds the Tauri plugin `paddock-workbench`: the layout, panel and assistant commands in one composed table behind one guard, generated permission sets `default`/`popout`/`dialog` per window class, and generated `WORKBENCH_COMMANDS`/`WORKBENCH_PLUGIN` in `workbench/generated`.
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
- `paddock-plugin-package` (plan X51, ADR 0001): the `.paddockplugin` package and manifest model, ported from Debrief's
  `debrief-plugin-package` and `debrief-plugin-api` manifest. `Manifest` with `Contribution` (tool, settings, view, MCP tool,
  extension namespace; `writes` marking on actions) and `Permission` (opened file paths, read/write folders by grant, programs,
  network hosts, named elevated operations, MCP, memory, CPU), parsed once from `manifest.toml` with limits and typed errors;
  validated names (`PluginId`, `GrantRef`, `HostPattern`, `Namespace`); a zip with `SHA256SUMS` and a detached Ed25519 signature,
  bounded reading (traversal, links, size and entry limits), `build`, `extract`, `verify_installed`; keys saved owner-only; trust
  levels Builtin / Signed / Unsigned with a versioned `TrustStore`. Fuzz targets for the manifest and the archive reader.
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

### Changed
- This repository's own documentation screenshots (the gallery and the shell demo, `ui/docs/screenshots`) are taken by the kit's
  `captureDocScreens` (`ui/e2e/screenshots.spec.ts`, `npm run screenshots`): 1440 x 900 at twice the density, a fake clock at one
  fixed time, UTC and en-GB, both themes, with a `hashes.json`. `captureDocScreens` accepts `folder/name` screens and an `element`
  to photograph. Conformance C24 takes its screenshot folders from `screenshotDirs` in `paddock.conformance.json` (this repository:
  `ui/docs/screenshots`). The images are regenerated and wait for a fresh review (no `REVIEWED.md` rows yet, so C24 reports them).
- `@decryp7/paddock-ui/shell/mcp` (plan X22, ADR 0007): Settings > AI access (MCP). `McpBackend` (Tauri over `mcp_get`, `mcp_save`, `mcp_issue_key`,
  `mcp_revoke_key`, `mcp_audit`, `mcp_audit_clear` and the `paddock://mcp` event; in-memory with the engine's refusals), the per-window `McpStore`,
  `defineMcpSettingsPage`/`defineMcpKeyDialog`/`defineMcpRevokeDialog`, `provideShellMcp`/`provideMcpBackend`. The page `PdMcpPage` (`/shell/mcp/page`): the
  master switch (off by default), a switch per read-only tool, a separate off-by-default "Allow" with a warning per action tool, the clients' keys with
  Revoke, and the shared audit viewer `PdMcpAudit` (the family's `pd-table`: time, client, tool, result, size, duration, redacted arguments; Clear). The key
  is issued in the key dialog's window (`/shell/mcp/dialog`, a modal credential form, `mcp_issue_key` for that window only) and shown once with Copy; it is in
  no store, event, log or URL. Shell demo: the page and both dialogs (`?mcp=calls|error`), Vitest specs and a Chromium spec with axe at 320 px.

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
- The Linux guest of `tools/macos-vm-smoke.sh` runs for real: lingering gives `tart exec` a `systemd --user` session, the
  update smoke checks the app's own status, and `--then` runs the new desktop smoke (`scripts/shell-demo/smoke-desktop-linux.sh`:
  Xvfb, stalonetray, dunst; the tray icon is embedded, a notification reaches the server when the agent stops, Quit in the tray
  menu exits 0 with nothing left). `scripts/shell-demo/build-linux-arm64.sh` builds N and N+1 in one throw-away container; the
  `service-smoke` workflow runs both guests. The shell demo's Cargo.lock lists `paddock-cli`.
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
