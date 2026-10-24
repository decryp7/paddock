import { areaGradient, type PaddockChartTokens } from '@decryp7/paddock-ui';
import { RING_MAX_SLICES, ringOrder } from './ring';

/** The generic chart kinds of the family (plan X21); domain charts build their own option and use `pd-chart` to draw it. */
export type PdChartKind = 'line' | 'area' | 'bar' | 'stackedBar' | 'donut';

/** A series colour: a token (the one hero series is `accent`; bars and tracks are `neutral`) or a slot of the categorical palette. */
export type PdSeriesColor = 'accent' | 'neutral' | 'success' | 'warning' | 'danger' | number;

/** One series: a name (legend, tooltip) and one value per x. A `null` is a gap (never connected, rule 15: no invented data). */
export interface PdChartSeries {
  readonly name: string;
  readonly values: readonly (number | null)[];
  readonly color?: PdSeriesColor;
}

/** What a generic chart shows. */
export interface PdChartSpec {
  readonly kind: PdChartKind;
  /** The x of each value: category names, or epoch milliseconds with `time`. For `donut`, the slice names. */
  readonly x: readonly (string | number)[];
  readonly series: readonly PdChartSeries[];
  /** `x` is a time (epoch ms) on a time axis. */
  readonly time?: boolean;
  /** The unit of the values, shown on the axis and in the tooltip (rule 15: every chart has its unit). */
  readonly unit?: string;
  /** Formats a value for the axis, tooltip and summary (default: the number with `Intl`). */
  readonly format?: (value: number) => string;
  /** Formats an x for the axis and tooltip (default: a time as a local date and time, a category as it is). */
  readonly formatX?: (x: string | number) => string;
  /** Draws a legend (default: when there are 2+ series, or always for a donut). */
  readonly legend?: boolean;
  /** Wheel and pinch zoom on the x axis. */
  readonly zoom?: boolean;
  /** A fixed top of the value axis (the same scale across charts that are compared). */
  readonly max?: number;
}

/** `&`, `<`, `>`, `"` and `'` escaped: series names and categories come from data, and ECharts tooltips are HTML. */
export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);
}

/** A tooltip body: a title and one row per series (marker colour, name, value), all escaped. */
export function tooltipHtml(title: string, rows: readonly { color: string; name: string; value: string }[]): string {
  const lines = rows.map(
    (r) =>
      `<div style="display:flex;align-items:center;gap:8px"><span style="width:8px;height:8px;border-radius:2px;background:${escapeHtml(r.color)}"></span><span style="flex:1">${escapeHtml(r.name)}</span><b>${escapeHtml(r.value)}</b></div>`,
  );
  return `<div style="font-weight:500;margin-bottom:4px">${escapeHtml(title)}</div>${lines.join('')}`;
}

/** The colour of series `index`: its own choice, else the accent for a lone series, else the next slot of the categorical palette. */
export function seriesColor(t: PaddockChartTokens, color: PdSeriesColor | undefined, index: number, only = false): string {
  const slot = (n: number): string => t.palette[((n % t.palette.length) + t.palette.length) % t.palette.length] ?? t.accent;
  switch (color) {
    case undefined:
      // One series is the chart's highlighted series: the accent, the same blue in every chart of the page.
      return only ? t.accent : slot(index);
    case 'accent':
      return t.accent;
    case 'neutral':
      return t.neutralBar;
    case 'success':
      return t.success;
    case 'warning':
      return t.warning;
    case 'danger':
      return t.danger;
    default:
      return slot(color);
  }
}

const defaultFormat = (v: number): string => new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 }).format(v);
const defaultFormatX = (x: string | number, time: boolean | undefined): string => (time && typeof x === 'number' ? new Date(x).toLocaleString() : String(x));

function withUnit(text: string, unit: string | undefined): string {
  return unit ? `${text} ${unit}` : text;
}

/** One slice of a ring. */
interface RingPart {
  readonly name: string;
  readonly value: number;
  readonly other: boolean;
}

/** The slices of a donut: the first {@link RING_MAX_SLICES} as given (a missing value is 0), the rest merged into one "Other". */
function ringParts(spec: PdChartSpec): RingPart[] {
  const values = spec.series[0]?.values ?? [];
  const all = spec.x.map((name, i) => ({ name: String(name), value: values[i] ?? 0, other: false }));
  if (all.length <= RING_MAX_SLICES) return all;
  const rest = all.slice(RING_MAX_SLICES - 1);
  return [...all.slice(0, RING_MAX_SLICES - 1), { name: $localize`:@@pd.chart.other:Other`, value: rest.reduce((a, p) => a + p.value, 0), other: true }];
}

/**
 * A text twin of the chart for people who cannot see it (the chart's accessible description): per series the range and the
 * latest value, per donut the shares. Short, in the data's own units.
 */
export function chartSummary(spec: PdChartSpec): string {
  const fmt = (v: number): string => withUnit((spec.format ?? defaultFormat)(v), spec.unit);
  if (spec.kind === 'donut') {
    const parts = ringParts(spec);
    const pct = shares(parts.map((p) => p.value));
    const total = parts.reduce((a, p) => a + p.value, 0);
    return parts.map((p, i) => `${p.name}: ${fmt(p.value)}${total ? ` (${pct[i]}%)` : ''}`).join('; ');
  }
  return spec.series
    .map((s) => {
      const present = s.values.filter((v): v is number => v !== null);
      if (!present.length) return `${s.name}: no data`;
      const last = present[present.length - 1] ?? 0;
      return `${s.name}: ${fmt(Math.min(...present))} to ${fmt(Math.max(...present))}, latest ${fmt(last)}`;
    })
    .join('; ');
}

/** How the option is drawn: `animate` false for reduced motion; `narrow` (a box under 400 px) puts a donut's list under the ring; `height` is the canvas height. */
export interface PdChartDrawing {
  readonly animate: boolean;
  readonly narrow?: boolean;
  readonly width?: number;
  readonly height?: number;
}

/** The ring's radius in px (the same in the wide and the narrow layout's maths). */
const RING_RADIUS_NARROW = 58;

/** The canvas height a donut with `rows` slices needs when it is narrow (ring on top, list under it). */
export function donutNarrowHeight(rows: number): number {
  return 12 + RING_RADIUS_NARROW * 2 + 14 + rows * 20 + 8;
}

/**
 * Whole-number shares that add up to exactly 100 (largest-remainder rounding): 48.3, 30.9, 16.1 and 4.7 read 48, 31, 16, 5, never 101.
 * All zeros when the total is 0.
 */
export function shares(values: readonly number[]): number[] {
  const total = values.reduce((a, v) => a + v, 0);
  if (total <= 0) return values.map(() => 0);
  const exact = values.map((v) => (v / total) * 100);
  const floor = exact.map(Math.floor);
  let left = 100 - floor.reduce((a, v) => a + v, 0);
  const order = exact.map((x, i) => ({ i, r: x - Math.floor(x) })).sort((a, b) => b.r - a.r || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    floor[i] = (floor[i] ?? 0) + 1;
    left -= 1;
  }
  return floor;
}

/**
 * The ECharts option for a generic chart. Axes, grid, legend, tooltip frame and series defaults come from the paddock
 * theme (`registerPaddockTheme`); this adds the data, the axis kind, the unit and the family's chart rules: bars and
 * areas start at zero, a gap is a gap, a legend when there are two or more series, stacked bars share a stack, series
 * colours come from the validated categorical palette in a fixed order, and tooltip text is escaped.
 */
export function chartOption(spec: PdChartSpec, t: PaddockChartTokens, drawing: PdChartDrawing = { animate: true }): Record<string, unknown> {
  const format = spec.format ?? defaultFormat;
  const formatX = spec.formatX ?? ((x: string | number) => defaultFormatX(x, spec.time));
  const legend = spec.legend ?? (spec.kind === 'donut' || spec.series.length > 1);
  const animation = { animation: drawing.animate, ...(drawing.animate ? {} : { animationDuration: 0, animationDurationUpdate: 0 }) };
  const legendOption = { legend: { show: legend, left: 12, data: spec.kind === 'donut' ? spec.x.map(String) : spec.series.map((s) => s.name) } };

  if (spec.kind === 'donut') {
    const parts = ringParts(spec);
    const total = parts.reduce((a, p) => a + p.value, 0);
    const pct = shares(parts.map((p) => p.value));
    const order = ringOrder(t.palette, Math.min(parts.length, RING_MAX_SLICES));
    const colorOf = (i: number): string => (i < RING_MAX_SLICES ? t.palette[order[i] ?? i] ?? t.accent : t.neutralBar);
    const data = parts.map((p, i) => ({ name: p.name, value: p.value, itemStyle: { color: p.other ? t.neutralBar : colorOf(i) } }));
    // One source for the ring and the total in its middle: the centre in pixels. Narrow boxes put the list under the ring.
    const narrow = drawing.narrow === true;
    const height = drawing.height ?? 200;
    const radius = narrow ? RING_RADIUS_NARROW : Math.max(40, Math.min(Math.floor(height / 2) - 10, 90));
    const cx = narrow ? Math.floor((drawing.width ?? 320) / 2) : radius + 16;
    const cy = narrow ? radius + 12 : Math.floor(height / 2);
    return {
      ...animation,
      // Direct labels: every slice is named in the list beside (or under) the ring with its value and share, so colour is never the only key.
      legend: {
        show: true,
        orient: 'vertical',
        icon: 'roundRect',
        itemGap: 8,
        ...(narrow ? { left: 'center', bottom: 4 } : { left: cx + radius + 24, top: 'middle' }),
        formatter: (name: string) => {
          const i = parts.findIndex((x) => x.name === name);
          return i < 0 ? name : `${name}  ${withUnit(format(parts[i]?.value ?? 0), spec.unit)}  ${pct[i]}%`;
        },
        data: parts.map((p) => p.name),
      },
      graphic: [
        {
          type: 'text',
          x: cx,
          y: cy,
          style: { text: `${format(total)}${spec.unit ? `\n${spec.unit}` : ''}`, textAlign: 'center', textVerticalAlign: 'middle', fill: t.text, font: `600 18px ${t.fontFamily}`, lineHeight: 22 },
        },
      ],
      tooltip: {
        trigger: 'item',
        formatter: (p: { name: string; value: number; color: string }) => {
          const i = parts.findIndex((x) => x.name === p.name);
          return tooltipHtml(p.name, [{ color: p.color, name: withUnit(format(p.value), spec.unit), value: `${pct[i] ?? 0}%` }]);
        },
      },
      series: [{ type: 'pie', radius: [Math.round(radius * 0.68), radius], center: [cx, cy], label: { show: false }, itemStyle: { borderColor: t.surface1, borderWidth: 2 }, data }],
    };
  }

  const bars = spec.kind === 'bar' || spec.kind === 'stackedBar';
  const points = (values: readonly (number | null)[]): unknown[] => (spec.time ? values.map((v, i) => [spec.x[i], v]) : [...values]);
  const only = spec.series.length === 1;
  /** Per category, the series on top of the stack: the only segment with rounded top corners (the guide: 4 px on the bar, not on every segment). */
  const topOfStack = spec.x.map((_, i) => {
    for (let s = spec.series.length - 1; s >= 0; s -= 1) {
      const v = spec.series[s]?.values[i];
      if (typeof v === 'number' && v > 0) return s;
    }
    return -1;
  });
  const series = spec.series.map((s, i) => {
    const color = seriesColor(t, s.color, i, only);
    const common = { name: s.name, color, itemStyle: { color } };
    switch (spec.kind) {
      case 'line':
      case 'area': {
        // A value with no neighbour (a gap on both sides) would draw nothing: it gets a marker so it is not lost.
        const lone = s.values.map((v, k) => v !== null && s.values[k - 1] == null && s.values[k + 1] == null);
        const line = { ...common, data: points(s.values), type: 'line', showSymbol: true, symbolSize: (_v: unknown, p: { dataIndex: number }) => (lone[p.dataIndex] ? 6 : 0), lineStyle: { width: 1.5, color } };
        return spec.kind === 'area' ? { ...line, areaStyle: { color: areaGradient(color) } } : line;
      }
      case 'stackedBar': {
        const data = s.values.map((v, k) => ({ value: spec.time ? [spec.x[k], v] : v, itemStyle: { color, borderRadius: topOfStack[k] === i ? [4, 4, 0, 0] : 0 } }));
        return { ...common, type: 'bar', stack: 'total', data };
      }
      default:
        return { ...common, data: points(s.values), type: 'bar' };
    }
  });
  return {
    ...animation,
    ...legendOption,
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: bars ? 'shadow' : 'line' },
      formatter: (params: { axisValue: string | number; seriesName: string; value: unknown; color: string }[]) => {
        const first = params[0];
        return tooltipHtml(
          first ? formatX(first.axisValue) : '',
          params.map((p) => {
            const v = Array.isArray(p.value) ? p.value[1] : p.value;
            return { color: p.color, name: p.seriesName, value: typeof v === 'number' ? withUnit(format(v), spec.unit) : '–' };
          }),
        );
      },
    },
    grid: { top: legend ? 36 : 12, left: 12, right: 12, bottom: spec.zoom ? 28 : 8, containLabel: true },
    xAxis: spec.time ? { type: 'time', axisLabel: { hideOverlap: true } } : { type: 'category', data: spec.x.map(String), boundaryGap: bars, axisLabel: { hideOverlap: true, formatter: (v: string) => formatX(v) } },
    yAxis: { type: 'value', min: bars || spec.kind === 'area' ? 0 : undefined, max: spec.max, axisLabel: { formatter: (v: number) => format(v) } },
    series,
    ...(spec.zoom ? { dataZoom: [{ type: 'inside' }] } : {}),
  };
}
