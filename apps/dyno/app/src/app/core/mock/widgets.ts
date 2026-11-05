import type { CommandError } from '../api.types';
import type {
  Arrangement,
  Cells,
  CatalogueInfo,
  DashboardKind,
  DashboardWidget,
  DynoSurface,
  Edit,
  GridGesture,
  KindInfo,
  KindSize,
  Layout,
  Moves,
  Outcome,
  PreviewReply,
  PreviewRequest,
  Rect,
  StepsInfo,
  MenuBarItem,
  MenuBarKind,
  MetricReading,
  ReadingQuery,
  WidgetChoices,
  Metric,
  Placement,
  PlacementState,
  PopoverKind,
  PopoverValue,
  PopoverWidget,
  Preset,
  Refresh,
  SurfaceEdit,
  SurfaceInfo,
  SurfaceView,
  WidgetActionId,
  WidgetState,
} from '../generated/widgets';
import type { DashboardView, PopoverSurfaceValues } from '../widget-types';
import { wobble } from './noise';

/**
 * The mock's widget layouts and popup values (family ADR 0035). It mimics the engine's contract with plausible data so the
 * browser build and the UI tests can exercise choosing, reordering, undo and every widget state; it is never the reference
 * for behaviour (the real engine is `paddock-widgets` and Dyno's catalogues).
 */

/** Which state the mock shows a popup kind in (`?popup=diskSpace:error,network:empty`). */
export type MockWidgetState = 'ready' | 'loading' | 'empty' | 'error' | 'working' | 'stale';
const STATES: readonly MockWidgetState[] = ['ready', 'loading', 'empty', 'error', 'working', 'stale'];

type W = PopoverWidget | DashboardWidget | MenuBarItem;
type AnyKind = PopoverKind | DashboardKind | MenuBarKind;

const kindOf = (w: W): string => (typeof w === 'string' ? w : 'kind' in w ? w.kind : (Object.keys(w)[0] as string));
const kebab = (s: string): string => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
const err = (message: string): CommandError => ({ code: 'badRequest', message }) as CommandError;

const info = (kind: AnyKind, title: string, summary: string, icon: string, size: number[] | KindSize, maxInstances = 1, refresh: Refresh = 'live') => ({ kind, title, summary, icon, size: Array.isArray(size) ? ({ spans: { spans: size } } as KindSize) : size, maxInstances, refresh });

const POPOVER_KINDS = [
  info('activeAlarms', 'Active alarms', 'What needs attention now, the most severe first', 'alarm', [2], 1, 'onChange'),
  info('diskSpace', 'Disk space', 'Free space per volume', 'disk', [2], 4, { every: { secs: 60 } }),
  info('cpuMemory', 'CPU and memory', 'Load and memory in use, with the last minute', 'cpu', [2]),
  info('network', 'Network', 'Download and upload now', 'network', [1, 2]),
  info('backupHealth', 'Backup', 'When the last backup ran', 'backup', [1, 2], 1, 'onChange'),
  info('lastIssue', 'Last issue', 'The newest problem of the last 7 days', 'issue', [2], 1, 'onChange'),
  info('pinnedCounters', 'Pinned counters', 'Your pinned performance counters', 'counters', [2]),
  info('topApps', 'Top apps', 'The busiest apps right now', 'apps', [2]),
] as KindInfo<PopoverKind>[];

/** A grid kind's sizes: its default (`w` by `h`) and the widths and heights it allows (the engine's table, Dyno WP-C1). */
const cellsRule = (w: number, h: number, widths: [number, number], heights: [number, number]): KindSize => ({
  cells: {
    widths: { range: { min: widths[0], max: widths[1] } },
    heights: { range: { min: heights[0], max: heights[1] } },
    defaultW: w,
    defaultH: h,
  },
});
const DASHBOARD_KINDS = [
  info('graph', 'Graph', 'A line over the last minutes.', 'graph', cellsRule(3, 4, [2, 12], [4, 8]), 32),
  info('bigNumber', 'Big number', 'One number, large.', 'number', cellsRule(3, 3, [2, 4], [3, 4]), 32),
  info('bars', 'Bars', 'A level against its limit.', 'bars', cellsRule(3, 3, [2, 6], [3, 4]), 32),
  info('ring', 'Ring', 'A percentage as a ring.', 'ring', cellsRule(3, 4, [2, 4], [4, 6]), 32),
  info('heatmap', 'Per-core heatmap', 'One cell per core.', 'heatmap', cellsRule(6, 3, [4, 12], [3, 5]), 32),
  info('topProcesses', 'Top processes', 'The apps using the most.', 'apps', cellsRule(6, 6, [3, 12], [5, 12]), 32),
  info('issues', 'Issues', 'Unusual patterns found.', 'issue', cellsRule(3, 6, [3, 6], [4, 12]), 32, 'onChange'),
  info('health', 'Health', 'The health score.', 'health', cellsRule(6, 9, [3, 12], [5, 14]), 32),
  info('cpu', 'CPU', 'Load, per-core view and details.', 'cpu', cellsRule(6, 6, [4, 12], [5, 10]), 32),
  info('diagnosis', 'Why slow', 'What is slowing the computer, in words.', 'diagnosis', cellsRule(6, 9, [4, 12], [6, 14]), 32, { every: { secs: 10 } }),
  info('events', 'Recent events', 'Alarms and what Dyno did.', 'events', cellsRule(6, 7, [4, 12], [4, 14]), 32, 'onChange'),
  info('history', 'History', 'A chart with events marked.', 'history', cellsRule(12, 8, [6, 12], [5, 14]), 32, 'onChange'),
  info('consumers', 'Biggest consumers', 'Biggest consumers over a day.', 'consumers', cellsRule(6, 7, [4, 12], [5, 12]), 32, { every: { secs: 60 } }),
  info('diskTrend', 'Disk space trend', 'When the disks will be full.', 'disk', cellsRule(12, 6, [6, 12], [4, 10]), 32, { every: { secs: 60 } }),
] as KindInfo<DashboardKind>[];

const MENU_KINDS = [
  info('lineGraph', 'Line graph', 'A small history line', 'graph', [1], 32),
  info('verticalBars', 'Bars', 'Vertical bars', 'bars', [1], 32),
  info('horizontalMeter', 'Meter', 'A horizontal meter', 'meter', [1], 32),
  info('ring', 'Ring', 'A percentage as a ring', 'ring', [1], 32),
  info('number', 'Number', 'The value as text', 'number', [1], 32),
  info('graphAndNumber', 'Graph and number', 'A line with the number beside it', 'graph', [1], 32),
] as KindInfo<MenuBarKind>[];

const SURFACES: Record<DynoSurface, { surface: SurfaceInfo<DynoSurface>; kinds: KindInfo<string>[] }> = {
  popover: { surface: { id: 'popover', noun: 'popup', columns: 2, maxPlacements: 8, minPlacements: 0, maxUserPresets: 0 }, kinds: POPOVER_KINDS },
  dashboard: { surface: { id: 'dashboard', noun: 'dashboard', columns: 12, maxPlacements: 32, minPlacements: 1, maxUserPresets: 8 }, kinds: DASHBOARD_KINDS },
  menuBar: { surface: { id: 'menuBar', noun: 'menu bar', columns: 1, maxPlacements: 32, minPlacements: 1, maxUserPresets: 0 }, kinds: MENU_KINDS },
};

const dw = (kind: DashboardKind, metric: Metric, hot?: [number, number]): DashboardWidget => ({
  kind,
  metric,
  options: { rangeSecs: 300, showLabel: true, fill: true, accent: null },
  thresholds: hot ? { warning: hot[0], critical: hot[1] } : { warning: null, critical: null },
});

function assign<T extends W>(items: [number, T][]): Placement<T>[] {
  const seen = new Map<string, number>();
  return items.map(([span, widget]) => {
    const base = kebab(kindOf(widget));
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    return { id: n === 1 ? base : `${base}-${n}`, span, widget };
  });
}

const DASH_PRESETS: Record<string, [string, [number, DashboardWidget][]]> = {
  overview: ['Overview', [[6, dw('health', 'cpuTotal')], [6, dw('diagnosis', 'cpuTotal')], [3, dw('issues', 'cpuTotal')], [6, dw('cpu', 'cpuTotal')], [3, dw('graph', 'memory')], [3, dw('graph', 'diskActivity')], [3, dw('graph', 'networkDown')], [3, dw('graph', 'gpuLoad')], [3, dw('ring', 'battery')], [6, dw('topProcesses', 'cpuTotal')], [6, dw('topProcesses', 'memory')], [6, dw('events', 'cpuTotal')], [6, dw('consumers', 'cpuTotal')], [12, dw('history', 'cpuTotal')], [12, dw('diskTrend', 'diskFree')]]],
  developer: ['Developer', [[12, dw('heatmap', 'cpuPerCore')], [6, dw('graph', 'cpuTotal')], [6, dw('graph', 'memory')], [3, dw('bigNumber', 'swap')], [6, dw('graph', 'diskActivity')], [3, dw('ring', 'diskFree')], [12, dw('topProcesses', 'cpuTotal')]]],
  gaming: ['Gaming', [[12, dw('graph', 'gpuLoad')], [6, dw('graph', 'cpuTotal')], [3, dw('bigNumber', 'temperature', [80, 92])], [3, dw('bigNumber', 'fan')], [3, dw('ring', 'memory')], [6, dw('graph', 'networkDown')], [6, dw('topProcesses', 'cpuTotal')]]],
  battery: ['Battery', [[6, dw('ring', 'battery')], [6, dw('graph', 'cpuTotal')], [3, dw('bigNumber', 'temperature', [80, 92])], [12, dw('topProcesses', 'cpuTotal')], [6, dw('issues', 'cpuTotal')]]],
};

function builtinPresets(): Preset<DashboardWidget, Cells>[] {
  return Object.entries(DASH_PRESETS).map(([id, [name, items]]) => ({ id, name, builtin: true, placements: id === 'overview' ? overviewCells(items) : cellsFromSpans(items) }));
}

// ---------------------------------------------------------------------------------------------------- grid helpers
// A plausible imitation of the engine's grid (never the reference): gravity and the 1.0 span upgrade are small ports here;
// collisions are the engine's, and the mock answers a gesture from a golden case (`generated/grid-cases.json`) when the
// layout is that case's, else it places the widget where asked without moving the others.

const COLUMNS = 12;
const MAX_ROWS = 512;
type Steps = StepsInfo;
const stepsNearest = (s: Steps, v: number): number =>
  'range' in s ? Math.min(Math.max(v, s.range.min), s.range.max) : s.only.values.reduce((best, c) => (Math.abs(c - v) < Math.abs(best - v) ? c : best), s.only.values[0] ?? 1);
const stepsMove = (s: Steps, v: number, up: boolean): number | null => {
  if ('range' in s) {
    const n = v + (up ? 1 : -1);
    return n >= s.range.min && n <= s.range.max ? n : null;
  }
  const list = up ? s.only.values.filter((c) => c > v) : s.only.values.filter((c) => c < v).reverse();
  return list[0] ?? null;
};
const cellsOf = (kind: string): Extract<KindSize, { cells: unknown }>['cells'] => {
  const size = DASHBOARD_KINDS.find((k) => k.kind === kind)?.size;
  if (!size || !('cells' in size)) throw err('This widget has no size on the dashboard.');
  return size.cells;
};
const rectOf = (p: Placement<W, Cells>): Rect => ({ x: p.x, y: p.y, w: p.w, h: p.h });
const overlaps = (a: Rect, b: Rect): boolean => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const readingOrder = <T extends Rect & { id: string }>(items: T[]): T[] => [...items].sort((a, b) => a.y - b.y || a.x - b.x || (a.id < b.id ? -1 : 1));

/** Gravity: each placement, in reading order, rises to the highest free row. */
function compactCells<T extends Rect & { id: string }>(items: T[]): T[] {
  const placed: T[] = [];
  for (const item of readingOrder(items)) {
    let y = item.y;
    while (y > 0 && !placed.some((p) => overlaps({ ...item, y: y - 1 }, p))) y -= 1;
    placed.push({ ...item, y });
  }
  return readingOrder(placed);
}

/** The 1.0 layouts as they looked: left to right, a new row when the next does not fit, each row as tall as its tallest. */
function cellsFromSpans(items: [number, DashboardWidget][]): Placement<DashboardWidget, Cells>[] {
  const rows: { x: number; w: number; h: number; rule: ReturnType<typeof cellsOf>; widget: DashboardWidget }[][] = [];
  let col = 0;
  for (const [span, widget] of items) {
    const rule = cellsOf(widget.kind);
    const w = stepsNearest(rule.widths, span);
    if (rows.length === 0 || col + w > COLUMNS) {
      rows.push([]);
      col = 0;
    }
    rows[rows.length - 1]!.push({ x: col, w, h: rule.defaultH, rule, widget });
    col += w;
  }
  const out: [number, number, number, number, DashboardWidget][] = [];
  let y = 0;
  for (const row of rows) {
    const tall = Math.max(...row.map((c) => c.h));
    for (const c of row) out.push([c.x, y, c.w, stepsNearest(c.rule.heights, tall), c.widget]);
    y += tall;
  }
  const seen = new Map<string, number>();
  return out.map(([x, yy, w, h, widget]) => {
    const base = kebab(kindOf(widget));
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    return { id: n === 1 ? base : `${base}-${n}`, x, y: yy, w, h, widget };
  });
}

/** The Overview: the 1.0 look, but the memory graph keeps the four rows of the small graphs below it (as the engine's `overview()`), not the CPU's six. */
function overviewCells(items: [number, DashboardWidget][]): Placement<DashboardWidget, Cells>[] {
  return cellsFromSpans(items).map((p) => (p.widget.kind === 'graph' && p.widget.metric === 'memory' ? { ...p, h: 4 } : p));
}

/** The arrangement of a narrower grid: the engine derives it (half or full width in reading order); a rough imitation. */
function derived(items: Placement<W, Cells>[], columns: number): Arrangement['placements'] {
  const out: Arrangement['placements'] = [];
  let cursorY = 0;
  let cursorX = 0;
  for (const p of readingOrder(items)) {
    const half = Math.ceil(columns / 2);
    const w = columns === 1 ? 1 : (p.w * columns) / COLUMNS <= half ? half : columns;
    if (cursorX + w > columns) {
      cursorX = 0;
      cursorY = Math.max(0, ...out.map((o) => o.rect.y + o.rect.h));
    }
    out.push({ id: p.id, rect: { x: cursorX, y: cursorY, w, h: p.h } });
    cursorX += w;
  }
  // The engine's lone-half rule (ADR 0044 §3): a half-width widget with nothing beside it takes the whole row if its kind may be that wide.
  const half = Math.ceil(columns / 2);
  const kindOf = new Map(items.map((p) => [p.id, (p.widget as unknown as { kind: string }).kind]));
  for (const o of out) {
    if (columns === 1 || o.rect.x !== 0 || o.rect.w !== half) continue;
    const steps = cellsOf(kindOf.get(o.id)!).widths;
    const scaledMax = Math.ceil(((('range' in steps ? steps.range.max : Math.max(...steps.only.values))) * columns) / COLUMNS);
    const hole = { ...o.rect, x: half, w: columns - half };
    if (scaledMax >= columns && !out.some((q) => overlaps(q.rect, hole))) o.rect.w = columns;
  }
  return out;
}

const DEFAULTS: Record<DynoSurface, () => Placement<W>[]> = {
  popover: () => assign<W>([[2, 'activeAlarms'], [2, { diskSpace: { volume: 'all' } }], [2, 'cpuMemory']]),
  dashboard: () => overviewCells(DASH_PRESETS['overview']![1]) as unknown as Placement<W>[],
  menuBar: () =>
    assign<W>([
      [1, { number: { metric: 'cpuTotal', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'memory', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'networkDown', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'temperature', unit: '', label: '', format: 'stacked' } }],
    ]),
};

type AnyPlacement = Placement<W> | Placement<W, Cells>;

interface Stored {
  placements: AnyPlacement[];
  presets: Preset<W, never>[] | Preset<W>[];
  /** Earlier placement lists, the latest last (at most 20), and the undone ones. */
  undo: AnyPlacement[][];
  redo: AnyPlacement[][];
  /** Counts up with every applied edit, like the engine's. */
  revision: number;
  /** What the last arrangement edit did to its placement. */
  outcome: Outcome | null;
}

const UNDO_DEPTH = 20;
const stale = (): CommandError => ({ code: 'stale', message: 'The layout changed in another window; it was reloaded.' }) as CommandError;

/** A golden case of the real engine (`generated/grid-cases.json`): a layout, a gesture and what the engine answered. */
export interface GridCase {
  name: string;
  columns: number;
  widths: { min: number; max: number };
  heights: { min: number; max: number };
  items: { id: string; rect: Rect }[];
  gesture: GridGesture;
  result: { ok: { id: string; rect: Rect }[] } | { error: string };
}

/** HH:MM in the viewer's zone, the way the widget frame says "Paused at 14:28" (one time across the notice and every widget). */
function clockOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** The most alarm or volume rows the engine sends one popup widget (`ALARM_ROWS` and `VOLUME_ROWS` in dyno-core's popover_values); the mock keeps in step. */
export const POPOVER_LIST_MAX = 200;

const STORE_KEY = 'dyno-mock-widgets';

/** Options of the mock's widgets. */
export interface MockWidgetsOptions {
  /** Where layouts are kept so a reload (and another window of the same browser) sees them; `null` keeps them in memory. */
  storage?: Storage | null;
  /** The state each popup kind is shown in. */
  popup?: Partial<Record<PopoverKind, MockWidgetState>>;
  /** Show the popup as if the agent had stopped. */
  agentStopped?: boolean;
  /** Long alarm and volume lists, past the engine's bound (`?lists=long`). */
  longLists?: boolean;
  /** The engine's golden cases: a dashboard whose layout is a case's items answers that case's gesture as the engine did. */
  gridCases?: readonly GridCase[];
  now?: () => number;
}

/** Reads `?popup=diskSpace:error,network:empty&agent=stopped`. */
export function mockWidgetOptionsFromUrl(search: string): Pick<MockWidgetsOptions, 'popup' | 'agentStopped'> {
  const q = new URLSearchParams(search);
  const popup: Partial<Record<PopoverKind, MockWidgetState>> = {};
  for (const pair of (q.get('popup') ?? '').split(',')) {
    const [kind, state] = pair.split(':').map((t) => t.trim());
    if (kind && POPOVER_KINDS.some((k) => k.kind === kind) && STATES.includes(state as MockWidgetState)) popup[kind as PopoverKind] = state as MockWidgetState;
  }
  return { popup, agentStopped: q.get('agent') === 'stopped' };
}

/** The mock's widget engine and popup values. */
export class MockWidgets {
  private readonly storage: Storage | null;
  private readonly now: () => number;
  private state: Record<DynoSurface, Stored>;
  private readonly cases: readonly GridCase[];
  private readonly states: Partial<Record<PopoverKind, MockWidgetState>>;
  private agentStopped: boolean;
  private readonly longLists: boolean;
  private readonly layoutListeners = new Set<(s: DynoSurface) => void>();
  private readonly valueListeners = new Set<(v: PopoverSurfaceValues) => void>();

  constructor(options: MockWidgetsOptions = {}) {
    this.storage = options.storage ?? null;
    this.now = options.now ?? Date.now;
    this.cases = options.gridCases ?? [];
    this.states = { ...options.popup };
    this.agentStopped = options.agentStopped ?? false;
    this.longLists = options.longLists ?? false;
    this.state = this.load();
    if (this.storage && typeof window !== 'undefined') {
      window.addEventListener('storage', (e) => {
        if (e.key !== STORE_KEY) return;
        this.state = this.load();
        for (const s of ['popover', 'dashboard', 'menuBar'] as const) this.layoutListeners.forEach((l) => l(s));
        this.pushValues();
      });
    }
  }

  private load(): Record<DynoSurface, Stored> {
    const blank = (placements: AnyPlacement[]): Stored => ({ placements, presets: [], undo: [], redo: [], revision: 1, outcome: null });
    const fresh = (): Record<DynoSurface, Stored> => ({
      popover: blank(DEFAULTS.popover()),
      dashboard: blank(DEFAULTS.dashboard()),
      menuBar: blank(DEFAULTS.menuBar()),
    });
    try {
      const raw = this.storage?.getItem(STORE_KEY);
      if (raw) {
        const saved = JSON.parse(raw) as Partial<Record<DynoSurface, Stored>>;
        // A layout stored before the dashboard moved to the grid has spans: the defaults take its place.
        const dashboard = saved.dashboard && saved.dashboard.placements.every((p) => 'x' in p) ? saved.dashboard : undefined;
        return { ...fresh(), ...saved, ...(dashboard ? {} : { dashboard: blank(DEFAULTS.dashboard()) }) };
      }
    } catch {
      // Unreadable or blocked storage: the defaults.
    }
    return fresh();
  }

  private save(): void {
    try {
      this.storage?.setItem(STORE_KEY, JSON.stringify(this.state));
    } catch {
      // Blocked storage: this window keeps its layouts.
    }
  }

  // ------------------------------------------------------------------------------------------------------ layouts

  view(surface: DynoSurface): SurfaceView {
    return structuredClone({ [surface]: this.viewOf(surface, this.state[surface]) } as unknown as SurfaceView);
  }

  private viewOf(surface: DynoSurface, st: Stored) {
    const { surface: s, kinds } = SURFACES[surface];
    const catalogue = { surface: s, kinds } as CatalogueInfo<DynoSurface, string>;
    const layout = { version: surface === 'dashboard' ? 2 : 1, placements: st.placements, presets: st.presets } as unknown as Layout<W, Cells>;
    const grid = surface === 'dashboard' ? (st.placements as Placement<W, Cells>[]) : null;
    const rects = grid ? readingOrder(grid) : [];
    const arrangements: Arrangement[] = grid
      ? [
          { minWidth: 840, columns: 12, placements: rects.map((p) => ({ id: p.id, rect: rectOf(p) })) },
          { minWidth: 560, columns: 6, placements: derived(grid, 6) },
          { minWidth: 0, columns: 1, placements: derived(grid, 1) },
        ]
      : [];
    const moves = rects.map((p) => ({ id: p.id, moves: this.movesOf(p, rects) }));
    return {
      layout,
      revision: st.revision,
      arrangements,
      moves,
      canRedo: st.redo.length > 0,
      outcome: st.outcome,
      catalogue,
      builtinPresets: surface === 'dashboard' ? builtinPresets() : [],
      notes: [],
      canUndo: st.undo.length > 0,
      editable: true,
    };
  }

  /** Which keys would do something: a plausible imitation (the engine answers by applying each gesture). */
  private movesOf(p: Placement<W, Cells>, all: Placement<W, Cells>[]): Moves {
    const rule = cellsOf(p.widget && kindOf(p.widget));
    const index = all.findIndex((o) => o.id === p.id);
    return {
      left: p.x > 0,
      right: p.x + p.w < COLUMNS,
      up: p.y > 0,
      down: all.some((o) => o.y >= p.y + p.h && o.x < p.x + p.w && p.x < o.x + o.w),
      first: index > 0,
      last: index < all.length - 1,
      wider: stepsMove(rule.widths, p.w, true) !== null && p.x + p.w < COLUMNS,
      narrower: stepsMove(rule.widths, p.w, false) !== null,
      taller: stepsMove(rule.heights, p.h, true) !== null,
      shorter: stepsMove(rule.heights, p.h, false) !== null,
    };
  }

  /** Puts the dashboard on a golden case's layout (placeholder graph widgets), so its gesture answers as the engine did. */
  useGridCase(name: string): void {
    const found = this.cases.find((c) => c.name === name);
    if (!found) throw err(`There is no golden case “${name}”.`);
    const st = this.state.dashboard;
    st.placements = found.items.map((i) => ({ id: i.id, ...i.rect, widget: dw('graph', 'memory') }));
    st.undo = [];
    st.redo = [];
    st.revision += 1;
    st.outcome = null;
    this.layoutListeners.forEach((l) => l('dashboard'));
  }

  /** What a gesture would give on `request.revision`; nothing is stored (the app's `preview_widget_layout`). */
  preview(request: PreviewRequest): PreviewReply<DashboardView> {
    const st = this.state.dashboard;
    if (request.revision !== st.revision) throw stale();
    const copy = structuredClone(st);
    this.arrange(copy, request.gesture);
    copy.revision += 1;
    return { seq: request.seq, view: structuredClone(this.viewOf('dashboard', copy)) as unknown as DashboardView };
  }

  /** Applies a gesture to a dashboard layout: the engine's answer when it is a golden case, else the widget moves alone. */
  private arrange(st: Stored, gesture: GridGesture): void {
    const items = st.placements as Placement<W, Cells>[];
    const id = Object.values(gesture)[0]!.id as string;
    const at = items.findIndex((p) => p.id === id);
    if (at < 0) throw err('That widget is no longer on the dashboard.');
    const golden = this.cases.find((c) => JSON.stringify(c.gesture) === JSON.stringify(gesture) && c.items.length === items.length && c.items.every((i) => items.some((p) => p.id === i.id && JSON.stringify(rectOf(p)) === JSON.stringify(i.rect))));
    let next: Placement<W, Cells>[];
    if (golden) {
      if ('error' in golden.result) throw err(golden.result.error);
      const rects = new Map(golden.result.ok.map((r) => [r.id, r.rect] as const));
      next = items.map((p) => ({ ...p, ...rects.get(p.id)! }));
    } else {
      const p = items[at]!;
      const rule = cellsOf(kindOf(p.widget));
      let r = rectOf(p);
      if ('place' in gesture) {
        const t = gesture.place.rect;
        const w = stepsNearest(rule.widths, t.w);
        r = { x: Math.min(Math.max(0, t.x), COLUMNS - w), y: Math.max(0, Math.min(t.y, MAX_ROWS - 1)), w, h: stepsNearest(rule.heights, t.h) };
      } else if ('nudge' in gesture) {
        const d = gesture.nudge.dir;
        r = { ...r, x: r.x + (d === 'left' ? -1 : d === 'right' ? 1 : 0), y: r.y + (d === 'up' ? -1 : d === 'down' ? 1 : 0) };
        if (r.x < 0 || r.x + r.w > COLUMNS || r.y < 0) throw err('Already at the edge.');
      } else if ('grow' in gesture) {
        const d = gesture.grow.dir;
        const w = d === 'right' ? stepsMove(rule.widths, r.w, true) : d === 'left' ? stepsMove(rule.widths, r.w, false) : r.w;
        const h = d === 'down' ? stepsMove(rule.heights, r.h, true) : d === 'up' ? stepsMove(rule.heights, r.h, false) : r.h;
        if (w === null || h === null || r.x + w > COLUMNS) throw err(d === 'right' || d === 'down' ? "Can't get any larger." : "Can't get any smaller.");
        r = { ...r, w, h };
      } else if ('toFirst' in gesture) r = { ...r, x: 0, y: 0 };
      else r = { ...r, x: 0, y: Math.max(0, ...items.filter((o) => o.id !== id).map((o) => o.y + o.h)) };
      // The mock does not resolve collisions: what the placement lands on moves below it, one pass, then gravity.
      let moved = items.map((o) => (o.id === id ? { ...o, ...r } : o));
      moved = moved.map((o) => (o.id !== id && overlaps(rectOf(o), r) ? { ...o, y: r.y + r.h } : o));
      next = compactCells(moved);
    }
    st.placements = next;
    const order = readingOrder(next);
    const i = order.findIndex((p) => p.id === id);
    const me = order[i]!;
    st.outcome = { id, position: i + 1, of: order.length, after: order[i - 1]?.id ?? null, before: order[i + 1]?.id ?? null, w: me.w, h: me.h, limit: null };
  }

  edit(request: SurfaceEdit, baseRevision: number | null = null): SurfaceView {
    const surface = Object.keys(request)[0] as DynoSurface;
    const edit = (request as unknown as Record<string, Edit<W>>)[surface]!;
    const spec = SURFACES[surface];
    const st = this.state[surface];
    if (baseRevision !== null && baseRevision !== st.revision) throw stale();
    const noun = spec.surface.noun;
    const grid = surface === 'dashboard';
    const placements = st.placements as Placement<W>[];
    const find = (id: string): number => {
      const i = placements.findIndex((p) => p.id === id);
      if (i < 0) throw err(`That widget is no longer in the ${noun}.`);
      return i;
    };
    const before = st.placements;
    st.outcome = null;
    const next = (list: AnyPlacement[], step = true): void => {
      st.placements = grid ? compactCells(list as Placement<W, Cells>[]) : list;
      if (step && JSON.stringify(st.placements) !== JSON.stringify(before)) {
        st.undo.push(before);
        if (st.undo.length > UNDO_DEPTH) st.undo.shift();
        st.redo = [];
      }
    };
    const kindInfo = (k: string) => spec.kinds.find((x) => x.kind === k);
    if (edit === 'resetToDefaults') next(DEFAULTS[surface]());
    else if (edit === 'undo') {
      const prev = st.undo.pop();
      if (!prev) throw err('There is nothing to undo.');
      st.redo.push(before);
      st.placements = prev;
    } else if (edit === 'redo') {
      const again = st.redo.pop();
      if (!again) throw err('There is nothing to redo.');
      st.undo.push(before);
      st.placements = again;
    } else if ('arrange' in edit) {
      if (!grid) throw err('This surface has no grid.');
      this.arrange(st, edit.arrange.gesture);
      if (JSON.stringify(st.placements) !== JSON.stringify(before)) {
        st.undo.push(before);
        if (st.undo.length > UNDO_DEPTH) st.undo.shift();
        st.redo = [];
      }
    } else if ('add' in edit) {
      const { widget, span, at } = edit.add;
      const kind = kindOf(widget);
      const k = kindInfo(kind);
      if (!k) throw err(`The ${noun} has no widget “${kind}”.`);
      if (placements.length >= spec.surface.maxPlacements) throw err(`At most ${spec.surface.maxPlacements} widgets fit in the ${noun}; remove one first.`);
      if (placements.filter((p) => kindOf(p.widget) === kind).length >= k.maxInstances) throw err(`The ${noun} can show ${k.title} only ${k.maxInstances} time${k.maxInstances === 1 ? '' : 's'}; remove one first.`);
      const list = [...st.placements];
      if (grid) {
        if (span !== null) throw err('A grid widget has a size, not a span.');
        const rule = cellsOf(kind);
        const taken = list as Placement<W, Cells>[];
        let spot: Rect | null = null;
        for (let y = 0; y < MAX_ROWS && !spot; y++)
          for (let x = 0; x + rule.defaultW <= COLUMNS && !spot; x++) {
            const r = { x, y, w: rule.defaultW, h: rule.defaultH };
            if (!taken.some((p) => overlaps(r, rectOf(p)))) spot = r;
          }
        list.push({ id: '', ...spot!, widget } as unknown as AnyPlacement);
      } else {
        const first = (k.size as Extract<KindSize, { spans: unknown }>).spans.spans[0]!;
        list.splice(at === null ? list.length : Math.max(0, Math.min(list.length, at)), 0, { id: '', span: span ?? first, widget } as AnyPlacement);
      }
      next(this.withIds(list as Placement<W>[]) as AnyPlacement[]);
    } else if ('remove' in edit) {
      const i = find(edit.remove.id);
      if (placements.length <= spec.surface.minPlacements) throw err(`At least ${spec.surface.minPlacements} widget${spec.surface.minPlacements === 1 ? ' is' : 's are'} needed in the ${noun}.`);
      next(st.placements.filter((_, j) => j !== i));
    } else if ('move' in edit) {
      if (grid) throw err('Arrange widgets on this surface by dragging.');
      const i = find(edit.move.id);
      const list = [...st.placements];
      const [item] = list.splice(i, 1);
      list.splice(Math.max(0, Math.min(list.length, edit.move.to)), 0, item!);
      next(list);
    } else if ('resize' in edit) {
      if (grid) throw err('Resize widgets on this surface by dragging.');
      const i = find(edit.resize.id);
      const k = kindInfo(kindOf(placements[i]!.widget));
      const spans = k && 'spans' in k.size ? k.size.spans.spans : [];
      if (!spans.includes(edit.resize.span)) throw err(`That width is not available for ${k?.title ?? 'this widget'}.`);
      next(placements.map((p, j) => (j === i ? { ...p, span: edit.resize.span } : p)));
    } else if ('replace' in edit) {
      const i = find(edit.replace.id);
      if (kindOf(edit.replace.widget) !== kindOf(placements[i]!.widget)) throw err('A widget keeps its kind; remove it and add another instead.');
      next(st.placements.map((p, j) => (j === i ? ({ ...p, widget: edit.replace.widget } as AnyPlacement) : p)));
    } else if ('applyPreset' in edit) {
      const preset = [...(grid ? builtinPresets() : []), ...(st.presets as Preset<W>[])].find((p) => p.id === edit.applyPreset.id);
      if (!preset) throw err('That preset no longer exists.');
      next(structuredClone(preset.placements) as AnyPlacement[]);
    } else if ('savePreset' in edit) {
      const name = edit.savePreset.name.trim();
      if (!name) throw err('Give the preset a name.');
      if (st.presets.length >= spec.surface.maxUserPresets) throw err(`You can keep at most ${spec.surface.maxUserPresets} presets; delete one first.`);
      const id = `custom-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;
      st.presets = [...(st.presets as Preset<W>[]).filter((p) => p.id !== id), { id, name: name.slice(0, 64), builtin: false, placements: structuredClone(st.placements) as Placement<W>[] }];
    } else if ('deletePreset' in edit) {
      if (!(st.presets as Preset<W>[]).some((p) => p.id === edit.deletePreset.id)) throw err('That preset cannot be deleted.');
      st.presets = (st.presets as Preset<W>[]).filter((p) => p.id !== edit.deletePreset.id);
    }
    st.revision += 1;
    this.save();
    for (const l of this.layoutListeners) l(surface);
    if (surface === 'popover') this.pushValues();
    return this.view(surface);
  }

  private withIds(list: Placement<W>[]): Placement<W>[] {
    const taken = new Set(list.filter((p) => p.id).map((p) => p.id));
    return list.map((p) => {
      if (p.id) return p;
      const base = kebab(kindOf(p.widget));
      let n = 1;
      let id = base;
      while (taken.has(id)) id = `${base}-${++n}`;
      taken.add(id);
      return { ...p, id };
    });
  }

  onLayout(listener: (s: DynoSurface) => void): () => void {
    this.layoutListeners.add(listener);
    return () => this.layoutListeners.delete(listener);
  }

  // ------------------------------------------------------------------------------------------------- popup values

  /** Shows a popup kind in a state (tests and screenshots). */
  setState(kind: PopoverKind, state: MockWidgetState): void {
    this.states[kind] = state;
    this.pushValues();
  }

  /** Stops or restores the agent as the popup sees it. */
  setAgentStopped(stopped: boolean): void {
    this.agentStopped = stopped;
    this.pushValues();
  }

  onValues(listener: (v: PopoverSurfaceValues) => void): () => void {
    this.valueListeners.add(listener);
    return () => this.valueListeners.delete(listener);
  }

  private pushValues(): void {
    const v = this.values();
    for (const l of this.valueListeners) l(v);
  }

  values(): PopoverSurfaceValues {
    const at = this.now();
    const stoppedAt = at - 3 * 60_000;
    const placements: PlacementState<PopoverValue, WidgetActionId>[] = [];
    for (const p of this.state.popover.placements as Placement<PopoverWidget>[]) {
      const kind = kindOf(p.widget) as PopoverKind;
      const state = this.states[kind] ?? 'ready';
      if (state === 'loading') continue;
      placements.push({ id: p.id, state: this.stateOf(p, kind, state, at) });
    }
    const notice = this.agentStopped
      ? { tone: 'danger' as const, title: 'Agent stopped', detail: `Readings paused at ${clockOf(stoppedAt)}`, action: { action: 'restartAgent' as const, label: 'Restart agent' } }
      : null;
    if (this.agentStopped) {
      for (const p of placements) if (p.state.state === 'ready') p.state = { ...p.state, staleSince: stoppedAt };
    }
    return { notice, noticeKind: notice ? 'agentStopped' : null, placements };
  }

  private stateOf(p: Placement<PopoverWidget>, kind: PopoverKind, state: MockWidgetState, at: number): WidgetState<PopoverValue, WidgetActionId> {
    const retry: WidgetActionId = { retry: p.id };
    const ready = (value: PopoverValue, stale = false): WidgetState<PopoverValue, WidgetActionId> => ({ state: 'ready', value, asOf: stale ? at - 4 * 60_000 : at, staleSince: stale ? at - 4 * 60_000 : null });
    const empty = (title: string, detail: string, label: string, action: WidgetActionId, calm = false): WidgetState<PopoverValue, WidgetActionId> => ({ state: 'empty', title, detail, next: { action, label }, calm });
    const error = (title: string, cause: string): WidgetState<PopoverValue, WidgetActionId> => ({ state: 'error', title, cause, next: { action: retry, label: 'Retry' } });
    // A minute of readings that wander like real ones (seeded noise, not a sine wave).
    const wave = (base: number, amp: number, seed = 1) => Array.from({ length: 60 }, (_, i) => Math.round((base + wobble(i, seed, 9) * amp) * 10) / 10);
    const stale = state === 'stale';
    switch (kind) {
      case 'activeAlarms':
        if (state === 'empty') return empty('No active alarms', 'All clear.', 'Open alarms', { openPage: 'alarms' }, true);
        if (state === 'error') return error("Couldn't read alarms", "The alarm store didn't answer.");
        if (this.longLists) {
          const rows = Array.from({ length: POPOVER_LIST_MAX }, (_, i) => ({ id: i + 1, title: `Invented alarm ${i + 1}: a rather long title that has to be cut short on one line`, level: (['critical', 'warning', 'info'] as const)[i % 3]!, since: at - (i + 1) * 3 * 60_000, acknowledged: i % 4 === 0 }));
          return ready({ activeAlarms: { rows, more: 5 } }, stale);
        }
        return ready({ activeAlarms: { rows: [{ id: 1, title: 'Memory pressure is high', level: 'warning', since: at - 4 * 60_000, acknowledged: false }, { id: 2, title: 'Backup is out of date', level: 'info', since: at - 26 * 3_600_000, acknowledged: true }], more: 0 } }, stale);
      case 'diskSpace': {
        if (state === 'empty') return empty("See Time Machine's space when it's connected", "The disk isn't connected now.", 'Choose another volume', 'chooseVolume');
        if (state === 'error') return error("Couldn't read volumes", "The volume list didn't answer.");
        const GB = 1e9;
        if (this.longLists) {
          const rows = Array.from({ length: 30 }, (_, i) => ({ id: `/Volumes/V${i}`, name: `Invented volume ${i + 1}`, usedPercent: (i * 7) % 100, freeBytes: (100 - ((i * 7) % 100)) * GB, totalBytes: 100 * GB, tone: 'ok' as const, reachable: true }));
          return ready({ diskSpace: { rows, more: 0 } }, stale);
        }
        return ready({ diskSpace: { rows: [{ id: '/', name: 'Macintosh HD', usedPercent: 82, freeBytes: 182 * GB, totalBytes: 994 * GB, tone: 'ok', reachable: true }, { id: '/Volumes/Projects', name: 'Projects', usedPercent: 96, freeBytes: 19 * GB, totalBytes: 500 * GB, tone: 'bad', reachable: true }], more: 0 } }, stale);
      }
      case 'cpuMemory':
        return ready({ cpuMemory: { cpuPercent: 76, cpuTone: 'warn', memoryUsed: 16.3 * 1024 ** 3, memoryTotal: 32 * 1024 ** 3, memoryPercent: 51, memoryTone: 'ok', cpuMinute: wave(70, 12) } }, stale);
      case 'network':
        if (state === 'empty') return empty('See traffic when online', 'Offline now.', 'Open network', { openPage: 'network' });
        return ready({ network: { rxBps: 2.4e6, txBps: 310e3, rxMinute: wave(2e6, 5e5, 2), txMinute: wave(3e5, 5e4, 3) } }, stale);
      case 'backupHealth':
        if (state === 'empty') return empty('Know when backups ran', 'No backup set up.', 'Set up checks', 'setUpBackupChecks');
        if (state === 'error') return error("Couldn't check", 'tmutil failed.');
        if (state === 'working') return { state: 'working', title: 'Checking backup', phase: '', progress: { count: { done: 2, unit: 'items' } }, last: null, cancel: null };
        return ready({ backupHealth: { name: 'Time Machine', state: 'healthy', lastBackup: at - 2 * 3_600_000, summary: 'Time Machine finished a backup.', tools: 1 } }, stale);
      case 'lastIssue':
        if (state === 'empty') return empty('No issues in the last 7 days', 'Dyno reads the system logs for crashes and hangs.', 'Open issues', { openPage: 'issues' }, true);
        if (state === 'error') return error("Couldn't read issues", 'The issue index is unavailable.');
        if (state === 'working') return { state: 'working', title: 'Last issue', phase: 'Reading system logs · Indexing', progress: { count: { done: 12400, unit: 'events' } }, last: null, cancel: null };
        return ready({ lastIssue: { title: 'Disk write errors on disk4', app: 'disk4', kind: 'Disk error', level: 'critical', at: at - 86_400_000, days: 7 } }, stale);
      case 'pinnedCounters':
        if (state === 'empty') return empty('Watch your own counters here', 'Pin counters in Dyno; the first 4 show here.', 'Pin counters', 'pinCounters');
        if (state === 'error') return error("Couldn't read counters", 'None of the 3 pinned counters answered.');
        return ready({ pinnedCounters: { rows: [{ path: '\\GPU\\Memory in use', name: 'GPU memory in use', value: 3.2e9, unit: 'bytes', percent: null, tone: 'ok' }, { path: '\\Memory\\Compressed', name: 'Compressed memory', value: 1.14e9, unit: 'bytes', percent: null, tone: 'ok' }, { path: '\\System\\Context switches per second', name: 'Context switches per second', value: null, unit: 'perSec', percent: null, tone: 'ok' }], more: 0 } }, stale);
      case 'topApps':
        return ready({ topApps: { rows: [{ name: 'Rust build (cargo)', cpuPercent: 42 }, { name: 'Google Chrome', cpuPercent: 6.8 }, { name: 'WindowServer', cpuPercent: 4.1 }] } }, stale);
    }
  }
}

const METRIC_TITLES: [Metric, string][] = [
  ['cpuTotal', 'CPU'], ['cpuPerCore', 'CPU per core'], ['memory', 'Memory'], ['memoryPressure', 'Memory pressure'], ['swap', 'Swap'],
  ['diskActivity', 'Disk activity'], ['diskFree', 'Disk free'], ['networkDown', 'Network down'], ['networkUp', 'Network up'],
  ['gpuLoad', 'GPU'], ['temperature', 'Temperature'], ['fan', 'Fan'], ['battery', 'Battery'],
];
const PERCENT: Metric[] = ['cpuTotal', 'memory', 'swap', 'diskFree', 'gpuLoad', 'battery'];

/** Which metrics a dashboard kind accepts (a plausible imitation of the engine's rule). */
function accepted(kind: DashboardKind): Metric[] {
  if (kind === 'heatmap') return ['cpuPerCore'];
  if (kind === 'ring') return PERCENT;
  if (kind === 'topProcesses') return ['cpuTotal', 'memory', 'diskActivity', 'networkUp', 'networkDown', 'gpuLoad'];
  if (['health', 'issues', 'cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend'].includes(kind)) return ['cpuTotal'];
  return METRIC_TITLES.map(([m]) => m);
}

/** What the tray draws of a number item (the engine's `number_support`, imitated: macOS has both formats; Windows and Linux one, with the label only from 22 px). */
export function mockNumberSupport(os: 'macos' | 'windows' | 'linux', px: number): WidgetChoices['number'] {
  if (os === 'macos') return { formats: ['stacked', 'line'], labelInIcon: true, fixed: null, note: null };
  const label = px >= 22;
  return {
    formats: ['stacked'],
    labelInIcon: label,
    fixed: label ? 'Label over value' : 'Value only (label in the tooltip)',
    note: label
      ? 'Here, tray icons have one number layout: the label over the value. The label is also in the tooltip.'
      : `Here, tray icons are ${px} px, too small for a label above the number. The label shows in the tooltip; with display scaling at 150 % or more there is room for it.`,
  };
}

/** What the choosers offer (the engine's `widget_choices`, imitated). */
export function mockWidgetChoices(numberSupport: WidgetChoices['number'] = mockNumberSupport('macos', 18)): WidgetChoices {
  const title = (m: Metric) => METRIC_TITLES.find(([x]) => x === m)?.[1] ?? String(m);
  const choice = (metric: Metric) => ({ metric, title: title(metric) });
  return {
    popover: POPOVER_KINDS.map((k) => ({ kind: k.kind as PopoverKind, default: (k.kind === 'diskSpace' ? { diskSpace: { volume: 'all' } } : k.kind) as PopoverWidget })),
    dashboard: DASHBOARD_KINDS.map((k) => {
      const metrics = accepted(k.kind as DashboardKind).map(choice);
      return { kind: k.kind as DashboardKind, default: dw(k.kind as DashboardKind, metrics[0]!.metric), metrics };
    }),
    menuBar: MENU_KINDS.map((k) => {
      const metric: Metric = 'cpuTotal';
      const kind = k.kind as MenuBarKind;
      return { kind, default: (kind === 'number' ? { number: { metric, unit: '%', label: '', format: 'stacked' } } : { [kind]: { metric } }) as MenuBarItem };
    }),
    menuBarMetrics: METRIC_TITLES.map(([m]) => choice(m)),
    number: numberSupport,
  };
}

/** The numbers of the mock's live sample that its metric readings imitate. */
export interface MockSystem {
  cpu: number;
  memory: number;
  battery?: number | string;
  gpu?: number | string;
  /** The disk's busy percentage, or why it is not reported. */
  diskBusy?: number | string;
  /** The first volume's free share, in percent. */
  diskFree?: number;
  rxBps?: number;
  txBps?: number;
}

/** A rate the way the engine words it (`rate_reading`): one decimal above a kilobyte, a unit that fits. */
function rateReading(bps: number, query: ReadingQuery): MetricReading {
  const v = Number.isFinite(bps) ? Math.max(0, bps) : 0;
  const [text, unit] = v >= 1e9 ? [(v / 1e9).toFixed(1), 'GB/s'] : v >= 1e6 ? [(v / 1e6).toFixed(1), 'MB/s'] : v >= 1e3 ? [(v / 1e3).toFixed(1), 'KB/s'] : [v.toFixed(0), 'B/s'];
  const { warning, critical } = query.thresholds;
  const tone = critical !== null && v >= critical ? 'bad' : warning !== null && v >= warning ? 'warn' : 'ok';
  return { value: v, text, unit, percent: null, tone, reason: null };
}

/** A reading of `query` from the mock's numbers (a plausible imitation of the engine's `read_metric`; a metric the mock has no number for says so). */
export function mockMetricReading(query: ReadingQuery, system: MockSystem): MetricReading {
  if (query.metric === 'networkDown' && system.rxBps !== undefined) return rateReading(system.rxBps, query);
  if (query.metric === 'networkUp' && system.txBps !== undefined) return rateReading(system.txBps, query);
  const raw =
    query.metric === 'cpuTotal' ? system.cpu
    : query.metric === 'memory' ? system.memory
    : query.metric === 'battery' ? system.battery
    : query.metric === 'gpuLoad' ? system.gpu
    : query.metric === 'diskActivity' ? system.diskBusy
    : query.metric === 'diskFree' ? system.diskFree
    : undefined;
  if (typeof raw === 'string') return { value: null, text: '—', unit: '', percent: null, tone: 'ok', reason: raw };
  const value = raw ?? null;
  if (value === null) return { value: null, text: '—', unit: '', percent: null, tone: 'ok', reason: 'The mock has no live reading for this metric.' };
  const { warning, critical } = query.thresholds;
  const tone = critical !== null && value >= critical ? 'bad' : warning !== null && value >= warning ? 'warn' : 'ok';
  return { value, text: value.toFixed(0), unit: '%', percent: value, tone, reason: null };
}
