// Generated from the Rust wire types of paddock-widgets (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-widgets --lib generated.

export type PlacementId = string;

export type WidgetError = "badId" | { "duplicateId": string } | { "unknownPlacement": string } | { "tooMany": { 
/**
 * What there are too many of ("widgets", "saved layouts", "widgets in a saved layout").
 */
what: string, 
/**
 * The limit.
 */
max: number, } } | { "kindLimit": { 
/**
 * The kind's title.
 */
kind: string, 
/**
 * Its `max_instances`.
 */
max: number, } } | { "spanNotAllowed": { 
/**
 * The kind's title.
 */
kind: string, 
/**
 * The allowed spans.
 */
spans: Array<number>, } } | { "invalid": string } | { "notAPreset": string } | "builtinPreset" | "nothingToUndo";

export type NoteCode = { "code": "damaged" } | { "code": "newer" } | { "code": "tooLarge" } | { "code": "dropped", 
/**
 * How many entries were dropped.
 */
count: number, };

export type Note = { 
/**
 * The sentence to show.
 */
message: string, } & ({ "code": "damaged" } | { "code": "newer" } | { "code": "tooLarge" } | { "code": "dropped", 
/**
 * How many entries were dropped.
 */
count: number, });

export type Refresh = { "type": "live" } | { "type": "every", 
/**
 * The period.
 */
secs: number, } | { "type": "onChange" };

export type Placement<W> = { 
/**
 * Unique within its layout.
 */
id: PlacementId, 
/**
 * Columns of the surface's grid; one the kind allows.
 */
span: number, 
/**
 * The widget and its options.
 */
widget: W, };

export type Preset<W> = { 
/**
 * Unique among the built-in and saved presets of a surface.
 */
id: PlacementId, 
/**
 * The name to show, 1 to 64 characters.
 */
name: string, 
/**
 * The catalogue's own preset: never stored, cannot be deleted.
 */
builtin: boolean, 
/**
 * Its placements.
 */
placements: Array<Placement<W>>, };

export type Layout<W> = { 
/**
 * The surface's `schema_version` when written.
 */
version: number, 
/**
 * The placements, in order.
 */
placements: Array<Placement<W>>, 
/**
 * The person's own presets.
 */
presets: Array<Preset<W>>, };

export type Edit<W> = { "type": "add", 
/**
 * The widget.
 */
widget: W, 
/**
 * Its span.
 */
span?: number | null, 
/**
 * Its position (clamped into range).
 */
at?: number | null, } | { "type": "remove", 
/**
 * The placement.
 */
id: PlacementId, } | { "type": "move", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new position.
 */
to: number, } | { "type": "resize", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new span.
 */
span: number, } | { "type": "replace", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new widget, of the same kind.
 */
widget: W, } | { "type": "applyPreset", 
/**
 * The preset.
 */
id: PlacementId, } | { "type": "savePreset", 
/**
 * The name, 1 to 64 characters.
 */
name: string, } | { "type": "deletePreset", 
/**
 * The preset.
 */
id: PlacementId, } | { "type": "resetToDefaults" } | { "type": "undo" };

export type SurfaceInfo = { 
/**
 * The surface's name.
 */
id: string, 
/**
 * Columns of the grid.
 */
columns: number, 
/**
 * Most placements.
 */
maxPlacements: number, 
/**
 * Fewest placements.
 */
minPlacements: number, 
/**
 * Most saved presets.
 */
maxUserPresets: number, 
/**
 * Most placements in a preset.
 */
maxPresetPlacements: number, };

export type KindInfo<K> = { 
/**
 * The kind.
 */
kind: K, 
/**
 * Sentence-case title.
 */
title: string, 
/**
 * One line for the chooser.
 */
summary: string, 
/**
 * Registered icon name.
 */
icon: string, 
/**
 * Allowed spans, the first is the default.
 */
spans: Array<number>, 
/**
 * How many may be placed.
 */
maxInstances: number, 
/**
 * How it refreshes.
 */
refresh: Refresh, };

export type CatalogueInfo<K> = { 
/**
 * The surface.
 */
surface: SurfaceInfo, 
/**
 * The kinds, in chooser order.
 */
kinds: Array<KindInfo<K>>, };

export type LayoutView<W, K> = { 
/**
 * The layout.
 */
layout: Layout<W>, 
/**
 * The catalogue.
 */
catalogue: CatalogueInfo<K>, 
/**
 * The catalogue's own presets (the person's are in the layout).
 */
builtinPresets: Array<Preset<W>>, 
/**
 * What was wrong when the layout was read, to show once.
 */
notes: Array<Note>, 
/**
 * Whether an undo is available.
 */
canUndo: boolean, };

export type WidgetAction = { 
/**
 * The app-defined action id.
 */
id: string, 
/**
 * The button text: verb plus object.
 */
label: string, };

export type WidgetState<V> = { "state": "ready", 
/**
 * The app's value.
 */
value: V, 
/**
 * When the value was read, in milliseconds since the Unix epoch.
 */
asOf: number | null, 
/**
 * The value is older than its refresh allows.
 */
stale: boolean, } | { "state": "empty", 
/**
 * What there is none of.
 */
title: string, 
/**
 * One line of detail.
 */
detail: string, 
/**
 * The next step.
 */
action: WidgetAction | null, } | { "state": "error", 
/**
 * What failed, in plain words.
 */
title: string, 
/**
 * Why, when known.
 */
cause: string | null, 
/**
 * Whether Retry is offered.
 */
retry: boolean, 
/**
 * Another next step.
 */
action: WidgetAction | null, };

export type PlacementState<V> = { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * Its state.
 */
state: WidgetState<V>, };
