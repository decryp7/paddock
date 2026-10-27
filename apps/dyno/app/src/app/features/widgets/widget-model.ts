import type {
  CounterReading,
  DashboardLayout,
  NamedPreset,
  SystemSample,
  TrayItem,
  Widget,
  WidgetKind,
  WidgetMetric,
  WidgetSize,
  WidgetThresholds,
} from '../../core/api.types';
import { formatBytes, formatPercent, formatRate, splitValue } from '../../shared/format';
import { counterTitle, formatCounter, readingNumber, readingReason } from '../counters/counters-model';
import type { Series } from '../../state/live.store';

/** The limits the engine enforces (`dyno_core::widgets`); the UI keeps to them so a save is never rejected for them. */
export const MAX_WIDGETS = 32;
export const MAX_TRAY_ITEMS = 4;

export const KIND_LABEL: Record<WidgetKind, string> = {
  graph: 'Graph',
  bigNumber: 'Big number',
  bars: 'Bars',
  ring: 'Ring',
  heatmap: 'Per-core heatmap',
  topProcesses: 'Top processes',
  issues: 'Issues',
  health: 'Health',
  cpu: 'CPU',
  diagnosis: 'Why slow',
  events: 'Recent events',
  history: 'History',
  consumers: 'Biggest consumers',
  diskTrend: 'Disk space trend',
};

export const KIND_HINT: Record<WidgetKind, string> = {
  graph: 'A line over the last minutes.',
  bigNumber: 'One number, large.',
  bars: 'A level against its limit.',
  ring: 'A percentage as a ring.',
  heatmap: 'One cell per core.',
  topProcesses: 'The apps using the most.',
  issues: 'Unusual patterns found.',
  health: 'The health score.',
  cpu: 'Load, per-core view and details.',
  diagnosis: 'What is slowing the PC, in words.',
  events: 'Alarms and what Dyno did.',
  history: 'A chart with events marked.',
  consumers: 'Biggest consumers over a day.',
  diskTrend: 'When the disks will be full.',
};

export const KINDS = Object.keys(KIND_LABEL) as WidgetKind[];
export const SIZES: WidgetSize[] = ['s', 'm', 'l'];
export const SIZE_LABEL: Record<WidgetSize, string> = { s: 'Small', m: 'Medium', l: 'Large' };
/** Columns on the 12-column board. */
export const SIZE_COLUMNS: Record<WidgetSize, number> = { s: 3, m: 6, l: 12 };

/** The metrics offered when adding, in display order (the two parameterised ones are not offered here). */
export const METRICS: { metric: WidgetMetric; label: string }[] = [
  { metric: 'cpuTotal', label: 'CPU' },
  { metric: 'cpuPerCore', label: 'CPU per core' },
  { metric: 'memory', label: 'Memory' },
  { metric: 'memoryPressure', label: 'Memory pressure' },
  { metric: 'swap', label: 'Swap' },
  { metric: 'diskActivity', label: 'Disk activity' },
  { metric: 'diskFree', label: 'Disk free' },
  { metric: 'networkDown', label: 'Network down' },
  { metric: 'networkUp', label: 'Network up' },
  { metric: 'gpuLoad', label: 'GPU' },
  { metric: 'temperature', label: 'Temperature' },
  { metric: 'fan', label: 'Fan' },
  { metric: 'battery', label: 'Battery' },
];

export function metricLabel(m: WidgetMetric): string {
  if (typeof m === 'string') return METRICS.find((x) => x.metric === m)?.label ?? m;
  if ('process' in m) return `Process ${m.process}`;
  return m.perfCounter.startsWith('\\') ? counterTitle(m.perfCounter) : `Counter ${m.perfCounter}`;
}

/** Whether the metric is a 0-100 percentage. */
export function isPercent(m: WidgetMetric): boolean {
  return m === 'cpuTotal' || m === 'memory' || m === 'swap' || m === 'diskFree' || m === 'gpuLoad' || m === 'battery';
}

/** Kinds that draw a panel of their own and ignore the metric. */
export const PANEL_KINDS: readonly WidgetKind[] = ['health', 'issues', 'cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend'];

/** Whether a widget kind can show a metric (the same rule as `WidgetKind::accepts` in the engine). */
export function accepts(kind: WidgetKind, m: WidgetMetric): boolean {
  switch (kind) {
    case 'heatmap':
      return m === 'cpuPerCore';
    case 'ring':
      return isPercent(m);
    case 'topProcesses':
      return m === 'cpuTotal' || m === 'memory' || m === 'diskActivity' || m === 'networkUp' || m === 'networkDown' || m === 'gpuLoad';
    default:
      return true;
  }
}

/** The metric a freshly added widget of `kind` starts with. */
export function defaultMetric(kind: WidgetKind): WidgetMetric {
  return kind === 'heatmap' ? 'cpuPerCore' : 'cpuTotal';
}

export const NO_THRESHOLDS: WidgetThresholds = { warning: null, critical: null };

/** A new widget with default options and a unique id. */
export function newWidget(kind: WidgetKind, metric: WidgetMetric, existing: readonly Widget[]): Widget {
  let n = existing.length + 1;
  const base = kind;
  while (existing.some((w) => w.id === `${base}-${n.toString()}`)) n++;
  return {
    id: `${base}-${n.toString()}`,
    kind,
    metric,
    size: kind === 'topProcesses' || kind === 'heatmap' || kind === 'history' || kind === 'diskTrend' ? 'l' : kind === 'cpu' || kind === 'diagnosis' || kind === 'events' || kind === 'consumers' ? 'm' : kind === 'graph' ? 'm' : 's',
    options: { rangeSecs: 300, showLabel: true, fill: true, accent: null },
    thresholds: { ...NO_THRESHOLDS },
  };
}

/** Moves the widget at `from` to position `to` (clamped); the input is not changed. */
export function moveWidget<T>(list: readonly T[], from: number, to: number): T[] {
  if (from < 0 || from >= list.length) return [...list];
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(Math.max(0, Math.min(next.length, to)), 0, item!);
  return next;
}

export function resizeWidget(list: readonly Widget[], id: string, size: WidgetSize): Widget[] {
  return list.map((w) => (w.id === id ? { ...w, size } : w));
}

export function removeWidget(list: readonly Widget[], id: string): Widget[] {
  return list.filter((w) => w.id !== id);
}

/** The layout showing a preset's widgets (the presets themselves are kept). */
export function applyPreset(layout: DashboardLayout, id: string): DashboardLayout {
  const preset = layout.presets.find((p) => p.id === id);
  return preset ? { ...layout, widgets: structuredClone(preset.widgets) } : layout;
}

/** The status of a value against thresholds: `ok` when none is crossed (neutral), `warn`, `bad`. */
export function thresholdTone(value: number | null, t: WidgetThresholds): 'ok' | 'warn' | 'bad' {
  if (value === null) return 'ok';
  if (t.critical !== null && value >= t.critical) return 'bad';
  if (t.warning !== null && value >= t.warning) return 'warn';
  return 'ok';
}

/** One reading of a metric: a number for tones and charts, and how it reads. */
export interface Reading {
  /** The numeric value (percent for percent metrics), or `null` when unavailable. */
  value: number | null;
  /** Number and unit as shown (`['23.9', 'GB']`). */
  text: [string, string];
  /** 0-100 for bars and rings, or `null` when the metric has no natural scale. */
  percent: number | null;
  /** Why it is unavailable. */
  reason: string | null;
}

const na = (reason: string): Reading => ({ value: null, text: ['—', ''], percent: null, reason });

/** Reads `metric` from a live system sample. Pure. */
export function readMetric(metric: WidgetMetric, s: SystemSample | null): Reading {
  if (!s) return na('Waiting for the first reading.');
  const pct = (v: number): Reading => ({ value: v, text: splitValue(formatPercent(v)), percent: v, reason: null });
  switch (metric) {
    case 'cpuTotal':
      return pct(s.cpu.total);
    case 'cpuPerCore':
      return pct(s.cpu.perCore.length ? s.cpu.perCore.reduce((a, b) => a + b, 0) / s.cpu.perCore.length : 0);
    case 'memory':
      return pct(s.memory.total > 0 ? (s.memory.used / s.memory.total) * 100 : 0);
    case 'swap':
      return s.memory.swapTotal > 0 ? pct((s.memory.swapUsed / s.memory.swapTotal) * 100) : na('No swap is configured.');
    case 'memoryPressure': {
      if ('unavailable' in s.memory.pressure) return na(s.memory.pressure.unavailable);
      const p = s.memory.pressure.available;
      return { value: p === 'normal' ? 0 : p === 'elevated' ? 60 : 100, text: [p === 'normal' ? 'Normal' : p === 'elevated' ? 'Elevated' : 'Critical', ''], percent: null, reason: null };
    }
    case 'diskActivity': {
      const bps = s.diskIo.readBps + s.diskIo.writeBps;
      return { value: bps, text: splitValue(formatRate(bps)), percent: null, reason: null };
    }
    case 'diskFree': {
      const d = s.disks[0];
      return d && d.total > 0 ? pct((d.free / d.total) * 100) : na('No drive was reported.');
    }
    case 'networkDown':
    case 'networkUp': {
      const bps = s.network.reduce((a, n) => a + (metric === 'networkDown' ? n.rxBps : n.txBps), 0);
      return { value: bps, text: splitValue(formatRate(bps)), percent: null, reason: null };
    }
    case 'gpuLoad':
      return 'available' in s.gpu ? pct(s.gpu.available.utilization) : na(s.gpu.unavailable);
    case 'temperature': {
      if ('unavailable' in s.temperatures) return na(s.temperatures.unavailable);
      const t = s.temperatures.available[0];
      return t ? { value: t.celsius, text: [t.celsius.toFixed(0), '°C'], percent: null, reason: null } : na('No temperature sensor was reported.');
    }
    case 'fan': {
      if ('unavailable' in s.fans) return na(s.fans.unavailable);
      const f = s.fans.available[0];
      return f ? { value: f.rpm, text: [Math.round(f.rpm).toString(), 'rpm'], percent: null, reason: null } : na('No fan was reported.');
    }
    case 'battery':
      return 'available' in s.battery ? pct(s.battery.available.charge) : na(s.battery.unavailable);
    default:
      return na('This metric is not available yet.');
  }
}

/** Reads a performance counter widget from the counters the engine reads for it (see `CountersStore`). Pure. */
export function readCounter(r: CounterReading | undefined): Reading {
  if (!r) return na('Waiting for the first reading.');
  const n = readingNumber(r);
  if (n === null) return na(readingReason(r) ?? 'The counter is not available.');
  return { value: n, text: formatCounter(n, r.unit), percent: r.unit === 'percent' ? Math.min(100, Math.max(0, n)) : null, reason: null };
}

/** The live history of a metric for a graph (`values2` is the second line, e.g. disk write), or `null` when none is kept. */
export function seriesFor(metric: WidgetMetric, s: Series): { values: number[]; values2: number[] | null } | null {
  switch (metric) {
    case 'cpuTotal':
      return { values: s.cpu, values2: null };
    case 'memory':
      return { values: s.memory, values2: null };
    case 'diskActivity':
      return { values: s.diskRead, values2: s.diskWrite };
    case 'networkDown':
      return { values: s.netRx, values2: null };
    case 'networkUp':
      return { values: s.netTx, values2: null };
    case 'gpuLoad':
      return { values: s.gpu, values2: null };
    default:
      return null;
  }
}

export function presetNames(layout: DashboardLayout): NamedPreset[] {
  return layout.presets;
}

// ------------------------------------------------------------------------------------------------- menu bar items

export type TrayStyleId = 'lineGraph' | 'verticalBars' | 'horizontalMeter' | 'ring' | 'graphAndNumber' | 'number';

export const TRAY_STYLES: { id: TrayStyleId; label: string }[] = [
  { id: 'lineGraph', label: 'Line graph' },
  { id: 'verticalBars', label: 'Bars' },
  { id: 'horizontalMeter', label: 'Meter' },
  { id: 'ring', label: 'Ring' },
  { id: 'graphAndNumber', label: 'Graph and number' },
  { id: 'number', label: 'Number' },
];

export function trayStyleId(item: TrayItem): TrayStyleId {
  return typeof item.style === 'string' ? item.style : 'number';
}

/** A tray item's style from its id (a number style carries a unit and a short label). */
export function styleFromId(id: TrayStyleId, unit = '', label = ''): TrayItem['style'] {
  return id === 'number' ? { number: { unit, label } } : id;
}

export function newTrayItem(metric: WidgetMetric, style: TrayStyleId, existing: readonly TrayItem[]): TrayItem {
  let n = existing.length + 1;
  while (existing.some((i) => i.id === `${typeof metric === 'string' ? metric : 'item'}-${n.toString()}`)) n++;
  return { id: `${typeof metric === 'string' ? metric : 'item'}-${n.toString()}`, metric, style: styleFromId(style), opens: 'ownPopover' };
}

export const bytesText = formatBytes;
