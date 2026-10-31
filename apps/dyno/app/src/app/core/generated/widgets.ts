// Generated from Dyno's widget wire types and paddock-widgets (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p dyno-core --lib widgets::generated.

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

export type DynoSurface = "popover" | "dashboard" | "menuBar";

export type Metric = "cpuTotal" | "cpuPerCore" | "memory" | "memoryPressure" | "swap" | "diskActivity" | "diskFree" | "networkUp" | "networkDown" | "gpuLoad" | "temperature" | "fan" | "battery" | { "process": string } | { "perfCounter": string };

export type VolumeChoice = "all" | "system" | { "mount": string };

export type PopoverWidget = "activeAlarms" | { "diskSpace": { 
/**
 * Which volume.
 */
volume: VolumeChoice, } } | "cpuMemory" | "network" | "backupHealth" | "lastIssue" | "pinnedCounters" | "topApps";

export type PopoverKind = "activeAlarms" | "diskSpace" | "cpuMemory" | "network" | "backupHealth" | "lastIssue" | "pinnedCounters" | "topApps";

export type Thresholds = { 
/**
 * Warning level, in the metric's unit.
 */
warning: number | null, 
/**
 * Critical level.
 */
critical: number | null, };

export type WidgetOptions = { 
/**
 * How much history a graph or heat map shows, seconds (30-3600).
 */
rangeSecs: number, 
/**
 * Draw the metric's name.
 */
showLabel: boolean, 
/**
 * Fill under graph lines.
 */
fill: boolean, 
/**
 * An accent colour (`#rrggbb`), or the theme's default.
 */
accent: string | null, };

export type DashboardKind = "graph" | "bigNumber" | "bars" | "ring" | "heatmap" | "topProcesses" | "issues" | "health" | "cpu" | "diagnosis" | "events" | "history" | "consumers" | "diskTrend";

export type DashboardWidget = { 
/**
 * What it draws.
 */
kind: DashboardKind, 
/**
 * What it measures.
 */
metric: Metric, 
/**
 * Display options.
 */
options: WidgetOptions, 
/**
 * Colour thresholds.
 */
thresholds: Thresholds, };

export type MenuBarItem = { "lineGraph": { 
/**
 * What it shows.
 */
metric: Metric, } } | { "verticalBars": { 
/**
 * What it shows.
 */
metric: Metric, } } | { "horizontalMeter": { 
/**
 * What it shows.
 */
metric: Metric, } } | { "ring": { 
/**
 * What it shows.
 */
metric: Metric, } } | { "number": { 
/**
 * What it shows.
 */
metric: Metric, 
/**
 * The unit after the value ("%", "°C").
 */
unit: string, 
/**
 * A short label before it ("CPU").
 */
label: string, } } | { "graphAndNumber": { 
/**
 * What it shows.
 */
metric: Metric, } };

export type MenuBarKind = "lineGraph" | "verticalBars" | "horizontalMeter" | "ring" | "number" | "graphAndNumber";

export type ValueTone = "ok" | "warn" | "bad";

export type CounterUnit = "percent" | "bytes" | "bytesPerSec" | "perSec" | "count" | "milliseconds" | "seconds" | "number";

export type Page = "overview" | "alarms" | "disk" | "network" | "issues" | "backups" | "counters" | "processes";

export type WidgetActionId = { "openPage": Page } | { "retry": PlacementId } | "chooseVolume" | "pinCounters" | "setUpBackupChecks" | "startBackupCheck" | "restartAgent" | "customizeWidgets";

export type Level = "info" | "warning" | "critical";

export type AlarmRow = { 
/**
 * The alarm's id.
 */
id: number, 
/**
 * One line.
 */
title: string, 
/**
 * How bad.
 */
level: Level, 
/**
 * When the condition first appeared, ms since the epoch.
 */
since: number, 
/**
 * Whether someone acknowledged it.
 */
acknowledged: boolean, };

export type VolumeRow = { 
/**
 * The volume's id (its mount point).
 */
id: string, 
/**
 * Display name.
 */
name: string, 
/**
 * Used space, percent; none when the volume cannot say.
 */
usedPercent: number | null, 
/**
 * Free bytes; none when the volume cannot say.
 */
freeBytes: number | null, 
/**
 * Capacity in bytes; none when the volume cannot say.
 */
totalBytes: number | null, 
/**
 * Against the disk thresholds.
 */
tone: ValueTone, 
/**
 * False for a network volume that did not answer ("Not answering").
 */
reachable: boolean, };

export type CounterRow = { 
/**
 * The counter's path.
 */
path: string, 
/**
 * Its name: the part after the last backslash.
 */
name: string, 
/**
 * The reading, or none ("Unavailable").
 */
value: number | null, 
/**
 * What the value measures.
 */
unit: CounterUnit, };

export type AppRow = { 
/**
 * The app's friendly name.
 */
name: string, 
/**
 * Its CPU, percent of the machine; none when it could not be read.
 */
cpuPercent: number | null, };

export type BackupVerdict = "healthy" | "stale" | "noBackupYet";

export type BackupValue = { 
/**
 * The tool's name ("Time Machine").
 */
name: string, 
/**
 * Its verdict.
 */
state: BackupVerdict, 
/**
 * When the last backup finished.
 */
lastBackup: number | null, 
/**
 * One plain sentence.
 */
summary: string, 
/**
 * How many tools are set up.
 */
tools: number, };

export type IssueValue = { 
/**
 * One line.
 */
title: string, 
/**
 * The application or device it concerns; empty when none applies.
 */
app: string, 
/**
 * The kind's name ("App crash").
 */
kind: string, 
/**
 * How serious.
 */
level: Level, 
/**
 * When it happened, ms since the epoch.
 */
at: number, };

export type PopoverValue = { "activeAlarms": { 
/**
 * The first rows.
 */
rows: Array<AlarmRow>, 
/**
 * How many more there are.
 */
more: number, } } | { "diskSpace": { 
/**
 * The first rows.
 */
rows: Array<VolumeRow>, 
/**
 * How many more there are.
 */
more: number, } } | { "cpuMemory": { 
/**
 * CPU load, 0 to 100.
 */
cpuPercent: number, 
/**
 * Against the CPU thresholds.
 */
cpuTone: ValueTone, 
/**
 * Memory in use, bytes.
 */
memoryUsed: number, 
/**
 * Memory installed, bytes.
 */
memoryTotal: number, 
/**
 * Memory in use, percent.
 */
memoryPercent: number, 
/**
 * Against the memory thresholds.
 */
memoryTone: ValueTone, 
/**
 * CPU load over the last minute, oldest first.
 */
cpuMinute: Array<number>, } } | { "network": { 
/**
 * Download, bytes per second.
 */
rxBps: number, 
/**
 * Upload, bytes per second.
 */
txBps: number, 
/**
 * Download over the last minute, oldest first.
 */
rxMinute: Array<number>, 
/**
 * Upload over the last minute, oldest first.
 */
txMinute: Array<number>, } } | { "backupHealth": BackupValue } | { "lastIssue": IssueValue } | { "pinnedCounters": { 
/**
 * The first rows.
 */
rows: Array<CounterRow>, 
/**
 * How many more are pinned.
 */
more: number, } } | { "topApps": { 
/**
 * The rows.
 */
rows: Array<AppRow>, } };

export type SurfaceView = { "popover": LayoutView<DynoSurface, PopoverWidget, PopoverKind> } | { "dashboard": LayoutView<DynoSurface, DashboardWidget, DashboardKind> } | { "menuBar": LayoutView<DynoSurface, MenuBarItem, MenuBarKind> };

export type SurfaceEdit = { "popover": Edit<PopoverWidget> } | { "dashboard": Edit<DashboardWidget> } | { "menuBar": Edit<MenuBarItem> };
