# 44. Widget grid: free size and drag-and-drop arrangement on a snapped grid, computed by `paddock-widgets`

- Status: Accepted (2026-11-03; user decisions 2026-11-03: D1-D7 as recommended; independent review approved, round 1
  and a delta review of `25e1d694d`). Asked for by the user on 2026-11-03 for Dyno 1.1.0: dashboard widgets get "free size and
  drag-and-drop arrangement" on the grid-snap model (drag widgets on the dashboard to move them, drag a corner or edge to
  resize in grid steps, tidy and aligned, reflowing on smaller windows, keyboard move and resize). The user added the same
  day: "Maybe need a shared crate for the widgets system": §2 makes the split between the shared crate, the shared UI
  and the apps explicit. Independent review round 1 (2026-11-03) found the model, the split and D1-D7 sound and asked
  for changes (two P1, seven P2); this revision answers them: the engine's per-widget `moves` (§5), Move to first and
  last, holes beside tall widgets (§1), the legacy key on upgrade (§4), screen-reader browse mode (§5, WP-SR), the
  mock-ups (pushed History, Material's disabled button, edge grips, guides behind widgets, retuned defaults) and the
  estimate.
- Date: 2026-11-03
- Related: family ADR 0035 (widget layouts are shared; this ADR **amends its §1 placements, edits and undo, §2's
  `KindSpec::spans`, §4's area and chooser for grid surfaces, and §7's dashboard limits**), ADR 0026 §3 (contracts declared
  in Rust, TypeScript generated with `ts-rs`), ADR 0034 / rule 55 (one implementation per mechanism; register entry
  `widget-layouts`), ADR 0041 / rule 60 (tests control time: injected clocks and frame schedulers), rules 39, 42 and 43
  (share first, tested once), rule 47 (Material first), rule 48 (recorded benchmarks), rule 51 / ER-20 (parallelism and
  cost), rule 56 / ER-27 (logs explain failures), AGENTS.md "Where logic lives", the design guide §1, §2, §6, §8, §9 and
  §13; Dyno ADR 0001 (settings live in the agent) and 0012 (widgets are data).
- Mock-ups: `docs/design/widgets-grid/renders/*.png`, light and dark: `edit-drag` (moving a widget: lifted widget,
  snapped drop placeholder, snapping guides, Disk activity previewed in Battery's place: a same-size swap),
  `edit-resize` (corner resize with the size chip, edge grips, Recent events, History and Disk space trend pushed down),
  `edit-keyboard` (a focused widget, its ⋯ menu with the keys, the screen-reader announcement), `narrow` (a 1,000 px
  window: the derived 6-column arrangement, editing explained). Source `mock.html`, rendered by `render.mjs` with
  Playwright's pinned Chromium.
- Evidence read at `origin/main` `a18e18801`, 2026-11-03: `crates/paddock-widgets/src/{catalogue,check,edit,layout,read,view}.rs`,
  `ui/projects/paddock-ui/widgets/src/{area,chooser,frame,widget}.ts`, `apps/dyno/crates/dyno-core/src/widgets/dashboard.rs`,
  `apps/dyno/app/src/app/features/{dashboard/dashboard.ts,widgets/widget-board.ts,widgets/widget-view.ts}`, the dashboards of
  Pitwall, Spotter, Pitcrew and Debrief's Home, and the libraries' published sources listed under "Research".

## Context

- **Today a dashboard placement has a width and an order, nothing else.** `Placement { id, span, widget }`; a kind
  allows a few spans (`KindSpec::spans`; Dyno: 3, 6 and 12 of 12 columns). Editing is the shared chooser
  (`pd-widget-chooser`): a list with drag handles (`cdkDropList`) and Alt+arrow reorder, a Half/Full toggle; the
  dashboard itself is not editable. Rows are as tall as their tallest widget (`align-items: stretch`), so a short widget
  beside a tall one leaves a padded card, and nothing can be taller or shorter by choice.
- **Dyno draws its own grid.** `widget-board.ts` lays the placements out on its own 12-column CSS grid with
  `[style.grid-column]="'span ' + p.span"` and one `@container dash (max-width: 720px)` rule that makes every cell full
  width; the chooser opens in a card above the grid ("Customize dashboard"). Its widgets draw their own card chrome
  (`widget-view.ts`: surface, border, padding, a 132 px minimum height) and the panel kinds (CPU, Why slow, Events,
  History, Consumers, Disk trend) draw a second, different card. `pd-widget-area` (the shared grid) is used only by the
  popup.
- **Released data exists.** Dyno 1.0.0 is in users' hands; its dashboard layouts are stored by the agent as
  `{ version: 1, placements: [{ id, span, widget }], presets }`. They must arrive in 1.1 looking as they did.
- **Process boundary.** The layouts live in Dyno's agent; the webview talks to the app's Rust side, which relays to the
  agent (ADR 0035 §5). A drag needs a preview many times a second; a round trip to the agent per pointer move is the
  wrong cost.
- **Other apps.** Pitwall's dashboard is eleven fixed `pw-panel` cards on its own 12-column grid; Spotter's is a
  sectioned feed (summary, what's new, cadence cards, three charts) on 4 columns; Pitcrew's is a repeated runner tile per
  runner (`auto-fill, minmax(420px, 1fr)`); Debrief's Home is a pane of the shared workbench (its arrangement is the dock's,
  not widgets'). None uses `paddock-widgets` yet except Dyno.

## Decision

### 1. The layout model: a grid of columns by row units

A grid surface is `columns` wide (Dyno's dashboard: 12) and unbounded downwards (at most `max_rows`, 512). A placement is
a rectangle of cells:

```rust
pub struct Rect { pub x: u8, pub y: u16, pub w: u8, pub h: u8 }   // cells; x + w <= columns, y + h <= max_rows
```

- **Row unit.** One row is `--pd-widget-row` (24 px) high; a widget of `h` rows is `h × 24 + (h − 1) × gap` px, with the
  gap the card gap (16 px Standard and Comfortable, 12 Compact), so a row pitch is 40 px (36 Compact): every height is on
  the design guide's 8 px snap. Columns are `(width − (columns − 1) × gap) / columns`. These are the UI's numbers; the crate
  knows only cells.
- **Sizes per kind replace `spans`.** A grid kind declares a `SizeRule`:

  ```rust
  pub enum Steps { Range { min: u8, max: u8 }, Only(&'static [u8]) }   // allowed widths or heights, in cells
  pub struct SizeRule { pub widths: Steps, pub heights: Steps, pub default: Size }  // default inside both
  ```

  `check_catalogue` refuses an empty range, a width above `columns` and a default outside the rule. A kind's minimum is
  the size at which every part of its ready, loading, empty and error states fits (a gallery test renders each kind at its
  minimum and asserts nothing overflows; WP-C2). Popup and menu-bar kinds keep their spans (§1a).
- **No overlaps, gravity up.** Placements never overlap and float up: after every change the layout is *compacted*,
  each placement in reading order (`y`, then `x`, then id) moving up to the highest free row. A layout on disk is always
  compact; one that is not (edited by hand) is compacted on read, without a note. Gravity is vertical only, so it can
  leave a hole beside a tall widget (a short widget's column under a long neighbour's span stays empty until something
  fits it); Grafana's grid does the same, and the alternative (sideways filling) moves widgets the person did not touch.
  A proptest checks the rule exactly: no placement can move up by one row without overlapping, so nothing can rise into
  a hole, and every hole is one no placement fits from above.
- **Collisions.** A placement put on a target rectangle (a drop, a keyboard move, a resize) keeps that rectangle; what it
  overlaps is resolved in one deterministic step:
  1. **Swap** when it overlaps exactly one placement of the same size whose cells it covers completely: the two exchange
     places (the macOS-widgets feel for the most common move, tiles of one size in a row).
  2. Otherwise every overlapped placement is **pushed down** to just below the moved one, in reading order, and what
     those overlap is pushed below them in turn (a cascade, bounded by the placement count).
  3. Then the whole layout is compacted. The moved placement is compacted too: a drop into empty space under the last row
     rises to it.
- **End is the exception to gravity (amendment 2026-11-03).** `ToLast` puts the placement in the first free place after the last one in reading order and does not let gravity lift it into a hole earlier in reading order (a layout is therefore compacted after edits, not on read); the others are compacted as usual, so End always means last.
- **Keyboard steps** are defined on the same rules (§5): a nudge moves one cell, or past the neighbour that blocks it.
- **Identity and limits** stay ADR 0035's: ids from the kind, `max_instances`, `max_placements` (32 on the dashboard),
  presets of at most 24, the 128 KiB file limit.

### 1a. Two arrangements, one crate

A surface declares its arrangement, and the placement type follows from it, so a span on a grid surface or a rectangle on
the popup cannot be written:

```rust
pub trait Place: Clone + PartialEq + Serialize + DeserializeOwned { type Rule; type Gesture; /* check, settle */ }
pub struct Span { pub span: u8 }              // flow surfaces: the popup (2 columns), the menu bar (1); ADR 0035 unchanged
pub struct Cells(pub Rect);                   // grid surfaces: the dashboard
pub trait Catalogue { type Place: Place; /* … */ }
pub struct Placement<W, P> { pub id: PlacementId, #[serde(flatten)] pub place: P, pub widget: W }
```

On the wire a flow placement is `{ id, span, widget }` exactly as today, so popup and menu-bar settings are untouched; a
grid placement is `{ id, x, y, w, h, widget }`. `KindSpec::spans` becomes `KindSpec::size: P::Rule` (`&[u8]` for flow,
`SizeRule` for grid). `Edit<W, P>` keeps its common variants (add, remove, replace, presets, reset, undo, redo) and
replaces `Move` and `Resize` with `Arrange(P::Gesture)`: flow keeps `Move { id, to }` and `Resize { id, span }`; grid has

```rust
pub enum GridGesture {
    Place { id: PlacementId, rect: Rect },          // a drop or a pointer resize: the target rectangle
    Nudge { id: PlacementId, dir: Dir },            // arrow keys
    Grow  { id: PlacementId, dir: Dir },            // Shift+arrow keys: Right wider, Left narrower, Down taller, Up shorter
    ToFirst { id: PlacementId },                    // Home: first in reading order
    ToLast  { id: PlacementId },                    // End: last in reading order
}
pub enum Dir { Left, Right, Up, Down }
```

### 2. Who owns what (the shared split, ER-22 / rules 39, 42, 43)

| Layer | Owns | Never |
|---|---|---|
| **`crates/paddock-widgets`** (pure core) | The layout model (§1, §1a), placement validation, collision, swap and compaction, keyboard nudge and grow semantics, responsive derivation (§3), versioned reading, the upgrade from span layouts (§4), the undo and redo stack, presets, `LayoutView` with every breakpoint's arrangement, the `Outcome` of each edit (what moved where, for announcements), the per-kind size rules' checks. | I/O, Tauri, pixels, app types. |
| **`paddock-widgets` `tauri` feature** (new adapter) | `paddock_widgets::tauri::Previews`: the app's Rust side keeps the last `LayoutView` per surface it relayed (with its revision) and answers `plugin:widgets\|preview { surface, revision, gesture, seq }` from it with the pure `preview` function, in-process, never through the agent. Surfaces register with one line each (`Previews::new().grid::<DashboardCatalogue>()`), keyed by the serialised surface id. | Storing or saving layouts (the app's store owner does: Dyno's agent). |
| **`@decryp7/paddock-ui/widgets`** | `pd-widget-grid` (the grid view, breakpoint choice by container width, placement by CSS grid lines), edit mode, the pointer gesture directive (move and resize, snapping, the placeholder, auto-scroll, cancel), keyboard handling and focus, `LiveAnnouncer` messages formatted from `Outcome`, motion and reduced motion, the frame's card variant, the add sheet, undo and redo controls. Calls the engine through an injected `WidgetGridBackend` (preview, edit). | Collision, compaction, size clamping or derivation: it shows what the engine returns. Pixel to cell rounding of the pointer is its only arithmetic (a purely visual transform). |
| **Apps** | Their catalogue (kinds, titles, icons, `SizeRule`s, defaults, presets), their widget renderers (`*pdWidget` templates, which receive the placement's size), their store owner (where the layout is saved) and the protocol to it, their upgrade hook. | A grid, a drag handler, an edit mode or a copy of a rule above. |

**What moves out of Dyno into shared code:** `widget-board.ts`'s grid, its `@container` reflow rule, its cell wrapper and
its edit toggle become `pd-widget-grid` and its edit mode; `widget-view.ts`'s card chrome (surface, hairline, radius,
padding, head with icon and title, the 132 px minimum) becomes `pd-widget-frame`'s `card` variant, and the panel kinds
lose their own second card (one frame for every kind); `SMALL`, `MEDIUM`, `LARGE` and `SPANS` in `dashboard.rs` become
per-kind `SizeRule`s (app data); the Half/Full toggles of the dashboard chooser go (resize is on the grid). Dyno keeps
its kinds, metrics, thresholds, the value readings and the templates. The register entry `widget-layouts` gains nothing:
no copy is left behind (rule 55 (c)).

### 3. Responsive: breakpoints with derived arrangements

- A grid surface declares breakpoints by its own width (the container, not the window): Dyno's dashboard
  `[{ min: 840, columns: 12 }, { min: 560, columns: 6 }, { min: 0, columns: 1 }]`. At 840 px and more the stored
  layout is shown as it is (the *design* arrangement).
- Narrower arrangements are **derived, not stored**: `derive(layout, columns)` takes the placements in the design's
  reading order, gives each the half or the full width of the narrower grid (the smaller one that is not below its share
  `w × columns / design`, and never below the kind's minimum scaled the same way), keeps each height, and places each one
  at the first free place after the previous one (CSS grid's sparse auto-placement), without compaction. At 1 column every
  widget is full width. Reading order, and so focus order, is the design's at every width (a proptest checks it).
- `LayoutView` carries every breakpoint's arrangement, computed by the engine on each change (three arrangements of 32
  rectangles), so a window resize never needs a round trip and the UI only picks one by container width.
- **Editing happens on the design arrangement.** Below the first breakpoint "Edit layout" is disabled, with the reason
  under it ("Arranging needs a wider dashboard. Make the window wider or collapse the sidebar."); adding, removing and
  widget options stay available from the add sheet and each widget's ⋯ menu. For Dyno, 840 px of dashboard is a 1,104 px
  window with the sidebar open or 944 px with it collapsed; the default window is 1,280 px. (D2 records the alternative of
  editable per-breakpoint arrangements.)

### 4. Persistence, versioning and the upgrade from span layouts

- Wire (camelCase, as ADR 0035 §2a.5): `{ "version": 2, "placements": [{ "id": "cpu", "x": 3, "y": 10, "w": 6, "h": 6,
  "widget": { … } }], "presets": [ … ] }`. User presets hold rectangles too.
- **An upgrade, not a shim.** `Catalogue::upgrade(from: u32, value: Value) -> Result<Value, String>` (default: none)
  runs in `read` when a stored version is older than the surface's `schema_version`; the crate provides the generic step
  `upgrade::spans_to_cells::<C>(value)`. Dyno's dashboard goes from schema 1 to 2 with it, under its new key (below).
  For each placement in stored order: `w` = its span (clamped into the kind's widths), `h` = the kind's default height,
  placed first-fit in reading order exactly as CSS grid's auto-placement placed it in 1.0; then every placement on one
  visual row gets the row's tallest height (1.0 stretched a row to its tallest card, so the dashboard looks as it did);
  then compaction. Presets are upgraded the same way. Each entry that cannot be upgraded is dropped and counted (ADR
  0035's `Dropped` note).
- `Read` reports `upgraded: Some(1)`; the store owner writes the new layout at once (one write, logged) under the new
  key; the span layout stays only under the old key, for a downgrade, until the first real edit (below). An upgrade that
  fails as a whole takes ADR 0035's damaged path: the defaults are shown and the old value is kept unread until the
  first edit, so nothing is lost.
- **Downgrade: the old value stays under the old key until the first real edit.** A grid surface whose stored value is
  upgraded writes the new layout under a new key (Dyno: `dashboardGrid`) and leaves the old key (`dashboard`) holding
  the version 1 value exactly as read. `Read` reports it as `legacy: Some(value)`; `Stored` writes it back unchanged
  under the old key until the first edit the person makes (an upgrade, a reload or a compaction on read is not one),
  then drops it, and the old key is removed. So Dyno 1.0 started after 1.1 still shows the person's own dashboard; after
  their first edit in 1.1 it shows its defaults (and an edit in 1.0 writes only the old key, which 1.1 ignores while the
  new key exists). Dyno 1.0 never meets a version 2 value, so ADR 0035's newer path is not needed for it; it stays for
  later schema versions.
- The upgrade step stays while a supported Dyno release can have written version 1 (all of 1.x); removing it later is a
  normal change that makes such layouts fall back to the defaults with a note.
- **Revision.** `LayoutView` gains `revision: u64` (in memory, bumped by every applied edit and every reload). A preview
  or an arrange edit names the revision it was computed on; a mismatch is refused with `WidgetError::Stale` and the UI
  reloads the view (another window edited meanwhile).

### 5. Interaction

**Edit mode, not always on** (D3). "Customize dashboard" becomes **Edit layout** (outlined, with its icon). Widgets
contain buttons, segmented controls and links, so an always-draggable surface would fight every click; macOS widgets and
Grafana's edit state do the same. In edit mode:

- A sticky edit bar replaces the page's bar: "Editing layout" with one muted hint line ("Drag to move. Drag an edge or
  the corner to resize. Arrow keys move the selected widget; Shift with arrow keys resizes."), then Undo and Redo icon
  buttons (with tooltips and ⌘Z / ⇧⌘Z, Ctrl+Z / Ctrl+Shift+Z elsewhere), **Add widget** (outlined, opens the add
  sheet), a ⋯ `mat-menu` (Presets, Save as preset…, Reset to defaults) and **Done** (the one primary).
- Every edit is applied and saved at once (ADR 0035: no draft); Undo and Redo cover the session (D4).
- Widget bodies are `inert` (no focus or clicks inside); each frame shows a ⋯ button in its head and a resize grip in its
  bottom-right corner (two diagonal strokes in `--pd-text-muted`); on hover and focus the right and bottom edges show a
  split-view grip, two short 1.5 px strokes parallel to the edge (16 px long, 5 px apart, `--pd-text-muted`), the same
  stroke language as the corner grip, never a filled pill that reads as a scroll thumb.
- The add sheet is the chooser's "Add a widget" list in a side sheet (`pd-widget-chooser` in its `add` mode for grid
  surfaces; the "Shown" list and its reorder are not shown, the grid is the editor). Add puts the widget at its default
  size at the first free place in reading order, selects it and scrolls it into view.
- Widget options (Dyno: the metric) move from the chooser list to the widget's ⋯ menu ("Options…", a `mat-menu` panel
  with the app's `pdWidgetOption` template), with Remove (undo toast as today).

**Pointer gestures** (one directive, `pdGridGesture`, for move and resize; D1 says why not CDK drag-drop):

- **Move**: pointerdown anywhere on a frame (not on the ⋯ button or a handle) starts a move after 4 px of travel, with
  pointer capture. The frame lifts (`--pd-shadow-3`, `scale(1.02)`, cursor `grabbing`) and follows the pointer through a
  `transform` set with `Renderer2.setStyle` once per animation frame (no change detection per pointer event). The UI
  rounds the frame's top-left to the nearest cell; **only when that cell changes** it sends a preview
  (`{ surface, revision, gesture: Place { id, rect }, seq }`). The reply's rectangles are drawn: the **placeholder** (the
  snapped target: `--pd-accent-tint` fill, 1.5 px dashed `--pd-accent` outline, radius 10), the **snapping
  guides** (1 px dashed lines in `--pd-snap-guide`, the accent at 70 %, continuing the placeholder's left and right edges
  across the board **behind** the other widgets, so they show in the gaps and in empty space and never cross a widget's
  text), and every other widget at its previewed place (FLIP with `transform`, 160 ms, `--pd-ease-move`). Replies older
  than the latest `seq` are dropped.
- **Resize**: from the corner grip or an edge handle; the frame's size follows the pointer, a **size chip** next to the
  handle says "8 × 7" (columns by rows), and the preview's snapped rectangle (clamped to the kind's rule by the engine) is
  the placeholder. At a limit the chip says "Largest size" or "Smallest size".
- **Drop** sends the edit `Arrange(Place { id, rect })` with the base revision; the reply is the new view. **Escape** or
  a lost pointer capture cancels: everything returns to the stored layout, no edit is sent.
- **Auto-scroll**: while the pointer is within 48 px of the scroll container's top or bottom edge, the page scrolls at a
  speed proportional to the depth (up to 16 px a frame), driven by an injected `FrameScheduler` (rAF in the app, a
  stepped fake in tests, rule 60). While dragging, the grid shows empty rows below its last row (the dragged widget's
  height plus 4) so a widget can be dropped at the end.
- Touch and pen work through the same pointer events; no long-press menu.

**Keyboard** (WCAG 2.1.1; no pointer-only function):

- In edit mode each frame is one tab stop (`tabindex="0"`, `role="group"`, `aria-roledescription="widget"`, named by its
  title, described by its size and place, "6 by 6, position 5 of 15", and by the hint). Tab and Shift+Tab go through the
  frames in reading order; the frame's ⋯ button is the next stop after it. DOM order follows reading order after every
  change, and focus is restored to the moved frame after the DOM reorder (a spec checks it).
- **Arrow keys** nudge (`GridGesture::Nudge`): one cell in that direction if it is free; if a neighbour blocks, the
  widget swaps with it when they are the same size, and otherwise moves just past it (left of it, right of it, above it
  or below it), pushing and compacting as in §1. Under gravity "one free cell down" does not exist, so Down always passes
  the next widget below. At an edge nothing changes and the announcement says so.
- **Shift+arrow keys** grow (`GridGesture::Grow`): Right and Left change the width, Down and Up the height, by one step of
  the kind's rule; neighbours are pushed down as with a pointer resize.
- **Delete** or **Backspace** removes the widget (undo toast). ⌘Z / ⇧⌘Z undo and redo. **Escape** leaves edit mode when
  no gesture is active (edits are already saved).
- **Home** and **End** move the widget to the first or the last place in reading order (`GridGesture::ToFirst`,
  `ToLast`: placed at the top-left or after the last widget at its own size, then settled as in §1), as the chooser's
  ⌥Home and ⌥End do for a list (`chooser.ts`).
- The ⋯ menu lists the same actions with their keys: Move left, Move right, Move up, Move down, Move to first, Move to
  last, Wider, Narrower, Taller, Shorter, then Options…, then Remove. Each item is enabled from the engine's answer, never
  from UI arithmetic: `LayoutView.moves` maps every placement id to
  `Moves { left, right, up, down, first, last, wider, narrower, taller, shorter }` (booleans), computed by
  `paddock-widgets` with every view (each is "would this gesture change the layout", the same function the edit runs;
  32 placements × 10 gestures are microseconds). A key whose move is false does nothing but announce why.
- **Announcements** (CDK `LiveAnnouncer`, polite), formatted by the UI from the edit's `Outcome { position, of, after,
  before, size, limit }`: "Memory moved right, after CPU. Position 6 of 15." "Memory resized to 4 by 6." "Memory is
  already at the right edge." "Memory can't get taller: largest size." Pointer drops announce the result the same way.
  Live widget values stay off live regions (ADR 0035 §6).
- **Screen readers' browse mode.** NVDA and JAWS read a page in browse mode, where arrow keys move the virtual cursor and
  never reach the page; a focused `role="group"` normally switches them to focus mode, but not reliably. WP-SR tests the
  edit-mode keys with NVDA and JAWS on Windows and VoiceOver on macOS (browse and focus mode, Chrome's WebView2 and
  WebKit). If arrows do not reach the frame, the fallback, in this order: the edit-mode grid becomes one
  `role="application"` region whose focused widget is announced through `aria-activedescendant` (focus mode is then
  automatic; the region's description lists the keys), or the frames use `role="button"` with `aria-describedby` for
  the keys, which every reader passes keys through after Enter. The menu path works in every mode and is the floor.
- Hit targets: the ⋯ button 24 × 24; the corner grip a 24 × 24 hit area; edge handles a full-length 8 px strip (pointer
  only, with the keyboard and menu equivalents, WCAG 2.5.8's equivalent exception).

**Measured contrast** (tokens compiled from `ui/projects/paddock-ui/scss`, Dyno accent, 2026-11-03; dark / light): the
placeholder's dashed outline 6.44 / 5.91 on the window background and 5.43 / 4.71 on its own tint, and under
colour-vision deficiency (Machado 2009, severity 1) 5.24 / 7.66 protan, 7.25 / 5.12 deutan, 6.45 / 5.85 tritan; the
placeholder is told apart by its dashed outline, not by colour. Snapping guides 3.67 / 3.55 on the window background
(they are behind the widgets, so the background is what they meet); the corner grip and the edge grips
(`--pd-text-muted`) 7.40 / 7.29 on surface-1; the focus ring 6.03 / 6.45 on surface-1; the size chip's text 13.08 /
17.96. Every non-text part is at 3:1 or more (WCAG 1.4.11); WP-B's token test keeps these pairs.

**Follow-up (not part of this ADR):** the disabled outlined button ("Edit layout" below the first breakpoint) uses
Material's disabled tokens as the family bridge sets them (`--pd-text-subtle` label, `--pd-hairline` outline;
`ui/projects/paddock-ui/scss/_material.scss:481-482`). Whether a disabled control should look more clearly disabled is a
family token change for every app, to be proposed and measured on its own.

**Reduced motion** (`prefers-reduced-motion: reduce`): no lift scale, no FLIP glide (widgets jump to their new place),
no placeholder fade, instant scroll-into-view; the dragged widget still follows the pointer (direct manipulation is not
animation). High contrast (`data-contrast="more"`): frames outlined in `--pd-border-strong`, the placeholder outline solid
2 px.

**Undo and redo** (D4; amends ADR 0035 §1's one level): `Stored` keeps a bounded stack of 20 previous placement lists and
a redo stack, in memory only, cleared on restart; every placement edit pushes (add, remove, arrange, replace, preset,
reset), a new edit clears redo. `LayoutView` reports `can_undo` and `can_redo`. The undo toast after Remove stays.

### 6. Where the logic lives (AGENTS.md)

The engine is the source of truth: validation, collision, compaction, derivation and keyboard semantics live once, in
`paddock-widgets`, with property tests. The UI previews through a **backend operation answered by the same Rust code in
the app's own process** (the `tauri` feature, §2), and the store owner (Dyno's agent) re-validates the committed edit with
the same crate. Against the alternatives:

- **A TypeScript mirror tested against Rust** is a second implementation of one mechanism (rule 55 allows a copy only with
  a delete-by tag; here there is no reason to have one), and a golden-file test only covers the cases written down.
- **WebAssembly of the crate** would need `'wasm-unsafe-eval'` in every app's CSP and a wasm toolchain in CI, for a
  function that costs microseconds.
- **Previews through the agent** add a second IPC hop and the agent's queue to every cell change.

A preview's cost is one in-process Tauri invoke and microseconds of Rust; previews are sent only on a cell change, so a
fast drag sends a few dozen. The mock backend (browser development and UI tests) answers previews from **golden cases**
written by a Rust test (`ui/projects/paddock-ui/widgets/generated/grid-cases.json`: layout, gesture, result, kept current
like the generated TypeScript) and, for any other gesture, places the widget without reflow; it is never the reference
(D6).

### 7. Performance with 32 widgets

- Engine: collision, compaction and derivation are O(n²) over at most 32 rectangles. Budget: a `preview` under 50 µs, a
  `LayoutView` with its three arrangements under 200 µs on the slowest CI machine; WP-A benches both and records them in
  `docs/benchmarks.md` (rule 48).
- IPC: budget p95 under 8 ms for a preview round trip in the real app (measured by WP-D on macOS and Windows, recorded).
- UI: during a drag, Angular renders only on a preview reply; pointer moves update one `transform`. A gallery
  performance spec drags across a 32-widget board with Chromium tracing and records frame times (budget: no frame over
  16.7 ms on the CI Mac, rule 48). Live values keep ticking during a drag (one `MetricReadings` request per tick, as
  today).
- Placement is CSS grid lines (`[style.grid-column]`, `[style.grid-row]`; Angular style bindings, CSP-safe), not absolute
  positions, so a window resize is the browser's layout and never JavaScript.

### 8. Adoption

- **Dyno 1.1.0**: the dashboard is the first grid surface (§9 WP-C). Its popup and menu bar stay flow surfaces.
- **Pitwall**: its dashboard (eleven cards on 12 columns) is the next candidate: it writes a catalogue of its cards and
  their size rules, and its store owner is its own settings (D7: in the first Pitwall release after Dyno 1.1, not in
  Pitwall 1.1, whose scope is fixed).
- **Spotter**: its popup adopts ADR 0035's flow surface as planned; its home is a feed by design (design guide §12), so it
  stays a fixed page unless the user wants a widget home (D7).
- **Pitcrew** (a tile per runner, a collection, not a catalogue) and **Debrief** (Home is a dock pane of the shared
  workbench) do not adopt it.
- Any new app with a dashboard of independent cards uses `pd-widget-grid`; a hand-made dashboard grid is a rule 42
  finding.

### 9. Logging, robustness, security

- The store owner logs each applied or refused edit: surface, edit kind, outcome, revision, duration (no widget options,
  no user content); the upgrade logs "dashboard layout upgraded from 1 to 2: 15 placements, 0 dropped" once.
- The preview command parses its input at the edge (surface id, revision, a gesture with bounded numbers), never takes a
  layout from the webview, and refuses unknown ids; a stale or unknown revision is `Stale`, never a panic.
- `pd-widget-grid` never throws on odd input: a placement outside the grid is drawn where the engine put it in the
  derived arrangement, a missing arrangement falls back to one column, an empty layout shows ADR 0035's empty state.

## Consequences

- One grid model in the family, computed in Rust, used by the UI through one shared component and one preview command.
- `paddock-widgets` gains a `tauri` feature, a generic placement type and an upgrade step; its generated TypeScript and
  `Placement`'s shape change for grid surfaces (a breaking change of the shared crate, adopted by Dyno in the same change,
  rule 43). `@decryp7/paddock-ui/widgets` gains `pd-widget-grid`, the gesture directive and the frame's card variant
  (CHANGELOG), two tokens (`--pd-widget-row`, `--pd-snap-guide`) and a design guide section.
- Dyno 1.1: protocol `PROTOCOL_VERSION` 3 → 4 (new edits and the revision); dashboard schema 1 → 2 with the upgrade;
  the dashboard edits in place; the Half/Full toggles and the chooser card above the grid go.
- Dyno 1.0 after a 1.1 run shows the person's 1.0 dashboard until their first layout edit in 1.1, and its defaults after.

## Alternatives considered

- **gridstack.js 14.0.0** (MIT, no framework dependency, published 2026-09-21): mature grid with gravity, swap and
  responsive columns, CSS variables and `element.style` (CSP-safe). Its keyboard support is Escape and R during a pointer
  drag only, it sets no ARIA, it owns the DOM of its items (Angular's renderer and OnPush templates sit inside a
  foreign layout engine), and its collision rules are TypeScript: the engine could not be the source of truth without
  mirroring them.
- **angular-gridster2 22.0.0** (MIT, peer `@angular/core ^22`, published 2026-06-26): Angular-native, positions through
  `Renderer2.setStyle`. No keyboard handling and no ARIA attributes in its source; layout rules in TypeScript (same
  mirroring problem); a dependency for what is about 600 lines of our own UI code on top of the engine.
- **react-grid-layout 2.3.0** (Grafana's grid): React only.
- **CDK drag-drop for the move** (`cdkDrag` with `constrainPosition`): CDK 22.2.4's auto-scroll runs only inside a
  `cdkDropList`, whose sorting is one-dimensional (`mixed` orientation wraps a list, it does not place rectangles); it
  has no keyboard dragging and no resize. Using `cdkDrag` for the move and our own code for the resize would give two
  gesture models with different snapping, cancel and scroll behaviour. CDK stays for `LiveAnnouncer`, `FocusMonitor`,
  overlays (the add sheet, menus) and the chooser's list.
- **Free placement with holes** (macOS desktop widgets): the user asked for tidy and aligned; holes need their own keyboard
  model.
- **Per-breakpoint stored layouts** (react-grid-layout's model): see D2.
- **Always-on dragging** (Grafana's panel headers): see D3.

## Decisions

**The user accepted D1-D7 as recommended on 2026-11-03**, after independent review round 1 (2026-11-03) had found each
recommendation sound and the delta review approved the revision. The alternatives stay recorded for a later change.

- **D1. Library: our own grid on the engine, with CDK and Material for the parts they fit (recommended)**, rather than
  angular-gridster2 or gridstack.js: neither has keyboard move and resize or screen-reader support, both keep the layout
  rules in TypeScript, and the engine already owns layouts.
- **D2. Narrow windows: derived arrangements, editing only on the design arrangement (recommended).** Alternative:
  editable 6-column arrangements stored as per-breakpoint overrides with "Match the wide layout" (+2 agent-days, a second
  arrangement to explain).
- **D3. An explicit "Edit layout" mode (recommended)** rather than always-on dragging.
- **D4. Undo and redo of 20 steps for every layout edit (recommended)** rather than ADR 0035's single level, which was
  enough for a list but not for a session of drags.
- **D5. Previews in the app's own process through the `tauri` feature (recommended)**, the agent only for commits;
  alternatives in §6.
- **D6. The mock backend answers from Rust-written golden cases (recommended)**; alternative: a WebAssembly build of the
  crate for development and tests only (+1 agent-day, a wasm toolchain in CI).
- **D7. Adoption: Dyno 1.1 now; Pitwall's dashboard in its first release after Dyno 1.1; Spotter's home stays a feed
  (recommended).**

## Work packages

Order: WP-A's types land first (a compiling skeleton), then WP-A's behaviour, WP-B and WP-SR in parallel (WP-SR decides
§5's keyboard focus model before WP-B builds it into the UI), WP-C1 after WP-A, WP-C2 after WP-B, WP-D last. WP-A, WP-B, WP-C1 and WP-C2 land on main as one change (rule 43: the shared crate's breaking
change and its adoption), with Dyno's installer build (rule 54). Estimates in agent-days, reviews excluded.

### WP-A: `crates/paddock-widgets` (implementer, Sonnet; 5 agent-days)

- **Owns:** `crates/paddock-widgets/**`, `ui/projects/paddock-ui/widgets/generated/{widgets.ts,grid-cases.json}`, the
  crate's `crates/README.md` entry.
- **Build:** §1, §1a, §3, §4 and §5's semantics: `Place`, `Span`, `Cells`, `Rect`, `SizeRule`, `GridGesture`, `Dir`,
  `settle` (swap, push, compact), `nudge`, `grow`, `to_first`, `to_last`, `moves`, `derive`, `upgrade::spans_to_cells`,
  `Catalogue::upgrade`, revision, undo and redo stacks, `Outcome`, `LayoutView` arrangements, `WidgetError::Stale`; the
  `tauri` feature's `Previews` and its command; `grid-cases.json` from a test.
- **Tests:** unit tests for every gesture's success and limit; proptests over random layouts and gestures (no overlap,
  inside the grid, every size inside its kind's rule, compaction idempotent, gravity holds (no placement can rise one
  row into a hole), deterministic, `moves` agrees with applying each gesture, a swap undone by the reverse nudge,
  `derive` keeps the design's reading order and has no overlap at 6 and 1 columns); the upgrade of the shipped Dyno 1.0
  default and presets, of a hand-written 1.0 layout, of damaged entries; reading a version 2 layout in a version 1
  catalogue (newer path); the legacy value kept unchanged through reads and dropped by the first edit; Home and End;
  revision mismatch; undo and redo bounds; benches for §7. Coverage ≥ 99 % lines.
- **Gates:** `cargo fmt -p paddock-widgets --check`, `cargo clippy -p paddock-widgets --all-targets --all-features -- -D
  warnings`, `cargo test -p paddock-widgets --all-features`, own `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`.

### WP-B: `@decryp7/paddock-ui/widgets`, gallery, tokens, design guide (implementer, Sonnet; 7 agent-days)

- **Owns:** `ui/projects/paddock-ui/widgets/**` (except `generated/`), the gallery's widgets section and its e2e spec,
  the two tokens (one declared line each in `_tokens.scss`, with the token test), `docs/design-guide.md` §2 and §8 ("Widget
  grid"), the CHANGELOG entry and export line.
- **Build:** `pd-widget-grid`, `pdGridGesture`, edit mode, keyboard, announcements, the frame's `card` variant, the
  chooser's `add` mode in a side sheet, `WidgetGridBackend` and `FrameScheduler` tokens; matching the mock-ups, tokens
  only, Material and CDK as listed.
- **Tests (rule 60: no real time; the frame scheduler and the backend are fakes):** move, resize, cancel by Escape and by
  lost capture, stale preview replies dropped, auto-scroll stepped frame by frame, every key and the menu, focus kept on
  the moved frame after the DOM reorder, announcements' wording, inert bodies, reduced motion (no transforms but the
  dragged one), breakpoint choice by container width, edit disabled below the first breakpoint, odd input (empty, outside,
  missing arrangement). Gallery Playwright (Chromium): drag and resize with the mouse, the keyboard path, axe in both
  themes and in `data-contrast="more"`, the 32-widget frame-time spec. Components 100 %.
- **Gates:** `cd ui && npm run test:tokens && npm run lint && npm test && npm run build && npx playwright test --project=gallery`.

### WP-C1: Dyno engine, protocol, agent and app adapter (implementer, Sonnet; 3 agent-days)

- **Owns:** `apps/dyno/crates/dyno-core/src/widgets/{dashboard.rs,mod.rs,generated.rs}`, `protocol.rs` (the widget
  requests and `PROTOCOL_VERSION`), the agent's widget handler, `app/src-tauri` command registration for `Previews`.
- **Build:** the dashboard catalogue on `Cells` with `SizeRule`s (starting values below, confirmed by WP-C2's
  measurements), schema 2 and the upgrade hook, write-back after an upgrade, the edit log lines, revision through the
  relay.
- **Tests:** the shipped 1.0 default, each preset and a 1.0 user layout upgrade to the expected rectangles; a version 2
  file read by the 1.0 catalogue is kept; the protocol round trip; preview against a relayed view; stale revision.

| Kind | Default (w × h) | Widths | Heights |
|---|---|---|---|
| Health | 6 × 9 | 3-12 | 5-14 |
| Why slow | 6 × 9 | 4-12 | 6-14 |
| Issues | 3 × 6 | 3-6 | 4-12 |
| CPU | 6 × 6 | 4-12 | 5-10 |
| Graph | 3 × 4 | 2-12 | 4-8 |
| Big number | 3 × 3 | 2-4 | 3-4 |
| Bars | 3 × 3 | 2-6 | 3-4 |
| Ring | 3 × 4 | 2-4 | 4-6 |
| Per-core heatmap | 6 × 3 | 4-12 | 3-5 |
| Top processes | 6 × 6 | 3-12 | 5-12 |
| Recent events | 6 × 7 | 4-12 | 4-14 |
| History | 12 × 8 | 6-12 | 5-14 |
| Biggest consumers | 6 × 7 | 4-12 | 5-12 |
| Disk space trend | 12 × 6 | 6-12 | 4-10 |

### WP-C2: Dyno UI (implementer, Sonnet; 4 agent-days)

- **Owns:** `apps/dyno/app/src/app/features/{dashboard,widgets}/**`, the dashboard panels' card chrome, the mock backend's
  widget previews.
- **Build:** `widget-board.ts` becomes `pd-widget-grid` with Dyno's templates; delete its grid, `@container` rule, cell
  wrapper, the chooser card and the Half/Full toggles; panel kinds render inside the shared frame; templates take the
  placement's size (Health shows the hero from 4 columns, as today's "span > 3"); the metric option moves to the widget's
  ⋯ menu. Measure each kind at its minimum and adjust the table.
- **Tests:** each kind at its minimum and default with no overflow (both themes, three densities), edit mode end to end
  against the mock backend's golden cases.

### WP-SR: screen-reader check of the edit-mode focus model (implementer, Sonnet; 1 agent-day; alongside WP-B)

- A gallery prototype of the edit-mode frame (`role="group"` with the keys and announcements of §5, and the two
  fallbacks behind a switch), before WP-B builds the real component. NVDA and JAWS on the Windows VM and VoiceOver on
  macOS, in browse and focus mode, in WebView2 and WebKit: every edit-mode key, the announcements and the menu path.
  The result picks §5's model (the group, or a fallback) and is recorded in this ADR's WP-B brief before WP-B's keyboard
  work starts.

### WP-D: end-to-end, screenshots, real-app smoke (implementer, Sonnet; 1.5 agent-days)

- Playwright against the mock: enter edit mode, drag, resize, keyboard move and resize, undo and redo, add, remove,
  narrow window; screenshots for the docs in both themes; the real app on macOS and Windows: a 1.0 layout upgraded on
  first start, a drag with previews from the real engine, the preview round-trip measurement (§7), a 1.0 downgrade check
  in the local VM (1.0 shows the person's layout after a 1.1 start, its defaults after a 1.1 edit); a short screen-reader
  pass over the finished UI with the model WP-SR chose.

**Total: 21.5 agent-days** (WP-A 5, WP-B 7, WP-SR 1, WP-C1 3, WP-C2 4, WP-D 1.5; revised from 15.5 after review round 1: the `moves` map and the two extra gestures, the
legacy key, the screen-reader matrix and its possible fallback, and the measured default sizes were not in the first
estimate), plus the independent reviews (ER-23 for the mock-ups and the screenshots; code review per
WP).

## Research (sources and dates)

- npm registry metadata, fetched 2026-11-03: `gridstack` 14.0.0 (2026-09-21, MIT, no peer dependencies),
  `angular-gridster2` 22.0.0 (2026-06-26, MIT, peer `@angular/core ^22.0.0`), `react-grid-layout` 2.3.0 (2026-10-05, MIT,
  React), `@angular/cdk` 22.2.5 (2026-10-28, MIT).
- Published sources read 2026-11-03 (the npm tarballs): gridstack `dist/dd-draggable.js` and `dd-resizable-handle.js` (a
  `keydown` listener only during a pointer drag, for Escape and R), `dist/gridstack.js` (`--gs-cell-height`,
  `--gs-column-width` through `style.setProperty`; no ARIA); angular-gridster2 `fesm2022/angular-gridster2.mjs` (39
  `renderer.setStyle` calls, no `keydown`, no `aria-`); `@angular/cdk` 22.2.4 `fesm2022/drag-drop.mjs`
  (`_startScrollingIfNecessary` is called on the drop container only; no `keydown` handling; `mixed` orientation sorts a
  wrapping list).
- Grafana, "JSON model" (fetched 2026-11-03,
  https://grafana.com/docs/grafana/latest/dashboards/build-dashboards/view-dashboard-json-model/): `gridPos` x, y, w, h; 24
  columns; a height unit of 30 px; panels move up to fill space (the gravity this ADR adopts, with a 24 px row).
