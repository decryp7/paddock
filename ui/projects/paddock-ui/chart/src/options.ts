import { areaGradient, type PaddockChartTokens } from '@decryp7/paddock-ui';

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

/** The colour of series `index`: its own choice, else the next slot of the categorical palette. */
export function seriesColor(t: PaddockChartTokens, color: PdSeriesColor | undefined, index: number): string {
  const slot = (n: number): string => t.palette[((n % t.palette.length) + t.palette.length) % t.palette.length] ?? t.accent;
  switch (color) {
    case undefined:
      return slot(index);
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

/**
 * A text twin of the chart for people who cannot see it (the chart's accessible description): per series the range and the
 * latest value, per donut the shares. Short, in the data's own units.
 */
export function chartSummary(spec: PdChartSpec): string {
  const fmt = (v: number): string => withUnit((spec.format ?? defaultFormat)(v), spec.unit);
  if (spec.kind === 'donut') {
    const s = spec.series[0];
    const total = (s?.values ?? []).reduce<number>((a, v) => a + (v ?? 0), 0);
    return spec.x.map((name, i) => `${name}: ${fmt(s?.values[i] ?? 0)}${total ? ` (${Math.round(((s?.values[i] ?? 0) / total) * 100)}%)` : ''}`).join('; ');
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

/** How the option is drawn: `animate` false for reduced motion. */
export interface PdChartDrawing {
  readonly animate: boolean;
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
  const legendOption = { legend: { show: legend, data: spec.kind === 'donut' ? spec.x.map(String) : spec.series.map((s) => s.name) } };

  if (spec.kind === 'donut') {
    const s = spec.series[0];
    const data = spec.x.map((name, i) => ({ name: String(name), value: s?.values[i] ?? 0, itemStyle: { color: seriesColor(t, undefined, i) } }));
    return {
      ...animation,
      ...legendOption,
      tooltip: {
        trigger: 'item',
        formatter: (p: { name: string; value: number; color: string; percent: number }) =>
          tooltipHtml(p.name, [{ color: p.color, name: withUnit(format(p.value), spec.unit), value: `${Math.round(p.percent)}%` }]),
      },
      series: [{ type: 'pie', radius: ['58%', '82%'], center: ['50%', '54%'], label: { show: false }, itemStyle: { borderColor: t.surface1, borderWidth: 2 }, data }],
    };
  }

  const bars = spec.kind === 'bar' || spec.kind === 'stackedBar';
  const points = (values: readonly (number | null)[]): unknown[] => (spec.time ? values.map((v, i) => [spec.x[i], v]) : [...values]);
  const series = spec.series.map((s, i) => {
    const color = seriesColor(t, s.color, i);
    const common = { name: s.name, data: points(s.values), color, itemStyle: { color } };
    switch (spec.kind) {
      case 'line':
        return { ...common, type: 'line', lineStyle: { width: 1.5, color } };
      case 'area':
        return { ...common, type: 'line', lineStyle: { width: 1.5, color }, areaStyle: { color: areaGradient(color) } };
      case 'stackedBar':
        return { ...common, type: 'bar', stack: 'total' };
      default:
        return { ...common, type: 'bar' };
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
    grid: { top: legend ? 32 : 12, left: 8, right: 12, bottom: spec.zoom ? 28 : 8, containLabel: true },
    xAxis: spec.time ? { type: 'time', axisLabel: { hideOverlap: true } } : { type: 'category', data: spec.x.map(String), boundaryGap: bars, axisLabel: { hideOverlap: true, formatter: (v: string) => formatX(v) } },
    yAxis: { type: 'value', min: bars || spec.kind === 'area' ? 0 : undefined, max: spec.max, axisLabel: { formatter: (v: number) => format(v) }, name: spec.unit ?? '', nameGap: 8 },
    series,
    ...(spec.zoom ? { dataZoom: [{ type: 'inside' }] } : {}),
  };
}
