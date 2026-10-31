// Generated from the Rust wire types of paddock-widgets (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-widgets --lib generated.

export type PlacementId = string;

export type PresetId = string;

export type Counted = "placements" | "userPresets" | "presetPlacements";

export type WidgetError = { "unknownPlacement": string } | { "unknownPreset": string } | { "duplicateId": string } | { "tooMany": { 
/**
 * What there are too many of.
 */
counted: Counted, 
/**
 * The limit.
 */
max: number, } } | { "kindLimit": { 
/**
 * Its `max_instances`.
 */
max: number, } } | { "spanNotAllowed": { 
/**
 * The allowed spans.
 */
allowed: Array<number>, } } | { "tooFew": { 
/**
 * The surface's `min_placements`.
 */
min: number, } } | { "invalid": string } | "builtinPreset" | "nothingToUndo" | "newerLayout";

export type NoteCode = "damaged" | "tooLarge" | { "newer": { 
/**
 * The stored version.
 */
version: number, } } | { "dropped": { 
/**
 * How many.
 */
count: number, } };

export type Note = { 
/**
 * Machine-readable reason.
 */
code: NoteCode, 
/**
 * The sentence to show.
 */
message: string, };

export type Refresh = "live" | { "every": { 
/**
 * The period.
 */
secs: number, } } | "onChange";

export type Placement<W> = { 
/**
 * Unique within its layout.
 */
id: PlacementId, 
/**
 * Columns it spans; one of its kind's spans.
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
id: PresetId, 
/**
 * The name to show, 1 to 64 characters.
 */
name: string, 
/**
 * The app's own: rebuilt on every read, cannot be edited or deleted.
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
 * The person's own presets only.
 */
presets: Array<Preset<W>>, };

export type Edit<W> = { "add": { 
/**
 * The widget.
 */
widget: W, 
/**
 * Its span; the kind's first by default.
 */
span: number | null, 
/**
 * Its position (clamped into range).
 */
at: number | null, } } | { "remove": { 
/**
 * The placement.
 */
id: PlacementId, } } | { "move": { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new position.
 */
to: number, } } | { "resize": { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new span.
 */
span: number, } } | { "replace": { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The new widget, of the same kind.
 */
widget: W, } } | { "applyPreset": { 
/**
 * The preset.
 */
id: PresetId, } } | { "savePreset": { 
/**
 * The name, 1 to 64 characters.
 */
name: string, } } | { "deletePreset": { 
/**
 * The preset.
 */
id: PresetId, } } | "resetToDefaults" | "undo";

export type SurfaceInfo<S> = { 
/**
 * The app's surface.
 */
id: S, 
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
maxUserPresets: number, };

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

export type CatalogueInfo<S, K> = { 
/**
 * The surface.
 */
surface: SurfaceInfo<S>, 
/**
 * The kinds, in chooser order.
 */
kinds: Array<KindInfo<K>>, };

export type LayoutView<S, W, K> = { 
/**
 * The layout.
 */
layout: Layout<W>, 
/**
 * The catalogue.
 */
catalogue: CatalogueInfo<S, K>, 
/**
 * The catalogue's own presets (the person's are in the layout).
 */
builtinPresets: Array<Preset<W>>, 
/**
 * What was wrong when the layout was read, to show once.
 */
notes: Array<Note>, 
/**
 * True right after an undoable edit.
 */
canUndo: boolean, 
/**
 * False while a newer version's layout is kept (`NoteCode::Newer`): the chooser is read-only.
 */
editable: boolean, };

export type WidgetAction<A> = { action: A, 
/**
 * Verb plus object, sentence case: "Pin counters", "Retry".
 */
label: string, };

export type Progress = { "count": { done: number, unit: string, } } | { "fraction": { done: number, total: number, unit: string, } };

export type WidgetState<V, A> = { "state": "ready", value: V, asOf: number, staleSince: number | null, } | { "state": "working", title: string, phase: string, progress: Progress, last: V | null, cancel: WidgetAction<A> | null, } | { "state": "empty", title: string, detail: string, next: WidgetAction<A> | null, } | { "state": "error", title: string, cause: string | null, next: WidgetAction<A> | null, };

export type PlacementState<V, A> = { id: PlacementId, state: WidgetState<V, A>, };

export type Tone = "info" | "warning" | "danger";

export type SurfaceNotice<A> = { tone: Tone, title: string, detail: string, action: WidgetAction<A> | null, };

export type SurfaceValues<V, A> = { notice: SurfaceNotice<A> | null, placements: Array<PlacementState<V, A>>, };
