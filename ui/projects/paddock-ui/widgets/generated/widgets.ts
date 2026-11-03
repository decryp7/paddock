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
max: number, 
/**
 * The surface's noun ("popup").
 */
noun: string, } } | { "kindLimit": { 
/**
 * Its `max_instances`.
 */
max: number, 
/**
 * The surface's noun.
 */
noun: string, } } | { "spanNotAllowed": { 
/**
 * The allowed spans.
 */
allowed: Array<number>, } } | { "sizeNotAllowed": { 
/**
 * Allowed widths.
 */
widths: StepsInfo, 
/**
 * Allowed heights.
 */
heights: StepsInfo, } } | { "blocked": Block } | "stale" | "nothingToRedo" | { "tooFew": { 
/**
 * The surface's `min_placements`.
 */
min: number, 
/**
 * The surface's noun.
 */
noun: string, } } | { "invalid": string } | "builtinPreset" | "nothingToUndo" | "newerLayout";

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

export type Span = { 
/**
 * Columns it spans; one of its kind's spans.
 */
span: number, };

export type Cells = { 
/**
 * Left column, 0-based.
 */
x: number, 
/**
 * Top row, 0-based.
 */
y: number, 
/**
 * Width in columns.
 */
w: number, 
/**
 * Height in rows.
 */
h: number, };

export type StepsInfo = { "steps": "range", 
/**
 * Smallest.
 */
min: number, 
/**
 * Largest.
 */
max: number, } | { "steps": "only", 
/**
 * The values.
 */
values: Array<number>, };

export type KindSize = { "arrangement": "spans", 
/**
 * The spans.
 */
spans: Array<number>, } | { "arrangement": "cells", 
/**
 * Allowed widths.
 */
widths: StepsInfo, 
/**
 * Allowed heights.
 */
heights: StepsInfo, 
/**
 * Default width.
 */
defaultW: number, 
/**
 * Default height.
 */
defaultH: number, };

export type Block = { "edge": Dir } | "largest" | "smallest" | "unchanged";

export type Placed = { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * Its cells.
 */
rect: Rect, };

export type Arrangement = { 
/**
 * The least container width, in px, for this arrangement.
 */
minWidth: number, 
/**
 * Its columns.
 */
columns: number, 
/**
 * Every placement, in reading order.
 */
placements: Array<Placed>, };

export type PlacementMoves = { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * Its moves.
 */
moves: Moves, };

export type PreviewRequest = { 
/**
 * The surface, as its id serialises ("dashboard").
 */
surface: string, 
/**
 * The revision the webview's view was computed on.
 */
revision: number, 
/**
 * The gesture.
 */
gesture: GridGesture, 
/**
 * The webview's counter; replies older than the latest are dropped there.
 */
seq: number, };

export type Rect = { 
/**
 * Left column, 0-based.
 */
x: number, 
/**
 * Top row, 0-based.
 */
y: number, 
/**
 * Width in columns.
 */
w: number, 
/**
 * Height in rows.
 */
h: number, };

export type Dir = "left" | "right" | "up" | "down";

export type GridGesture = { "gesture": "place", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The target.
 */
rect: Rect, } | { "gesture": "nudge", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The direction.
 */
dir: Dir, } | { "gesture": "grow", 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * The direction.
 */
dir: Dir, } | { "gesture": "toFirst", 
/**
 * The placement.
 */
id: PlacementId, } | { "gesture": "toLast", 
/**
 * The placement.
 */
id: PlacementId, };

export type Moves = { 
/**
 * Move left.
 */
left: boolean, 
/**
 * Move right.
 */
right: boolean, 
/**
 * Move up.
 */
up: boolean, 
/**
 * Move down.
 */
down: boolean, 
/**
 * Move to first.
 */
first: boolean, 
/**
 * Move to last.
 */
last: boolean, 
/**
 * Wider.
 */
wider: boolean, 
/**
 * Narrower.
 */
narrower: boolean, 
/**
 * Taller.
 */
taller: boolean, 
/**
 * Shorter.
 */
shorter: boolean, };

export type Placement<W, P = Span> = { 
/**
 * Unique within its layout.
 */
id: PlacementId, 
/**
 * The widget and its options.
 */
widget: W, } & P;

export type Preset<W, P = Span> = { 
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
placements: Array<Placement<W, P>>, };

export type Layout<W, P = Span> = { 
/**
 * The surface's `schema_version` when written.
 */
version: number, 
/**
 * The placements, in order.
 */
placements: Array<Placement<W, P>>, 
/**
 * The person's own presets only.
 */
presets: Array<Preset<W, P>>, };

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
id: PresetId, } } | "resetToDefaults" | { "arrange": { 
/**
 * The gesture.
 */
gesture: GridGesture, } } | "undo" | "redo";

export type SurfaceInfo<S> = { 
/**
 * The app's surface.
 */
id: S, 
/**
 * What the surface is called in messages ("popup").
 */
noun: string, 
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
 * How it may be sized: spans (flow) or widths and heights (grid).
 */
size: KindSize, 
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

export type LayoutView<S, W, P, K> = { 
/**
 * The layout.
 */
layout: Layout<W, P>, 
/**
 * Counts up with every applied edit and every reload; an arrange edit names the revision it was computed on.
 */
revision: number, 
/**
 * The arrangement for each breakpoint, widest first (grid surfaces; empty on flow surfaces).
 */
arrangements: Array<Arrangement>, 
/**
 * Which gestures work for each placement (grid surfaces; empty on flow surfaces).
 */
moves: Array<PlacementMoves>, 
/**
 * True when an undone edit can be redone.
 */
canRedo: boolean, 
/**
 * The catalogue.
 */
catalogue: CatalogueInfo<S, K>, 
/**
 * The catalogue's own presets (the person's are in the layout).
 */
builtinPresets: Array<Preset<W, P>>, 
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

export type WidgetAction<A> = { 
/**
 * The app's action.
 */
action: A, 
/**
 * Verb plus object, sentence case: "Pin counters", "Retry".
 */
label: string, };

export type Progress = { "count": { 
/**
 * Items finished so far.
 */
done: number, 
/**
 * What is counted: "volumes", "files".
 */
unit: string, } } | { "fraction": { 
/**
 * Items finished so far.
 */
done: number, 
/**
 * All items.
 */
total: number, 
/**
 * What is counted.
 */
unit: string, } } | "unknown";

export type WidgetState<V, A> = { "state": "ready", 
/**
 * The app's value.
 */
value: V, 
/**
 * When the value was read, in milliseconds since the Unix epoch.
 */
asOf: number, 
/**
 * Since when the value has been out of date, in milliseconds since the Unix epoch.
 */
staleSince: number | null, } | { "state": "working", 
/**
 * What is running.
 */
title: string, 
/**
 * The current phase.
 */
phase: string, 
/**
 * How far it has come.
 */
progress: Progress, 
/**
 * The previous value, still shown.
 */
last: V | null, 
/**
 * How to cancel it, when it can be cancelled.
 */
cancel: WidgetAction<A> | null, } | { "state": "empty", 
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
next: WidgetAction<A> | null, 
/**
 * `true`: the empty state means "all good" (no active alarms), drawn as one calm row with a success check.
 * `false` (the default): "nothing here yet", drawn with the info icon and the next step.
 */
calm: boolean, } | { "state": "error", 
/**
 * What failed, in plain words.
 */
title: string, 
/**
 * Why, when known.
 */
cause: string | null, 
/**
 * The next step, such as Retry.
 */
next: WidgetAction<A> | null, };

export type PlacementState<V, A> = { 
/**
 * The placement.
 */
id: PlacementId, 
/**
 * Its state.
 */
state: WidgetState<V, A>, };

export type Tone = "info" | "warning" | "danger";

export type SurfaceNotice<A> = { 
/**
 * How serious it is.
 */
tone: Tone, 
/**
 * What happened, in plain words.
 */
title: string, 
/**
 * One line of detail.
 */
detail: string, 
/**
 * The next step.
 */
action: WidgetAction<A> | null, };

export type SurfaceValues<V, A> = { 
/**
 * A message for the whole surface.
 */
notice: SurfaceNotice<A> | null, 
/**
 * Each placement's state.
 */
placements: Array<PlacementState<V, A>>, };
