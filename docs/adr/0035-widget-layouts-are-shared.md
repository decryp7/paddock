# 35. Widget layouts are shared: `paddock-widgets` and the `paddock-ui` widget area and chooser

- Status: Accepted (2026-10-31, after independent review rounds 1-2; the user asked for this in Dyno 1.0). Needed by Dyno 1.0, with everything in it: the
  popup widgets, the dashboard and the menu-bar items editor (user, 2026-10-31: "In Dyno 1.0, delay the tag"; no copies
  left). Dyno's tag waits for work packages WP-A to WP-D.
- Date: 2026-10-31
- Related: Dyno ADR 0001 (the app and the agent are separate processes; settings live in the agent), Dyno ADR 0012
  (widgets and tray items are data, not code; this ADR moves its model into a shared crate and replaces its Dyno types),
  Dyno ADR 0009 (native feel), Dyno roadmap WP-38 and WP-21, family ADR 0026 §3 (contracts declared in Rust, TypeScript
  generated with `ts-rs`), ADR 0034 / rule 55 (the copies register, mechanism `widget-layouts`), rules 39, 42 and 43
  (share first, tested once), rule 47 (Material first), rules 10-12 / ER-17 to ER-19 (background work, phase-labelled
  progress, no UI timeouts), rule 51 (parallelism and cost), the design guide §1, §4, §5, §8, §9 and §13, AGENTS.md
  "Where logic lives".
- Mock-ups: `docs/design/tray-widgets/renders/*.png`, light and dark: `popover` (defaults), `popover-more` (the other
  kinds, footer menu), `popover-agent` (agent stopped), `states` (every kind's loading, empty, error, working or stale),
  `chooser` (Settings with the live preview), `chooser-limit` (8 of 8, added kinds, widths), `chooser-actions` (drag
  placeholder, row menu, Undo). Source `mock.html`, rendered by `render.mjs` with Playwright's pinned Chromium.
- Code: the type skeleton of §2 is `crates/paddock-widgets/src/lib.rs` on this branch; WP-A builds on it
  (`feat/paddock-widgets`), with the rulings of §2a.

## Context

The person wants to choose which small widgets Dyno's tray popup shows and in what order: active alarms, disk space per
volume, CPU and memory now, network now, backup health, last issue and pinned counters, with defaults (alarms, disk space,
CPU and memory), persisted, each widget with designed empty, loading and error states, usable from the keyboard and a
screen reader.

What exists:

- **The popover host is shared and stays as it is.** `crates/paddock-window` (`popover.rs`, `tauri/popover.rs`,
  `tray.rs`, `tauri/native_tray.rs`) places, fits and closes the popover window; `pd-tray-popover`
  (`ui/projects/paddock-ui/src/lib/tray-popover.ts`) draws its surface, header, scrolling content (past 520 px) and
  footer. This ADR adds nothing to them: the widget area lives in the popover's content slot.
- **The layout model is Dyno's own, three times.** `dyno-core/src/widgets.rs` (1,082 lines) holds the dashboard layout
  (`WidgetKind`, `WidgetSize`, `WidgetOptions`, `Widget`, `DashboardLayout`, presets) and the menu-bar items
  (`TrayItemStyle`, `TrayItem`, `TrayItems`); `dyno-core/src/settings.rs` holds an older menu-bar model (`TrayMetric`,
  `TrayStyle`, `Tray`); `app/src/app/features/widgets/widget-model.ts` repeats the catalogue (`KIND_LABEL`, `KIND_HINT`,
  `SIZE_COLUMNS`), the limits (`MAX_WIDGETS`, `MAX_TRAY_ITEMS`), the edits (`newWidget`, `newTrayItem`, `moveWidget`,
  `resizeWidget`, `removeWidget`, `applyPreset`, `presetNames`), the compatibility rule (`accepts`) and severity logic
  (`thresholdTone`, `readMetric`) in TypeScript, which "Where logic lives" forbids. All three are registered as copies
  of `widget-layouts` in `docs/known-copies.toml`, delete by `dyno-v1.0.0`.
- **The popover's content is fixed** (`features/tray/tray.ts`): four sparkline tiles and the top five apps.
- **Two processes.** The agent (`dyno-agent`) owns settings, the sampler, the alarm store, the volume list, the backup
  check and the issue index; the app (`app/src-tauri`) talks to it through `dyno-core/src/protocol.rs` (postcard over
  `dyno-ipc`, `PROTOCOL_VERSION` 2) and relays to the webview. The app supervises the agent and restarts it.
- Pitwall's and Spotter's popovers are the next candidates for the same model.

A popup-only chooser in Dyno would be a fourth copy; rule 55 (c) allows a new copy only with a register entry and a
delete-by tag, and the user wants none left. So the model moves to Paddock now, and all three of Dyno's surfaces
(dashboard, menu-bar items, popup) move onto it in 1.0.

## Decision

### 1. One shared crate, `paddock-widgets` (pure, named by the concept)

No I/O, no Tauri, no app types. It owns everything about a layout that is not what a widget shows:

- **Placements and layouts.** A layout is an ordered list of placements on one surface. A placement is an id, a span
  (columns of the surface's grid) and the app's widget value.
- **A closed catalogue per app and surface.** The app implements `Catalogue` per surface: its surface id (one enum per
  app), its widget type (a closed enum whose variants carry the options), each kind's title, summary, icon, spans, count
  limit and refresh, the defaults and the built-in presets. Unknown kinds in a file are dropped with a note, never drawn.
- **Limits** per surface and per kind (§7), checked on every read and every edit.
- **Lenient, versioned reading that never loses data.** A layout never fails its settings file:
  - each invalid, unknown, duplicate or over-limit entry is dropped on its own, with one `Dropped { count }` note;
  - only a layout that is not a layout at all (not an object, wrong shape, version 0, missing or not a number) or too
    large gives the defaults, with one note, and the value is kept unread (written back unchanged) until the person's
    first edit of that surface replaces it; `null` or a missing field gives the defaults without a note;
  - if every entry was dropped, the defaults are used; an honest empty list stays empty when the surface allows 0;
  - a layout written by a **newer** schema version is kept as it was read (`Read::unread`) and written back unchanged;
    the surface shows the defaults, the chooser is read-only (`LayoutView::editable = false`) and every edit is refused
    with `NewerLayout`, so a downgrade never loses the newer layout;
  - a stored preset marked `builtin: true` is skipped (built-ins come from the catalogue); at most `2 × max` entries are
    looked at, the rest are dropped unread and counted.
- **Edits as commands.** `apply(layout, edit)` is pure and validates the result as a whole, in this order: the widget's
  own options, the kind, the span, duplicate ids, the surface total, then the kind's count. Ids are assigned by the crate
  from the kind's kebab-case wire name, with `-2`, `-3` for further instances; user preset ids are unique slugs of the
  name.
- **One level of undo.** `Layout`'s previous placements are private and never serialised. Remove, apply preset and
  reset set them; add, move, resize and replace clear them; save and delete preset leave them. `LayoutView::can_undo`
  reports them; the UI's undo toast is shown while it is true and dismissed when it turns false.
- **The widget state envelope** every surface renders the same way (§2): ready (with its time and, when late, the time it
  went stale), working (phase-labelled progress), empty or error, plus one notice for the whole surface. *Loading* is not
  on the wire: it is the UI's state before the first value arrives.

What a widget shows, its values and its domain rules (Dyno's metrics, thresholds, severity, which metric a ring accepts)
stay in the app's engine.

### 2. API (Rust): the skeleton in `crates/paddock-widgets/src/lib.rs`

The types and traits are committed as a compiling skeleton; this is their shape. WP-A adds the behaviour behind them:

```rust
pub struct PlacementId(String);   // [a-z0-9-]{1,64}; TS `string`; (de)serialised through TryFrom<String>
pub struct PresetId(String);      // same rules, a user preset only
pub struct Surface { noun, schema_version, columns, max_placements, min_placements, max_user_presets,
                     max_preset_placements, max_bytes }
pub enum Refresh { Live, Every { secs }, OnChange }
pub struct KindSpec<K> { kind, title, summary, icon, spans: &[u8], max_instances: u8, refresh }

pub trait Widget: Clone + PartialEq + Debug + Serialize + DeserializeOwned {
    type Kind: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned;
    fn kind(&self) -> Self::Kind;
    fn validate(&self) -> Result<(), String>;
    fn clamped(self) -> Self { self }
}
pub trait Catalogue {
    type SurfaceId: Copy + Eq + Hash + Debug + Serialize + DeserializeOwned;   // one enum per app
    type Widget: Widget;
    const SURFACE_ID: Self::SurfaceId;
    const SURFACE: Surface;
    fn kinds() -> &'static [KindSpec<<Self::Widget as Widget>::Kind>];
    fn defaults() -> Vec<Placement<Self::Widget>>;
    fn presets() -> Vec<Preset<Self::Widget>> { Vec::new() }
}

pub struct Placement<W> { id: PlacementId, span: u8, widget: W }
pub struct Preset<W> { id: PresetId, name: String, builtin: bool, placements: Vec<Placement<W>> }
pub struct Layout<W> { version: u32, placements: Vec<Placement<W>>, presets: Vec<Preset<W>> /* user only */, previous /* private */ }
pub enum NoteCode { Damaged, TooLarge, Newer { version }, Dropped { count } }   // data only where the case has data
pub struct Note { code: NoteCode, message: String }
pub enum Edit<W> { Add { widget, span, at }, Remove { id }, Move { id, to }, Resize { id, span }, Replace { id, widget },
                   ApplyPreset { id: PresetId }, SavePreset { name }, DeletePreset { id: PresetId }, ResetToDefaults, Undo }
pub enum Counted { Placements, UserPresets, PresetPlacements }                   // what a limit counts, typed
pub enum WidgetError { UnknownPlacement, UnknownPreset, DuplicateId, TooMany { counted, max }, KindLimit { max },
                       SpanNotAllowed { allowed }, TooFew { min }, Invalid(String), BuiltinPreset, NothingToUndo, NewerLayout }
                       // serialised camelCase, externally tagged; Display says what to do (design guide §10)

pub struct SurfaceInfo<S> { id: S, noun, columns, max_placements, min_placements, max_user_presets }
pub struct KindInfo<K> { kind: K, title, summary, icon, spans, max_instances, refresh }
pub struct CatalogueInfo<S, K> { surface: SurfaceInfo<S>, kinds: Vec<KindInfo<K>> }
pub struct LayoutView<S, W, K> { layout, catalogue, builtin_presets: Vec<Preset<W>>, notes, can_undo: bool, editable: bool }

pub struct WidgetAction<A> { action: A /* the app's closed action enum */, label: String /* verb + object */ }
pub enum Progress { Count { done, unit }, Fraction { done, total, unit } }        // ER-18: a growing count, never a static 0
pub enum WidgetState<V, A> {                                                     // TS: tagged by "state"
    Ready   { value: V, as_of: i64, stale_since: Option<i64> },                  // ms since the epoch
    Working { title: String, phase: String, progress: Progress, last: Option<V>, cancel: Option<WidgetAction<A>> },
    Empty   { title: String, detail: String, next: Option<WidgetAction<A>> },
    Error   { title: String, cause: Option<String>, next: Option<WidgetAction<A>> },
}
pub struct PlacementState<V, A> { id: PlacementId, state: WidgetState<V, A> }
pub enum Tone { Info, Warning, Danger }
pub struct SurfaceNotice<A> { tone, title, detail, action: Option<WidgetAction<A>> }
pub struct SurfaceValues<V, A> { notice: Option<SurfaceNotice<A>>, placements: Vec<PlacementState<V, A>> }

pub struct Read<C> { layout, notes, unread: Option<serde_json::Value> }
pub struct Stored<C> { read: Read<C> }   // a settings field: Default, Serialize (writes `unread` back if present),
                                         // lenient Deserialize through read::<C>, never an error
pub fn read<C: Catalogue>(value: &serde_json::Value) -> Read<C>;
pub fn apply<C: Catalogue>(layout: &Layout<C::Widget>, edit: Edit<C::Widget>) -> Result<Layout<C::Widget>, WidgetError>;
pub fn info<C: Catalogue>() -> CatalogueInfo<C::SurfaceId, Kind<C>>;
pub fn view<C: Catalogue>(stored: &Stored<C>) -> LayoutView<C::SurfaceId, C::Widget, Kind<C>>;
```

Rules the types carry: a busy source is `Working` or a stale `Ready`, never an `Error`; an `Error` is never the place
for a problem of the whole surface (that is the `SurfaceNotice`); `Empty::title` starts with what the person gains
(design guide §10). The crate has no `tauri` feature: the commands stay in the app, which owns where settings live.

### 2a. Rulings on WP-A's open points (2026-10-31, after round 1)

WP-A (`feat/paddock-widgets`) made decisions this ADR left open; these are binding:

1. `LayoutView::builtin_presets` is part of the view (confirmed).
2. Apps edit through `Stored::edit(edit) -> Result<LayoutView, WidgetError>` and read `Stored::editable()`; only `Stored`
   knows whether a newer layout is held, so it refuses with `NewerLayout`. `apply` stays public as the pure step for
   tests and for `Stored`, and is never called by an app directly.
3. A value kept unread is written back as the same JSON value it was read as (structure and values, not bytes: the
   settings file is rewritten as a whole and its formatting is the writer's).
4. Undo: remove, apply preset and reset set it; add, move, resize and replace clear it; save preset and delete preset
   leave it (they do not change the placements, which is all that undo restores). The undo toast follows `can_undo`.
5. Wire shapes (serde's defaults, camelCase): `Edit` and `NoteCode` are externally tagged, a unit variant is a string
   (`{"add":{"widget":…,"span":null,"at":null}}`, `"resetToDefaults"`, `{"newer":{"version":3}}`, `"damaged"`);
   `Note` is nested `{ "code": NoteCode, "message": string }`; `WidgetState` is internally tagged by `"state"`
   (`{"state":"ready","value":…,"asOf":…,"staleSince":null}`); `WidgetError` is externally tagged like `Edit`. WP-B and
   WP-C use the generated TypeScript, never hand-written shapes.
6. `WidgetError` derives `Serialize` and `TS`; its messages name what a limit counts through `Counted::noun()`.
7. `Read` and `Stored` implement `Clone`, `Debug` and `PartialEq` by hand, so a catalogue type needs no derives.
8. Messages name the surface and count correctly: `Surface` gains `noun: &'static str` ("popup", "dashboard", "menu
   bar"), carried in `SurfaceInfo` and in the error variants that mention a limit; counts use singular and plural ("At
   least 1 widget is needed in the menu bar.", "At most 8 widgets fit in the popup; remove one first.").
9. Convenience constructors and `assign_ids` (ids from the kind's kebab wire name plus `-2`, `-3`): confirmed.
10. `info::<C>()`, `view(&Stored<C>)` and `builtin_presets::<C>()`: confirmed.
11. The reading and drop rules, the check order, defaults when every entry was dropped, and the `2 × max` scan of §1:
    confirmed.
12. Nothing is lost silently: **any** value that could not be read cleanly (damaged, too large or newer) is kept unread
    and written back unchanged. A newer one stays until the app is updated (edits refused). A damaged or too-large one
    stays until the person's first edit of that surface, which replaces it; its note says so in neutral words built from the
    surface noun, never an app name ("The popup layout could not be read. The defaults are shown, and the old layout is
    kept until you change this one."). The size is bounded by the
    settings file's own limit.

### 3. The TypeScript contract is generated (ADR 0026 §3)

- Every wire type derives `ts_rs::TS` under `cfg_attr(any(test, feature = "ts"))`; ids and times are `string` and
  `number` (no `bigint`). `paddock-widgets`' `generated.rs` test writes
  `ui/projects/paddock-ui/widgets/generated/widgets.ts` through `paddock_commands::testing::sync_generated` (stale fails;
  `PADDOCK_WRITE_TS=1 cargo test -p paddock-widgets --lib generated` rewrites it), as `paddock-window` does.
- The `ts` feature lets an app derive `TS` on its own catalogue types against these: Dyno's `DynoSurface`,
  `PopoverWidget` and kind, `PopoverValue`, `WidgetActionId`, `DashboardWidget`, `MenuBarItem` and their kinds are
  generated into `apps/dyno/app/src/app/core/generated/widgets.ts`. Dyno's hand-written mirrors in `api.types.ts`
  (`TrayItem`, `TrayItems`, `DashboardLayout`, `Widget`, `WidgetMetric`, ...) are deleted.

### 4. The shared UI: `@decryp7/paddock-ui/widgets`

A new secondary entry point (`ui/projects/paddock-ui/widgets/`), standalone OnPush components, tokens only, Material and
CDK (rule 47):

- **`<pd-widget-area>`**: a layout's widgets in order on the surface's grid (popup: 2 equal columns of 159 px, gap 8 px,
  the popover's 16 px inset). Inputs: `placements`, `catalogue`, `values` (`SurfaceValues`; a placement without a state
  is loading). The app supplies one template per kind with `*pdWidget="'cpuMemory'; let value"`. A `notice` is drawn once,
  above the widgets, with its action button; the widgets keep their last values, marked stale, and the content scrolls
  past 520 px as usual. An empty layout shows "Choose what the popup shows" with "Choose widgets" (emits `customize`).
- **`<pd-widget-frame>`**: no border inside the bordered popover; one token step instead (`--pd-surface-2` on
  `--pd-surface-3`), radius 10, padding 8 / 12. In high-contrast mode (`data-contrast="more"`, the OS setting) the
  frame gets a 1 px `--pd-border-strong` outline, checked by WP-B's token test. The head: 14 px icon, 12/500 muted title, an optional right-aligned meta
  ("3 volumes"; stale: a clock icon and the one stale wording, relative "Updated 4 min ago", or "As of 14:28" for a
  live kind whose source stopped; in a half-width frame the meta shortens to the clock and the time, with the full
  wording in its tooltip and accessible name). The body, by state:
  - *Loading*: the ready template rendered with no value and its parts marked `pdSkeleton` drawn as
    `--pd-skeleton-raised` blocks with the `--pd-skeleton-raised-shine` shimmer (off in reduced motion), so the loading
    height **is** the ready height; a WP-B test asserts equal heights for every kind of the sample catalogue.
  - *Ready*: the app's template.
  - *Working*: a 16 px refresh icon, the title, "phase · progress" (a growing count when the total is unknown), a 4 px
    progress bar (indeterminate for a count), the last value under it when there is one; no timeout (ER-19).
  - *Empty*: 16 px muted icon that never repeats the head's icon (`info`, or one with its own meaning such as `plug`
    for a disconnected disk), 13/600 title starting with the benefit, one muted line, the next step as a text button.
    Exception: a calm empty that means "all good" (Dyno: no active alarms, no issues) is one real `button` row (the engine
    decides: `WidgetState::Empty { calm: true }`, false by default; an app binds nothing in the UI): success
    icon, neutral text, relative time or range, chevron, accessible name such as "No active alarms. All clear.
    Open alarms".
  - *Error*: danger icon, the title in plain words, the cause muted, the next step (Retry with its icon) as a text
    button; never raw exception text as the title.
  A frame or row that opens a page is one real `button` with a chevron.
- **`<pd-widget-chooser>`**: the editor for one surface, used in the app's Settings window. It emits `Edit` values; the
  app sends each to the engine and feeds the returned `LayoutView` back, so the chooser holds no layout logic.
  - **Shown** (`cdkDropList`, `role="list"`): each row has a drag handle that is also the keyboard reorder control, the
    kind's icon, title and summary, its option (`mat-select` for the volume, `mat-button-toggle-group` Half / Full for a
    kind with two spans) and a `mat-menu` (⋯): Move up (⌥↑), Move down (⌥↓), Move to top (⌥Home), Move to bottom
    (⌥End), each with its icon, then Remove. The drop placeholder is the full row (accent tint, dashed accent outline).
    The menu opens below or above its button and never covers the button's focus ring.
  - **Keyboard reorder**: on the handle (aria-label "Reorder Disk space, position 2 of 4", described by the hint), Alt
    (Option on macOS) with Up, Down, Home and End move the row; focus stays on the moved handle; the CDK `LiveAnnouncer`
    says "Disk space moved to position 1 of 4". The hint under the list says so. No separate up and down buttons.
  - **Add a widget**: every kind, with its summary; "Add" (outlined), "Add volume" for a kind that can be placed again,
    and "Added" (a check and muted text, not a button) for a kind at its count. At the surface limit the counter says
    "8 of 8", an info line says "The popup holds 8 widgets. Remove one to add another." and every Add is disabled.
  - **Reset to defaults** (text button). Remove, preset and reset show a Material snack bar with Undo through the family
    toast area while `can_undo` is true; it is dismissed when `can_undo` turns false.
  - **Preview** beside the lists when the section is 880 px wide (the family form width; container query): the real `pd-tray-popover` at 360 px
    with the same `pd-widget-area` and the same values as the popup, scrolling past 520 px like the popup.
  - When `editable` is false (a newer layout), the lists are read-only and the note explains why.
- **Gallery**: a "Widgets" page with the frame in every state, the area with a notice, the chooser with a sample
  catalogue, both themes, all densities; its Playwright spec checks sizes, focus order, keyboard reorder and axe.

**Tokens** (in `_tokens.scss`, measured by `npm run test:tokens`, both added in this branch):
`--pd-meter-neutral` (dark #7c8593, light #6b7381; 3:1 or more on surface-1, -2 and -3 and against its track, the chart
grid over surface-2 and -3: lowest 3.19 dark, 3.47 light) for a meter's neutral fill (`--pd-chart-neutral-bar` measured 2.8:1 on the widget fill in dark, and stays as it is for charts);
`--pd-skeleton-raised` and `--pd-skeleton-raised-shine` (dark #2c3037 / #363b44, light #dadde2 / #e6e8ec), because
`--pd-skeleton` is the surface-2 colour and vanishes on a widget.

**Design guide amendment** (WP-B, in the same change): §8 gains "Widget area and frame" (the grid, the fill step, the
padding, the head, the five states and the notice, from §4 above) and "Chooser"; "Tray popups" changes to: header (icon,
name, one status pill), the widget area (the user's layout; a needs-attention notice above the widgets, which keep their stale values and
scroll), the footer
with one primary "Open <App>" and one ⋯ icon button whose `mat-menu` holds "Customize widgets…", "Settings…" and
"Quit <App>"; "Skeletons" names the raised pair; §4 lists `--pd-meter-neutral` and the skeleton pair.

### 5. Dyno adopts it in 1.0 (no backward compatibility: Dyno 1.0 is unreleased)

**Process boundary.** Settings, and with them the three layouts, live in the agent (Dyno ADR 0001). The webview never
talks to the agent; the app's Rust side relays.

- **Protocol** (`dyno-core/src/protocol.rs`): new requests `WidgetLayout { surface: DynoSurface }` → `LayoutView`,
  `EditWidgetLayout { surface, edit }` → `Result<LayoutView, WidgetError>`, `PopoverValues { placements }` →
  `Vec<PlacementState<PopoverValue, WidgetActionId>>` for the kinds that are not live, and the pushed event
  `WidgetLayoutChanged { surface }`. `Settings` loses `tray` and `tray_items` and gains `dashboard`, `menu_bar` and
  `popover` as `Stored<…>`; postcard is positional, so this is an incompatible change: **`PROTOCOL_VERSION` 2 → 3**.
- **Agent** (`dyno-agent`): handles the requests from the stored layouts; computes the slow kinds' values from state it
  already keeps (alarm store, volume list, backup report, issue index, counter readings), from caches only.
- **App** (`app/src-tauri`): builds the popup's `SurfaceValues` itself. Live kinds (CPU and memory, network, top apps,
  pinned counters) come from the agent's live push it already subscribes to; slow kinds from `PopoverValues`, asked on
  open and on the agent's change events. It emits `dyno://popover-values` to the popup **only while the popover is
  visible** (paddock-window reports it). When the agent connection is lost it synthesises the surface notice "Agent
  stopped · Readings paused at 14:28" with Restart agent, and marks every placement's last value `stale_since` (the
  frame says "As of 14:28" for live kinds and "Updated 2 min ago" for the others). Restart
  agent goes to the app's own supervisor, never through the agent.
- **Value builders are one pure module** (`dyno-core/src/widgets/popover_values.rs`): `fn state(kind, inputs) ->
  WidgetState<…>` for every kind, used by the app (live kinds) and the agent (slow kinds); severity, thresholds
  (`thresholdTone`, `readMetric` move here from TypeScript), ordering and staleness are decided here, never in the UI.
- **Actions** are a closed enum `WidgetActionId` (`OpenPage(Page)`, `Retry(PlacementId)`, `ChooseVolume`, `PinCounters`,
  `SetUpBackupChecks`, `StartBackupCheck`, `RestartAgent`, `CustomizeWidgets`), run by `run_widget_action`.

**The popup catalogue** (copy as in the mock-ups; empty titles start with the benefit):

| Kind | Title | Spans | Max | Values from | Refresh | Empty | Error | Working or stale |
|---|---|---|---|---|---|---|---|---|
| `activeAlarms` | Active alarms | 2 | 1 | alarm store, most severe then newest, **2 rows** and "+N more" | on change | calm row: "No active alarms · All clear." (opens Alarms) | "Couldn't read alarms" / "The alarm store didn't answer." / Retry | stale: "Updated 4 min ago" |
| `diskSpace { volume: all \| system \| mount(id) }` | Disk space | 2 | 4 | the agent's volume list (cached; never a fresh `statfs` on open), 3 volumes and "+N more" | every 60 s | "See Time Machine's space when it's connected" / "The disk isn't connected now." / Choose another volume | "Couldn't read volumes" / "The volume list didn't answer." / Retry | stale; a network volume that doesn't answer shows "Not answering" in its row |
| `cpuMemory` | CPU and memory | 2 | 1 | live push (load, used of total, last minute) | live | none (no sample yet is loading) | none (agent: the notice) | "As of 14:28" |
| `network` | Network | 1, 2 | 1 | live push (down and up, last minute) | live | "See traffic when online" / "Offline now." / Open network | none (agent: the notice) | "As of 14:28" |
| `backupHealth` | Backup | 1, 2 | 1 | the backup check's last report (never runs `tmutil` or `wbadmin` on open) | on change | "Know when backups ran" / "No backup set up." / Set up checks | "Couldn't check" / "tmutil failed." / Retry (starts the check job) | working: "Checking backup · N items so far" (no tool name and no "2 of 3": it fits two lines in the half-width frame; the tool shows in the normal state) |
| `lastIssue` | Last issue | 2 | 1 | the newest issue of the last 7 days | on change | calm row: "No issues in the last 7 days" (opens Issues) | "Couldn't read issues" / "The issue index is unavailable." / Retry | working: "Reading system logs · Indexing · 12,400 events so far" |
| `pinnedCounters` | Pinned counters | 2 | 1 | the pinned counters' latest readings, 4 rows (not in the live push: a slow kind the app asks the agent for every 5 s while the popup is visible, from the counters' kept readings, never a fresh OS read) | live | "Watch your own counters here" / "Pin counters in Dyno; the first 4 show here." / Pin counters | "Couldn't read counters" / "None of the 3 pinned counters answered." / Retry (one row: "Unavailable") | stale |
| `topApps` | Top apps | 2 | 1 | live push, top CPU, 3 rows | live | none | none (agent: the notice) | "As of 14:28" |

Defaults: Active alarms, Disk space (all volumes), CPU and memory, in that order; the popup is 475 px tall with them, so
it does not scroll. Popup limit: 8 widgets.

**The popup** (`features/tray/tray.ts`): header unchanged (health pill, agent state); content `<pd-widget-area>` with
Dyno's kind templates in `features/tray/widgets/`; footer "Open Dyno" and one ⋯ button with "Customize widgets…" (opens
Settings at Menu bar › Popup widgets), "Settings…" and "Quit Dyno".

**Settings and the dashboard**: Settings › Menu bar gets "Popup widgets" (`pd-widget-chooser`) and its menu-bar items
editor moves onto the same edits and catalogue info; the dashboard's edit mode moves onto them too (its grid look stays
Dyno's). Both are in 1.0.

**Register**: `widget-layouts` lists `widgets.rs`, the settings types and `widget-model.ts` (with every symbol in the
Context above), all delete by `dyno-v1.0.0`. WP-C1 and WP-C2 delete them and remove the entry in the same change; the
Dyno tag is refused until then (rule 55 (d)).

### 6. Accessibility

- Status is never colour alone (icon and word; alarm severity as the family badge); accent only for the primary button,
  the CPU hero sparkline, the working bar and focus.
- The popup is a dialog (`pd-tray-popover`); widgets are `section`s labelled by their titles, in layout order, which is
  the focus order; a widget or row that opens a page is one `button` whose name includes its value. Live numbers are not
  live regions; the alarms widget announces a change of its count politely; the notice is `role="alert"` once.
- The chooser: labelled handles with keyboard reorder and announcements, Material menus and selects with CDK focus
  handling, hit targets of 24 px or more (handle 24 × 32), the focus ring on every control and never covered.
- Reduced motion: no shimmer, no drop animation (the CDK preview only), state changes instant.

### 7. Limits

Popup: 8 placements, one per kind except disk space (4), 3 volumes per disk widget, 2 alarm rows, 4 counter rows, 3 app
rows. Dashboard: 32 placements, 8 user presets of at most 24. Menu bar: 4 items (fewer when the OS reports fewer). Ids
and preset names 64 characters, a layout 128 KiB, at most `2 × max_placements` entries looked at when reading.

### 8. Parallelism and cost

- Nothing new is long or bulk. The popup's values come from state the agent already keeps; no widget starts an OS tool, a
  disk walk or a network call on open or on a tick. Backup and issues refresh through their existing jobs (ADR 0020),
  and their progress reaches the widget as `Working`.
- While the popover is visible: one `SurfaceValues` of at most 8 entries per live tick (under 4 KB of JSON) and one
  `PopoverValues` request on open and per change event; nothing while it is hidden. The live tick is live work (rule 52).
  Reading a layout is linear and bounded by §7. No thread is added.

## Consequences

- One layout model in the family; Pitwall's and Spotter's popups adopt it by writing a catalogue.
- Dyno 1.0's tag waits for WP-A to WP-D. Pre-release settings lose their dashboard and menu-bar choices once (with a
  note); agent and app must match (`PROTOCOL_VERSION` 3).
- `@decryp7/paddock-ui` gains a secondary entry point, three tokens and a CHANGELOG entry; `crates/` gains a member.

## Alternatives considered

- **A popup-only chooser in Dyno.** Fastest, but a fourth copy; rule 55 forbids it without a delete-by tag.
- **Extend `paddock-window`'s popover with widgets.** Mixes window placement with content and settings; the dashboard and
  the menu-bar items are not popovers.
- **Generic layouts over `serde_json::Value` options.** Options validated twice and the per-app TypeScript lost.
- **Edit logic in the UI, save the whole layout** (today's editors). Keeps the TypeScript copy and cannot update the
  popup live from another window.
- **The agent pushes popup values itself.** It would have to know the popover's visibility and could not say "agent
  stopped"; the app already holds the live subscription and the supervisor.
- **One error per widget when the agent stops.** Eight identical errors; one notice says it once.
- **A flat `Note { code, count, version, message }`.** Fields that can disagree with the code; the enum carries data only
  where the case has it.
- **Up and down buttons on every row.** The handle's keyboard reorder plus the row menu covers keyboard and
  screen-reader use without doubling the controls.

## Work packages

Order: WP-A lands a type-only commit first (the skeleton on this branch), so WP-B and WP-C1 build against real types;
then WP-A's behaviour, WP-B and WP-C1 in parallel; WP-C2 when WP-B's components exist; WP-D last. WP-C1 and WP-C2 land
on main as **one merge** (the protocol, settings and UI change together, and neither half works alone), and rule 54's
installer build (Dyno's release workflow with bundles) runs on that combined landing, not on either half. Shared integration
points (`crates/Cargo.toml` member line, `ui/projects/paddock-ui` exports and CHANGELOG, `docs/known-copies.toml`) are one
declared line each, or the integrator's.

### WP-A: `crates/paddock-widgets` (implementer, Sonnet; in progress on `feat/paddock-widgets`)

- **Owns:** `crates/paddock-widgets/**`, `ui/projects/paddock-ui/widgets/generated/widgets.ts`, the member line, the
  `crates/README.md` entry.
- **Build:** §1-§3 and §7 behind the skeleton's types: `read`, `Stored` (lenient `Deserialize`, `Serialize` that writes
  `unread` back), `apply` with the check order and id rules of §1, undo as in §1, `info`, `view`, the `ts` feature and
  `generated.rs`. Remove the scaffold marker at the end.
- **Tests:** a test catalogue (three kinds, one with options, one with `max_instances` 2): every edit's success and
  each error; every limit at and past its bound; reading `null`, missing, damaged, non-object, version 0, oversized,
  newer (kept, written back unchanged, edits refused), unknown kind, duplicate id, over-count, a stored built-in preset;
  undo after remove, preset and reset, cleared by add, move, resize and replace; serde round trip; a proptest that any
  JSON gives a valid layout; generated TS current with no `bigint`. Coverage >= 99% lines.
- **Gates:** `cargo fmt -p paddock-widgets --check`, `cargo clippy -p paddock-widgets --all-targets --all-features -- -D
  warnings`, `cargo test -p paddock-widgets`, own `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`.

### WP-B: `@decryp7/paddock-ui/widgets`, the gallery and the design guide (implementer, Sonnet)

- **Owns:** `ui/projects/paddock-ui/widgets/**` (except `generated/`), `ui/projects/gallery/src/app/widgets-section.ts`
  and its import line, the gallery e2e spec for it, the `grip` glyph in `ui/projects/paddock-ui/icons/src/paths.ts`
  (six filled dots), `docs/design-guide.md` (the §4 and §8 amendments above), one CHANGELOG entry, the export line.
- **Build:** §4 exactly, matching the mock-ups, tokens only. Reuse `pd-tray-popover` (unchanged), `pd-status-pill`,
  `pd-sparkline`, Material and CDK as listed. The skeleton is the `pdSkeleton` twin of the ready template.
- **Tests:** component specs (Vitest, `await fixture.whenStable()`, CDK harnesses, rule 49): each state, a missing state
  is loading, loading height equals ready height for every sample kind, the notice replaces the first row, actions and
  retry emit the app's action, the empty layout; the chooser emits the right `Edit` for drag, ⌥↑/⌥↓/⌥Home/⌥End and menu
  items, announces moves, disables Add at limits, shows "Added", shows and dismisses the undo toast from `can_undo`, is
  read-only when not `editable`. Gallery Playwright (Chromium): sizes, focus order, menu never over the focus ring, axe in
  both themes. A token test checks that the high-contrast frame outline is `--pd-border-strong` and differs
  from surface-3 in `more` contrast, and an e2e shot of the gallery in `data-contrast="more"` shows it. Components at 100%; `npm run test:tokens` green.
- **Gates:** `cd ui && npm run lint && npm test && npm run build && npx playwright test --project=gallery`.

### WP-C1: Dyno engine, protocol and agent (implementer, Sonnet)

- **Owns:** `apps/dyno/crates/dyno-core/src/widgets/**` (catalogues: `popover.rs`, `dashboard.rs`, `menu_bar.rs`; value
  builders `popover_values.rs`), `dyno-core/src/settings.rs` (the three `Stored` fields; `Tray`, `TrayMetric`,
  `TrayStyle` and `tray_items` deleted), `dyno-core/src/protocol.rs` (requests, event, `PROTOCOL_VERSION` 3),
  `apps/dyno/crates/dyno-agent/**` handlers, `apps/dyno/app/src-tauri/src/` (commands `widget_layout`,
  `edit_widget_layout`, `popover_values`, `run_widget_action`; the popover-values emitter gated by visibility; the agent
  notice; `tray_model.rs` reading `menu_bar`; tray-class capabilities), `app/src/app/core/generated/widgets.ts`.
- **Build:** §5. Delete `dyno-core/src/widgets.rs`'s layout code and every Rust symbol listed in the register.
- **Tests:** every popup kind's value builder for ready, empty, error, working and stale from synthetic inputs; the
  emitter stops when the popover hides and resumes on show; losing the agent gives one notice and stale values; Restart
  goes to the supervisor; old settings keys load defaults with a note; the protocol round-trips the new requests; the
  dashboard and menu bar still render their defaults. Record test count and time before and after (rule 43).
- **Gates:** `node scripts/affected.mjs origin/main --explain`, Dyno's fmt, clippy and tests with its own
  `CARGO_TARGET_DIR`.

### WP-C2: Dyno UI (implementer, Sonnet)

- **Owns:** `apps/dyno/app/src/app/features/{tray,widgets,settings,dashboard}/**`, `core/api.types.ts` (deletions),
  `core/backend.ts` and the Tauri and mock backends (the new commands and the mock's widget data with switches for every
  state), `docs/known-copies.toml` (remove `widget-layouts` with the files), `apps/dyno/docs/roadmap.md` (WP-38 done),
  `apps/dyno/docs/adr/0012-*.md` (status: superseded in part by family ADR 0035), Dyno's CHANGELOG.
- **Build:** the popup (§5), the chooser section, the menu-bar items editor and the dashboard edit mode on the shared
  edits; delete `widget-model.ts`'s catalogue, limits, edits and severity logic and their duplicate specs.
- **Build (menus):** the footer menu shows ⌘, and ⌘Q (Ctrl+, and Ctrl+Q on Windows and Linux) only if the popover
  window binds those keys; otherwise no shortcut is shown.
- **Gates:** `cd apps/dyno/app && npm run lint && npm test`.

### WP-D: end-to-end, screenshots and the real-app smoke (implementer, Sonnet)

- **Owns:** `apps/dyno/app/e2e/tray-widgets.spec.ts`, `e2e/screenshots.spec.ts` additions,
  `apps/dyno/docs/screenshots/tray-widgets-*.png` and `settings-popup-widgets-*.png`, the roadmap's screenshot rows.
- **Build and test:** against the mock backend: choose, reorder by drag and by ⌥ keys, remove and Undo, reset, the popup
  follows live and keeps the order after a reload; each kind's loading, empty, error, working and stale state and the
  agent notice via the mock's switches; axe on the popup and the chooser in both themes; focus order equals widget
  order. One real-app smoke on macOS (rule 43): change the order in Settings, the visible popover follows, restart,
  order kept; stop the agent, the notice shows, Restart agent recovers. Screenshots in light and dark, then an
  independent review (rule 22).
