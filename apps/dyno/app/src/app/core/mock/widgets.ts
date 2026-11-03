import type { CommandError } from '../api.types';
import type {
  CatalogueInfo,
  DashboardKind,
  DashboardWidget,
  DynoSurface,
  Edit,
  KindInfo,
  Layout,
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
import type { PopoverSurfaceValues } from '../widget-types';
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

const info = (kind: AnyKind, title: string, summary: string, icon: string, spans: number[], maxInstances = 1, refresh: Refresh = 'live') => ({ kind, title, summary, icon, spans, maxInstances, refresh });

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

const DASH_SPANS = [6, 3, 12];
const DASHBOARD_KINDS = [
  info('graph', 'Graph', 'A line over the last minutes.', 'graph', DASH_SPANS, 32),
  info('bigNumber', 'Big number', 'One number, large.', 'number', DASH_SPANS, 32),
  info('bars', 'Bars', 'A level against its limit.', 'bars', DASH_SPANS, 32),
  info('ring', 'Ring', 'A percentage as a ring.', 'ring', DASH_SPANS, 32),
  info('heatmap', 'Per-core heatmap', 'One cell per core.', 'heatmap', DASH_SPANS, 32),
  info('topProcesses', 'Top processes', 'The apps using the most.', 'apps', DASH_SPANS, 32),
  info('issues', 'Issues', 'Unusual patterns found.', 'issue', DASH_SPANS, 32, 'onChange'),
  info('health', 'Health', 'The health score.', 'health', DASH_SPANS, 32),
  info('cpu', 'CPU', 'Load, per-core view and details.', 'cpu', DASH_SPANS, 32),
  info('diagnosis', 'Why slow', 'What is slowing the computer, in words.', 'diagnosis', DASH_SPANS, 32, { every: { secs: 10 } }),
  info('events', 'Recent events', 'Alarms and what Dyno did.', 'events', DASH_SPANS, 32, 'onChange'),
  info('history', 'History', 'A chart with events marked.', 'history', DASH_SPANS, 32, 'onChange'),
  info('consumers', 'Biggest consumers', 'Biggest consumers over a day.', 'consumers', DASH_SPANS, 32, { every: { secs: 60 } }),
  info('diskTrend', 'Disk space trend', 'When the disks will be full.', 'disk', DASH_SPANS, 32, { every: { secs: 60 } }),
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

function builtinPresets(): Preset<DashboardWidget>[] {
  return Object.entries(DASH_PRESETS).map(([id, [name, items]]) => ({ id, name, builtin: true, placements: assign(items) }));
}

const DEFAULTS: Record<DynoSurface, () => Placement<W>[]> = {
  popover: () => assign<W>([[2, 'activeAlarms'], [2, { diskSpace: { volume: 'all' } }], [2, 'cpuMemory']]),
  dashboard: () => assign<W>(DASH_PRESETS['overview']![1]),
  menuBar: () =>
    assign<W>([
      [1, { number: { metric: 'cpuTotal', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'memory', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'networkDown', unit: '', label: '', format: 'stacked' } }],
      [1, { number: { metric: 'temperature', unit: '', label: '', format: 'stacked' } }],
    ]),
};

interface Stored {
  placements: Placement<W>[];
  presets: Preset<W>[];
}

/** HH:MM in the viewer's zone, the way the widget frame says "Paused at 14:28" (one time across the notice and every widget). */
function clockOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

const STORE_KEY = 'dyno-mock-widgets';

/** Options of the mock's widgets. */
export interface MockWidgetsOptions {
  /** Where layouts are kept so a reload (and another window of the same browser) sees them; `null` keeps them in memory. */
  storage?: Storage | null;
  /** The state each popup kind is shown in. */
  popup?: Partial<Record<PopoverKind, MockWidgetState>>;
  /** Show the popup as if the agent had stopped. */
  agentStopped?: boolean;
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
  private previous: Record<DynoSurface, Placement<W>[] | null> = { popover: null, dashboard: null, menuBar: null };
  private readonly states: Partial<Record<PopoverKind, MockWidgetState>>;
  private agentStopped: boolean;
  private readonly layoutListeners = new Set<(s: DynoSurface) => void>();
  private readonly valueListeners = new Set<(v: PopoverSurfaceValues) => void>();

  constructor(options: MockWidgetsOptions = {}) {
    this.storage = options.storage ?? null;
    this.now = options.now ?? Date.now;
    this.states = { ...options.popup };
    this.agentStopped = options.agentStopped ?? false;
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
    const fresh = (): Record<DynoSurface, Stored> => ({
      popover: { placements: DEFAULTS.popover(), presets: [] },
      dashboard: { placements: DEFAULTS.dashboard(), presets: [] },
      menuBar: { placements: DEFAULTS.menuBar(), presets: [] },
    });
    try {
      const raw = this.storage?.getItem(STORE_KEY);
      if (raw) return { ...fresh(), ...(JSON.parse(raw) as Partial<Record<DynoSurface, Stored>>) };
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
    const { surface: s, kinds } = SURFACES[surface];
    const st = this.state[surface];
    const catalogue = { surface: s, kinds } as CatalogueInfo<DynoSurface, string>;
    const layout = { version: 1, placements: st.placements, presets: st.presets } as Layout<W>;
    const view = { layout, catalogue, builtinPresets: surface === 'dashboard' ? builtinPresets() : [], notes: [], canUndo: this.previous[surface] !== null, editable: true };
    return structuredClone({ [surface]: view } as unknown as SurfaceView);
  }

  edit(request: SurfaceEdit): SurfaceView {
    const surface = Object.keys(request)[0] as DynoSurface;
    const edit = (request as unknown as Record<string, Edit<W>>)[surface]!;
    const spec = SURFACES[surface];
    const st = this.state[surface];
    const noun = spec.surface.noun;
    const placements = st.placements;
    const find = (id: string): number => {
      const i = placements.findIndex((p) => p.id === id);
      if (i < 0) throw err(`That widget is no longer in the ${noun}.`);
      return i;
    };
    const next = (list: Placement<W>[], undoable: boolean | 'keep'): void => {
      if (undoable === true) this.previous[surface] = placements;
      else if (undoable === false) this.previous[surface] = null;
      st.placements = list;
    };
    const kindInfo = (k: string) => spec.kinds.find((x) => x.kind === k);
    if (edit === 'resetToDefaults') next(DEFAULTS[surface](), true);
    else if (edit === 'undo') {
      const prev = this.previous[surface];
      if (!prev) throw err('There is nothing to undo.');
      this.previous[surface] = null;
      st.placements = prev;
    } else if ('add' in edit) {
      const { widget, span, at } = edit.add;
      const kind = kindOf(widget);
      const k = kindInfo(kind);
      if (!k) throw err(`The ${noun} has no widget “${kind}”.`);
      if (placements.length >= spec.surface.maxPlacements) throw err(`At most ${spec.surface.maxPlacements} widgets fit in the ${noun}; remove one first.`);
      if (placements.filter((p) => kindOf(p.widget) === kind).length >= k.maxInstances) throw err(`The ${noun} can show ${k.title} only ${k.maxInstances} time${k.maxInstances === 1 ? '' : 's'}.`);
      const list = [...placements];
      list.splice(at === null ? list.length : Math.max(0, Math.min(list.length, at)), 0, { id: '', span: span ?? k.spans[0]!, widget });
      next(this.withIds(list), false);
    } else if ('remove' in edit) {
      const i = find(edit.remove.id);
      if (placements.length <= spec.surface.minPlacements) throw err(`At least ${spec.surface.minPlacements} widget${spec.surface.minPlacements === 1 ? ' is' : 's are'} needed in the ${noun}.`);
      next(placements.filter((_, j) => j !== i), true);
    } else if ('move' in edit) {
      const i = find(edit.move.id);
      const list = [...placements];
      const [item] = list.splice(i, 1);
      list.splice(Math.max(0, Math.min(list.length, edit.move.to)), 0, item!);
      next(list, false);
    } else if ('resize' in edit) {
      const i = find(edit.resize.id);
      const k = kindInfo(kindOf(placements[i]!.widget));
      if (!k?.spans.includes(edit.resize.span)) throw err(`That width is not available for ${k?.title ?? 'this widget'}.`);
      next(placements.map((p, j) => (j === i ? { ...p, span: edit.resize.span } : p)), false);
    } else if ('replace' in edit) {
      const i = find(edit.replace.id);
      if (kindOf(edit.replace.widget) !== kindOf(placements[i]!.widget)) throw err('A widget keeps its kind; remove it and add another instead.');
      next(placements.map((p, j) => (j === i ? { ...p, widget: edit.replace.widget } : p)), false);
    } else if ('applyPreset' in edit) {
      const preset = [...builtinPresets(), ...st.presets].find((p) => p.id === edit.applyPreset.id);
      if (!preset) throw err('That preset no longer exists.');
      next(structuredClone(preset.placements) as Placement<W>[], true);
    } else if ('savePreset' in edit) {
      const name = edit.savePreset.name.trim();
      if (!name) throw err('Give the preset a name.');
      if (st.presets.length >= spec.surface.maxUserPresets) throw err(`You can keep at most ${spec.surface.maxUserPresets} presets; delete one first.`);
      const id = `custom-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;
      st.presets = [...st.presets.filter((p) => p.id !== id), { id, name: name.slice(0, 64), builtin: false, placements: structuredClone(placements) }];
    } else if ('deletePreset' in edit) {
      if (!st.presets.some((p) => p.id === edit.deletePreset.id)) throw err('That preset cannot be deleted.');
      st.presets = st.presets.filter((p) => p.id !== edit.deletePreset.id);
    }
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
        return ready({ activeAlarms: { rows: [{ id: 1, title: 'Memory pressure is high', level: 'warning', since: at - 4 * 60_000, acknowledged: false }], more: 4 } }, stale);
      case 'diskSpace': {
        if (state === 'empty') return empty("See Time Machine's space when it's connected", "The disk isn't connected now.", 'Choose another volume', 'chooseVolume');
        if (state === 'error') return error("Couldn't read volumes", "The volume list didn't answer.");
        const GB = 1e9;
        return ready({ diskSpace: { rows: [{ id: '/', name: 'Macintosh HD', usedPercent: 82, freeBytes: 182 * GB, totalBytes: 994 * GB, tone: 'ok', reachable: true }, { id: '/Volumes/Projects', name: 'Projects', usedPercent: 96, freeBytes: 19 * GB, totalBytes: 500 * GB, tone: 'bad', reachable: true }], more: 2 } }, stale);
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

/** What the choosers offer (the engine's `widget_choices`, imitated). */
export function mockWidgetChoices(): WidgetChoices {
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
