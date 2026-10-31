# 35. Widget layouts are shared: `paddock-widgets` and the `paddock-ui` widget area and chooser

- Status: Proposed (2026-10-31), draft for independent review. Needed by Dyno 1.0 (user, 2026-10-31: "In Dyno 1.0, delay
  the tag"), so Dyno's tag waits for work packages WP-A to WP-D.
- Date: 2026-10-31
- Related: Dyno ADR 0012 (widgets and tray items are data, not code; this ADR moves its model into a shared crate and
  replaces its Dyno types), Dyno ADR 0009 (native feel), Dyno roadmap WP-38 (customizable tray popup widgets) and WP-21
  (damaged settings fall back with a note), family ADR 0026 §3 (contracts declared in Rust, TypeScript generated with
  `ts-rs`), ADR 0034 / rule 55 (the copies register, mechanism `widget-layouts`), rules 39, 42 and 43 (share first, tested
  once), rule 47 (Material first), rules 10, 13 and 51 (background work, parallelism and cost), the design guide §1, §5,
  §8 (tray popups, empty and error states), §9 and §13.
- Mock-ups: `docs/design/tray-widgets/renders/*.png` (source `mock.html`, rendered by `render.mjs` with Playwright's pinned
  Chromium): the popup with the defaults, the other kinds, every kind's loading, empty and error state, the chooser, and
  its row menu, drag and undo, each in light and dark.

## Context

The person wants to choose which small widgets Dyno's tray popup shows and in what order: active alarms, disk space per
volume, CPU and memory now, network now, backup health, last issue and pinned counters, with defaults (alarms, disk space,
CPU and memory), persisted, each widget with designed empty, loading and error states, usable from the keyboard and a
screen reader.

What exists:

- **The popover host is shared and stays as it is.** `crates/paddock-window` (`popover.rs`, `tauri/popover.rs`,
  `tray.rs`, `tauri/native_tray.rs`) places, fits and closes the popover window; `pd-tray-popover`
  (`ui/projects/paddock-ui/src/lib/tray-popover.ts`) draws its surface, header, scrolling content and footer. Dyno,
  Pitwall and the shell demo use both. This ADR adds nothing to them: the widget area lives in the popover's content slot.
- **The layout model is Dyno's own, three times.** `dyno-core/src/widgets.rs` (1,082 lines) holds the dashboard layout
  (`WidgetKind`, `WidgetSize`, `WidgetOptions`, `Widget`, `DashboardLayout`, presets with a custom-preset limit) and the
  menu-bar items (`TrayItemStyle`, `TrayItem`, `TrayItems`); `dyno-core/src/settings.rs` holds an older menu-bar model
  (`TrayMetric`, `TrayStyle`, `Tray`); and `app/src/app/features/widgets/widget-model.ts` repeats the catalogue, the
  limits (`MAX_WIDGETS`, `MAX_TRAY_ITEMS`), `accepts()`, `applyPreset()` and `moveWidget()` in TypeScript, which the rule
  on where logic lives forbids. The first two are registered as copies of `widget-layouts` (`docs/known-copies.toml`,
  delete by `dyno-v1.1.0`); the TypeScript copy is not registered.
- **The popover's content is fixed** (`features/tray/tray.ts`): four sparkline tiles and the top five apps.
- Pitwall's and Spotter's popovers (design guide §12: "tray popup lists 5 latest changes") are the next candidates for
  the same model; Dyno's dashboard is the second surface in the same app today.

A popup-only chooser in Dyno would be a fourth copy; rule 55 (c) allows a new copy only with a register entry and a
delete-by tag, and the user wants none left. So the model moves to Paddock now, and Dyno's three surfaces (dashboard, menu
bar items, popup) move onto it in the same release.

## Decision

### 1. One shared crate, `paddock-widgets` (pure, named by the concept)

A pure crate: no I/O, no Tauri, no app types. It owns everything about a layout that is not what a widget *shows*:

- **Placements and layouts.** A layout is an ordered list of placements on one *surface* (a dashboard grid, a popup, a
  menu bar). A placement is an id, a span (columns of the surface's grid) and the app's widget value.
- **A closed catalogue per app and surface.** The app implements `Catalogue` for each surface: its widget type (a closed
  Rust enum, its data the widget's options), each kind's title, summary, icon name, allowed spans, how many may be
  placed, how it refreshes, the defaults and the built-in presets. Unknown kinds in a file are dropped with a note, never
  rendered (ADR 0012 §2, kept).
- **Limits** per surface and per kind (§6), checked on every read and every edit.
- **Versioned, lenient reading.** A layout read from settings never fails the settings file: a damaged layout, a newer
  version or an oversized one gives the defaults and one note; each invalid or unknown placement is dropped on its own,
  with a count in a note (Dyno WP-21's behaviour, now shared).
- **Edits as commands.** Add, remove, move, resize, replace, apply a preset, save and delete a user preset, reset to the
  defaults, and one level of undo for the destructive ones (remove, apply a preset, reset). Each edit is a pure function
  `Layout -> Result<Layout, WidgetError>` that validates the result; the engine applies it and persists it.
- **The widget state envelope** every surface renders the same way: ready (with an "as of" time and a stale flag),
  empty (what, next step) or error (what failed, cause, retry). *Loading* is not on the wire: it is the UI's state
  before the first value arrives.

What a widget shows, its values and its domain rules (Dyno's `Metric`, thresholds, which metric a ring accepts) stay in
the app (rule 42: domain logic stays in the app).

### 2. API sketch (Rust)

```rust
// crates/paddock-widgets/src/lib.rs: catalogue, placement, layout, read, edit, state, generated (tests only)

/// A placement's id: 1-64 characters of [a-z0-9-], unique within its layout. Parsed once (serde try_from).
pub struct PlacementId(String);

/// The surface a catalogue fills: its grid and its limits.
pub struct Surface {
    pub id: &'static str,             // "popover", "dashboard", "menuBar"
    pub schema_version: u32,          // bumped by the app on a breaking change of its widget type
    pub columns: u8,                  // popover 2, dashboard 12, menu bar 1
    pub max_placements: u16,          // popover 8, dashboard 32, menu bar 4 (the OS may report fewer, ADR 0012 §5)
    pub min_placements: u16,          // 0: an empty popup is allowed and designed (§4)
    pub max_user_presets: u8,         // dashboard 8, others 0
    pub max_preset_placements: u16,   // dashboard 24
    pub max_bytes: usize,             // the largest serialised layout read: 128 KiB
}

/// How a kind gets fresh values (drives the engine's refresh and the stale flag; never a UI timer).
pub enum Refresh { Live, Every { secs: u16 }, OnChange }

/// One catalogue entry, as data.
pub struct KindSpec<K: 'static> {
    pub kind: K,
    pub title: &'static str,          // sentence case: "CPU and memory"
    pub summary: &'static str,        // one line for the chooser: "Load and use now, with the last minute"
    pub icon: &'static str,           // the app's registered icon name
    pub spans: &'static [u8],         // allowed spans, e.g. [1, 2]; the first is the default
    pub max_instances: u8,            // 1 for most; disk space 4
    pub refresh: Refresh,
}

/// The app's widget value: a closed enum whose variants carry the widget's options.
pub trait Widget: Clone + PartialEq + Serialize + DeserializeOwned + TS {
    type Kind: Copy + Eq + Hash + Serialize + DeserializeOwned + TS + 'static;
    fn kind(&self) -> Self::Kind;
    /// The widget's own options are valid; the message says what to change.
    fn validate(&self) -> Result<(), String>;
    /// Ranges clamped and text trimmed (a valid widget is also an in-range one).
    fn clamped(self) -> Self { self }
}

pub trait Catalogue: 'static {
    type Widget: Widget;
    const SURFACE: Surface;
    fn kinds() -> &'static [KindSpec<<Self::Widget as Widget>::Kind>];
    fn defaults() -> Vec<Placement<Self::Widget>>;
    fn presets() -> Vec<Preset<Self::Widget>> { Vec::new() }
}

#[derive(Serialize, Deserialize, TS)] #[serde(rename_all = "camelCase")]
pub struct Placement<W> { pub id: PlacementId, pub span: u8, pub widget: W }

#[derive(Serialize, Deserialize, TS)] #[serde(rename_all = "camelCase")]
pub struct Preset<W> { pub id: PlacementId, pub name: String, pub builtin: bool, pub placements: Vec<Placement<W>> }

/// The stored layout. Built-in presets are not stored (they come from the catalogue on every read).
#[derive(Serialize, TS)] #[serde(rename_all = "camelCase")]
pub struct Layout<W> {
    pub version: u32,
    pub placements: Vec<Placement<W>>,
    pub presets: Vec<Preset<W>>,      // the user's own only
    #[serde(skip)] previous: Option<Vec<Placement<W>>>,   // one level of undo, in memory until the next edit
}

/// What reading found: the layout to use and what was wrong (shown once, as WP-21 does).
pub struct Read<W> { pub layout: Layout<W>, pub notes: Vec<Note> }
pub fn read<C: Catalogue>(value: &serde_json::Value) -> Read<C::Widget>;    // never fails
/// A settings field that never fails its file: `#[serde(default)] popover: Stored<PopoverCatalogue>`.
pub struct Stored<C: Catalogue> { /* Layout<C::Widget> + notes; Deserialize goes through read::<C> */ }

#[derive(Serialize, Deserialize, TS)] #[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Edit<W> {
    Add { widget: W, span: Option<u8>, at: Option<u16> },   // id derived from the kind, unique ("disk-space-2")
    Remove { id: PlacementId },
    Move { id: PlacementId, to: u16 },
    Resize { id: PlacementId, span: u8 },
    Replace { id: PlacementId, widget: W },                  // change options (which volume), same kind
    ApplyPreset { id: PlacementId },
    SavePreset { name: String },
    DeletePreset { id: PlacementId },
    ResetToDefaults,
    Undo,
}
pub fn apply<C: Catalogue>(layout: &Layout<C::Widget>, edit: Edit<C::Widget>) -> Result<Layout<C::Widget>, WidgetError>;

/// What the chooser needs to draw and limit itself (the UI never hard-codes a limit or a title).
#[derive(Serialize, TS)] pub struct CatalogueInfo<K> { pub surface: SurfaceInfo, pub kinds: Vec<KindInfo<K>> }
#[derive(Serialize, TS)] pub struct LayoutView<W, K> {
    pub layout: Layout<W>, pub catalogue: CatalogueInfo<K>, pub notes: Vec<Note>, pub can_undo: bool,
}

/// One widget's value as the engine reports it. `V` is the app's value enum.
#[derive(Serialize, Deserialize, TS)] #[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum WidgetState<V> {
    Ready { value: V, as_of: Option<i64>, stale: bool },
    Empty { title: String, detail: String, action: Option<WidgetAction> },
    Error { title: String, cause: Option<String>, retry: bool, action: Option<WidgetAction> },
}
#[derive(Serialize, Deserialize, TS)] pub struct WidgetAction { pub id: String, pub label: String }  // app-defined id
#[derive(Serialize, Deserialize, TS)] pub struct PlacementState<V> { pub id: PlacementId, pub state: WidgetState<V> }

#[derive(Debug, thiserror::Error, Serialize, TS)]
pub enum WidgetError { BadId, DuplicateId(String), UnknownPlacement(String), TooMany { what: String, max: u16 },
    KindLimit { kind: String, max: u8 }, SpanNotAllowed { kind: String, spans: Vec<u8> }, Invalid(String),
    NotAPreset(String), BuiltinPreset, NothingToUndo }
pub struct Note { pub code: NoteCode, pub message: String }   // NoteCode: Damaged, Newer, TooLarge, Dropped { count }
```

Error messages follow the design guide §10 (what happened, what to do): "At most 8 widgets fit in the popup; remove one
first." The crate has no `tauri` feature: the commands stay in the app, which owns where settings live (in Dyno, the
agent). A second app with the same command shape is the point to add one (rule of three).

### 3. The TypeScript contract is generated (ADR 0026 §3)

- `paddock-widgets` derives `ts_rs::TS` on every wire type (generics included) and its `generated.rs` test writes
  `ui/projects/paddock-ui/widgets/generated/widgets.ts` through `paddock_commands::testing::sync_generated` (stale file
  fails; `PADDOCK_WRITE_TS=1 cargo test -p paddock-widgets --lib generated` rewrites it), exactly like `paddock-window`.
- The app's catalogue types (Dyno: `PopoverWidget`, `PopoverWidgetKind`, `PopoverValue`, `DashboardWidget`,
  `MenuBarItem` and their kinds) derive `TS` in the app's core crate and are generated the same way into
  `apps/dyno/app/src/app/core/generated/widgets.ts`; the hand-written mirrors in `api.types.ts` (`TrayItem`, `TrayItems`,
  `DashboardLayout`, `Widget`, `WidgetMetric`...) are deleted.
- The shared UI is generic over the app's types: `Placement<W>`, `KindInfo<K>`, `WidgetState<V>`.

### 4. The shared UI: `@decryp7/paddock-ui/widgets`

A new secondary entry point (`ui/projects/paddock-ui/widgets/`), standalone OnPush components, tokens only, Material and
CDK (rule 47):

- **`<pd-widget-area>`**: the widgets of a layout in order on the surface's grid (2 equal columns, 159 px in the popover, gap
  8 px, the popover's 16 px inset). Inputs: `placements`, `catalogue` (`CatalogueInfo`), `states` (a map from placement
  id to `WidgetState`; a missing entry means loading). The app supplies one template per kind with the structural
  directive `*pdWidget="'cpuMemory'; let value"`; the area draws everything else. An empty layout shows a designed empty
  state ("No widgets in the popup" + "Choose widgets", which emits `customize`).
- **`<pd-widget-frame>`** (used by the area, public for the dashboard): the widget's head (14 px icon, 12/500 muted
  title, an optional right-aligned meta such as "3 volumes") and its body in one of four designed states:
  - *Loading*: `pd-skeleton` blocks at the ready state's final height (no layout shift), shimmer off in reduced motion.
    `--pd-skeleton` is the surface-2 colour, which vanishes on a widget's own surface-2 fill; the mock-up uses text at
    9 % instead, and WP-B adds that as a token (`--pd-skeleton-raised`) with its test rather than a literal.
  - *Ready*: the app's template. When `stale` is set, the meta shows "Updated 4 min ago" in subtle text.
  - *Empty*: 16 px muted icon, a 13/600 title, one muted line, and the action as a text button ("Pin counters"); a
    calm empty (no active alarms) shows the success icon and its word instead and the whole row opens the page.
  - *Error*: danger icon and title in plain words, the cause in muted text, Retry (text button) when `retry` is set,
    emitting `retry(id)`; never raw exception text as the title.
  A frame that navigates is a real `button` (the row or tile), with a chevron on hover and focus.
- **`<pd-widget-chooser>`**: the editor for one surface, used in the app's Settings window (a popover closes on blur and
  is too small for editing). It emits `Edit` values; the app sends each to the engine and feeds the returned
  `LayoutView` back, so the chooser holds no layout logic and the popover updates live through the app's event.
  - **Shown**: a `cdkDropList` of rows (`role="list"`). Each row: a drag handle that is also the keyboard reorder
    control, the kind's icon and title, its summary or option (Material `mat-select` for the volume, a
    `mat-button-toggle-group` "Half / Full" where the kind allows both spans), and a `mat-menu` (⋯) with Move up, Move
    down, Move to top, Move to bottom and Remove. No separate up and down buttons.
  - **Keyboard reorder**: on the handle (aria-label "Reorder CPU and memory, position 2 of 3", described by the hint),
    Alt+Up and Alt+Down (Option on macOS) move the row, Alt+Home and Alt+End to the ends; focus stays on the moved
    handle and the CDK `LiveAnnouncer` says "CPU and memory moved to position 1 of 3". The hint under the list says so.
  - **Add a widget**: the catalogue's other kinds with their summary and an outlined "Add" button; a kind at its limit
    shows "Added" (disabled, with the check icon); at the surface limit every Add is disabled and the counter says
    "8 of 8: remove one to add another".
  - **Reset to defaults** (text button) and the undo of every remove, preset and reset as a Material snack bar with
    Undo (the family toast area), which sends `Edit::Undo`.
  - Beside the lists at widths of 760 px and more (container query), a live preview: the real `pd-widget-area` inside
    a `pd-tray-popover` frame, fed the same states as the popup.
- Gallery: a "Widgets" page with the area in every state, the chooser with a sample catalogue, both themes, all
  densities; its Playwright spec checks sizes, focus order, the keyboard reorder and axe.

### 5. Dyno adopts it in 1.0 (no backward compatibility: Dyno 1.0 is unreleased)

- **Three catalogues** in `dyno-core/src/widgets/`: `popover.rs` (new), `dashboard.rs` (Dyno's `WidgetKind`, `Metric`,
  thresholds and presets as a `Widget` impl; `accepts()` stays Dyno's domain rule inside `validate`), `menu_bar.rs` (the
  menu-bar items, `TrayItemStyle` as the widget's options). `DashboardLayout`, `TrayItems`, `Tray`, `TrayMetric`,
  `TrayStyle`, the `widgets.rs` layout code and `features/widgets/widget-model.ts`'s catalogue, limits, `accepts`,
  `applyPreset` and `moveWidget` are deleted; the settings fields become `dashboard: Stored<DashboardCatalogue>`,
  `menu_bar: Stored<MenuBarCatalogue>`, `popover: Stored<PopoverCatalogue>` (the old `tray` and `tray_items` keys are
  dropped outright: a settings file from a pre-release build loses its menu-bar and dashboard choices once, with the
  usual note).
- **The popup catalogue:**

  | Kind | Title | Spans | Max | Values from (existing engine state) | Refresh | Empty | Error |
  |---|---|---|---|---|---|---|---|
  | `activeAlarms` | Active alarms | 2 | 1 | the alarm store's open alarms, most severe then newest, 3 rows and "+N more" | on change | calm: "No active alarms" (success icon), row opens Alarms | "Couldn't read alarms" + Retry |
  | `diskSpace { volume: all \| system \| mount(id) }` | Disk space | 2 | 4 | the sampler's volume list (cached, never a fresh `statfs` on open), 4 volumes at most and "+N more" | every 60 s | "Backup disk isn't connected" + "Choose another volume" | "Couldn't read volumes" + Retry |
  | `cpuMemory` | CPU and memory | 2 | 1 | the live update (load, used of total, last 60 s) | live | none (no sample yet is loading) | "Agent stopped" + Restart agent (rule 19) |
  | `network` | Network | 1, 2 | 1 | the live update (down and up rates, last 60 s) | live | "Not connected" + Open network | as CPU |
  | `backupHealth` | Backup | 1, 2 | 1 | the backup check's last report (never runs `tmutil` or `wbadmin` on open) | on change | "No backups found" + "Set up backup checks" | "Couldn't check backups", cause, Retry (starts the check job) |
  | `lastIssue` | Last issue | 2 | 1 | the newest issue of the last 7 days | on change | "No issues this week" | "Couldn't read issues" + Retry |
  | `pinnedCounters` | Pinned counters | 2 | 1 | the pinned counters' latest readings, 4 rows | live | "No pinned counters" + "Pin counters" | per row "Unavailable", the widget only when all fail |
  | `topApps` | Top apps | 2 | 1 | the live update's top CPU list, 5 rows | live | none | as CPU |

  Defaults: Active alarms, Disk space (all volumes), CPU and memory, in that order (the user's list). `topApps` keeps
  today's list reachable; it is not in the defaults. Popup limit: 8 widgets; at most one of each kind except disk space.
- **Values come from the engine.** The agent computes a `PopoverValue` per placement from state it already holds and
  pushes `Vec<PlacementState<PopoverValue>>` on `dyno://popover-values` at the live tick, **only while the popover is
  visible** (paddock-window knows); `popover_values()` returns the same once on open, so the first paint has values.
  Sparkline history keeps today's path (`history('hour')` seed, then the live store). The UI formats; it does not decide
  a severity, a threshold, staleness or an order.
- **Commands** (Dyno's, async, tray class allowed only the read ones): `widget_layout(surface)`,
  `edit_widget_layout(surface, edit) -> LayoutView`, `popover_values()`, `run_widget_action(id)` (Retry, open a page,
  restart the agent); event `dyno://widget-layout { surface }` after every edit.
- **The popup**: `features/tray/tray.ts` keeps its header (health pill, agent state) and footer (Open Dyno, Quit) and
  gains a "Customize widgets" icon button in the footer (opens Settings at Menu bar › Popup widgets); its content
  becomes `<pd-widget-area>` with Dyno's kind templates in `features/tray/widgets/`.
- **The chooser**: Settings › Menu bar gets a "Popup widgets" section with `<pd-widget-chooser>`; the existing menu-bar
  items editor and the dashboard's edit mode move onto the same edits and catalogue info (their look stays Dyno's).
- **Register**: `widget-layouts` moves to `delete_by = "dyno-v1.0.0"` (the user's decision moves it earlier, so no date
  trailer is needed while the entry is not yet on main) and lists `features/widgets/widget-model.ts` as the third copy.
  WP-C deletes all three copies and removes the entry in the same change; the Dyno tag is refused until then (rule 55 (d)).

### 6. Limits

Popup: 8 placements, one per kind except disk space (4), 4 volumes per disk widget, 3 alarm rows, 4 counter rows, 5 app
rows; dashboard: 32 placements, 8 user presets of at most 24; menu bar: 4 items (fewer when the OS reports fewer); ids 64
characters, preset names 64, a layout 128 KiB, at most `2 × max_placements` entries looked at when reading (the rest are
dropped unread with the count). The popover itself keeps its own limits (360 × 520, content scrolls past 520).

### 7. States and accessibility

- Every kind's four states are designed (mock-ups: `docs/design/tray-widgets/renders/states-*.png`); status is never colour
  alone (icon and word; alarm severity as the family badge); accent is used only for the primary button, the CPU hero
  sparkline and focus.
- The popup is a dialog (`pd-tray-popover`); inside, widgets are `section`s labelled by their titles, in layout order,
  which is the focus order; a widget that opens a page is one `button` with an accessible name that includes its value
  ("Disk space: Macintosh HD, 182 GB free of 994 GB. Open disk map"). Live numbers are not live regions (they would talk
  every second); the alarms widget announces a change of its count politely.
- The chooser: real buttons, labelled handles, Material menus and selects with CDK focus handling, keyboard reorder
  with announcements (§4), hit targets of 24 px or more, the focus ring on every control.
- Reduced motion: no shimmer, no drop animation (CDK drag preview only), state changes instant.

### 8. Parallelism and cost

- Nothing new is long or bulk. The popup's values are read from state the agent already keeps; no widget starts an OS
  tool, a disk walk or a network call on open or on a tick. Backup and issues are refreshed by their existing jobs and
  pollers (ADR 0020 executor), at their own priority (rule 52: the live tick is *live* work).
- Cost per tick while the popover is visible: one `Vec<PlacementState>` of at most 8 entries (under 4 KB of JSON); none
  while it is hidden. Reading a layout is linear in its size, bounded by §6. No thread is added.

## Consequences

- One layout model in the family; Pitwall's and Spotter's popups and any dashboard can adopt it by writing a catalogue.
- Dyno 1.0's tag waits for WP-A to WP-D (the user's decision). Pre-release settings lose their dashboard and menu-bar
  choices once.
- The popover gets a fixed look per kind across apps (frame, states); apps keep their own ready-state templates.
- `@decryp7/paddock-ui` gains a secondary entry point and a CHANGELOG entry; `crates/` gains a member.

## Alternatives considered

- **A popup-only chooser in Dyno.** Fastest, but a fourth copy of the layout model and of its TypeScript mirror; rule 55
  forbids it without a delete-by tag, and the user wants no copy left.
- **Extend `paddock-window`'s popover with widgets.** Mixes window placement with content and settings; the dashboard
  and the menu bar items are not popovers. Roadmap WP-38 already said the host stays as it is.
- **Generic layouts over `serde_json::Value` options** (no `Widget` trait). Simpler generics, but the options would be
  validated in two places and the TypeScript would lose the per-app types.
- **Keep the edit logic in the UI and save the whole layout** (today's tray-items editor). Leaves limits, ids and
  presets in TypeScript, the logic copy this ADR removes; the popover could not update live from another window.
- **Up and down buttons on every row.** Rejected in WP-38: the handle's keyboard reorder plus the row menu covers
  keyboard and screen-reader use without doubling the controls.

## Work packages

Order: WP-A and WP-B start together (the wire shapes are fixed by §2 and §3, so WP-B writes against them and switches to
the generated file when WP-A lands); WP-C starts its Rust half with WP-A's first commit and its UI half when WP-B's
components exist; WP-D writes its specs alongside and runs them last. Each package owns only its files; the shared
integration points (`crates/Cargo.toml` member line, `ui/projects/paddock-ui` package exports and CHANGELOG,
`docs/known-copies.toml`) are a single declared line each, or the integrator's.

### WP-A: `crates/paddock-widgets` (implementer, Sonnet)

- **Goal:** the crate of §1-§3 and §6, nothing app-specific. Use the `new-crate` skill (scaffold marker, workspace hack,
  README entry, coverage, publish test).
- **Owns:** `crates/paddock-widgets/**`, `ui/projects/paddock-ui/widgets/generated/widgets.ts` (generated), one member
  line in `crates/Cargo.toml`, one entry in `crates/README.md`.
- **Build:** `Surface`, `KindSpec`, `Refresh`, `Widget`, `Catalogue`, `PlacementId` (serde `try_from`), `Placement`,
  `Preset`, `Layout`, `Read`/`read`, `Stored` (lenient `Deserialize`, plain `Serialize`), `Edit` and `apply`, one-level
  undo, `CatalogueInfo`/`KindInfo`/`SurfaceInfo` (from a catalogue), `LayoutView`, `WidgetState`, `WidgetAction`,
  `PlacementState`, `WidgetError` (messages per the design guide §10), `Note`/`NoteCode`. Port the behaviour of
  `apps/dyno/crates/dyno-core/src/widgets.rs` (`parse`, `normalised_counting`, `keep_valid`, custom presets) generically;
  do not copy Dyno's kinds or metrics. ts-rs generation as `paddock-window/src/generated.rs`.
- **Tests:** a test catalogue in `tests/` (three kinds, one with options, one with `max_instances` 2): every edit's
  success and each error; limits at and past each bound; read of missing, damaged, non-object, oversized, newer,
  unknown-kind, duplicate-id and over-count input; undo after remove, preset and reset, and `NothingToUndo`; serde round
  trip; a proptest that any JSON gives a valid layout; generated-TS current. Coverage >= 99% lines.
- **Gates:** `cd crates && cargo fmt -p paddock-widgets --check && cargo clippy -p paddock-widgets --all-targets
  --all-features -- -D warnings && cargo test -p paddock-widgets`, own `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`.

### WP-B: `@decryp7/paddock-ui/widgets` and the gallery (implementer, Sonnet)

- **Goal:** §4's `pd-widget-area`, `pd-widget-frame`, the `*pdWidget` directive and `pd-widget-chooser`, generic over the
  generated types, matching `docs/design/tray-widgets/*.png` (tokens only, no literals).
- **Owns:** `ui/projects/paddock-ui/widgets/**` (except `generated/`), the gallery's widgets section
  (`ui/projects/gallery/src/app/widgets-section.ts` and one import line in `gallery.ts`), `ui/e2e/widgets.spec.ts` (or the
  gallery's spec folder), one CHANGELOG entry, the entry point line in the package exports.
- **Build:** reuse `pd-skeleton`, `pd-empty-state`/`pd-error-state` styles, `pd-status-pill`, `pd-sparkline`,
  `pd-tray-popover` (unchanged); `cdkDropList`/`cdkDrag`, `mat-menu`, `mat-select`, `mat-button-toggle-group`,
  `matButton`, `matIconButton`, `MatSnackBar` through the family toast area, `LiveAnnouncer`. Adds the `grip` glyph
  (six filled dots) to `PD_ICON_PATHS`/`PD_ICON_DOTS` and the `--pd-skeleton-raised` token (§4). Keyboard reorder and
  announcements exactly as §4. Until WP-A lands, a temporary `generated/widgets.ts` hand-written to §2's shapes, replaced
  by WP-A's file (same names).
- **Tests:** component specs (Vitest, `await fixture.whenStable()`, CDK harnesses, rule 49): each state renders, a
  missing state is loading, retry and action outputs, empty layout, chooser emits the right `Edit` for drag, Alt+arrow,
  menu items, Add, limits disabled, undo snackbar; gallery Playwright (Chromium) for sizes, focus order and axe in both
  themes. Components at 100% coverage. `npm run test:tokens` stays green.
- **Gates:** `cd ui && npm run lint && npm test && npm run build && npx playwright test --project=gallery`.

### WP-C: Dyno adoption, engine values and copy deletion (implementer, Sonnet; Rust then UI)

- **Goal:** §5: Dyno's three catalogues, the popup values and commands, the popup and chooser UI, and every copy gone.
- **Owns:** `apps/dyno/crates/dyno-core/src/widgets*`, `settings.rs` (the three fields), the agent's popover-values push,
  `apps/dyno/app/src-tauri/src/` (commands, tray class capabilities, `tray_model.rs` reading `menu_bar`),
  `apps/dyno/app/src/app/features/{tray,widgets,settings,dashboard}/**`, `core/api.types.ts` (deletions),
  `core/generated/widgets.ts`, the mock backend's widget data, `docs/known-copies.toml` (remove `widget-layouts` with the
  files), `apps/dyno/docs/roadmap.md` WP-38 and `apps/dyno/docs/adr/0012-*.md` (status: superseded in part by family ADR
  0035), CHANGELOG.
- **Build:** the catalogue table of §5 with its `KindSpec`s and defaults; `PopoverValue` per kind from the agent's
  existing state (alarm store, volume list, live update, backup report, issues, counter readings), pushed only while the
  popover is visible; `run_widget_action` routes to existing commands. Delete the copies listed in §5 and their
  duplicate tests (rule 43: record test count and time before and after).
- **Tests:** each popup kind's value builder for ready, empty and error from synthetic state; the push stops when the
  popover hides; settings with the old keys load defaults with a note; the dashboard and menu bar still render their
  defaults; Dyno's wiring tests through the shared contract (no re-test of shared behaviour).
- **Gates:** `node scripts/affected.mjs origin/main --explain`, Dyno's cargo fmt, clippy and tests with its own
  `CARGO_TARGET_DIR`, `cd apps/dyno/app && npm run lint && npm test`, rule 54: the Dyno release workflow with bundles
  before landing.

### WP-D: end-to-end, screenshots and the real-app smoke (implementer, Sonnet)

- **Goal:** prove the feature works as the person sees it.
- **Owns:** `apps/dyno/app/e2e/tray-widgets.spec.ts`, `e2e/screenshots.spec.ts` additions, `apps/dyno/docs/screenshots/
  tray-widgets-*.png` and `settings-popup-widgets-*.png`, the roadmap's screenshot checklist rows.
- **Build and test:** against the mock backend: choose, reorder by drag and by Alt+arrow, remove and Undo, reset, and the
  popup shows the new order after a reload (persistence through the mock's settings); each kind's empty, loading and
  error state via the mock's switches; axe on the popup and on the chooser in both themes; focus order equals widget
  order. One real-app smoke (rule 43) on macOS: change the order in Settings, the visible popover follows, restart,
  order kept. Screenshots in light and dark, then an independent review (rule 22) before they land.

## Open questions for the coordinator

1. `topApps` in the popup catalogue (today's list, not in the defaults): keep, or drop it from 1.0?
2. The calm "No active alarms" state uses the whole row as its next step (open Alarms) instead of a separate button, to
   keep the popup quiet (design guide §1 "calm by default"); confirm.
