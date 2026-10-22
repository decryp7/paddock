import type { EChartsCoreOption } from 'echarts/core';
import type { HistoryPoint, HistoryRange, KeyedSeries, PitwallEvent, ReleaseReason, UsageSeries } from '../../core/api.types';
import { bytes, clock, compact, date, rate } from '../format';
import { REASON_LABEL } from '../labels';
import { seriesColor, type ThemeTokens } from '../theme-tokens';

/**
 * ECharts options for the dashboard's history charts, as pure functions of the data and the theme tokens. Series
 * colours come from the validated categorical palette in a fixed order; text uses the text tokens, never a series
 * colour; one y-axis per plot (two measures of different scale get two plots).
 */

const GB = 1024 ** 3;
const FONT = "Inter, system-ui, -apple-system, 'Segoe UI', sans-serif";

/** The x-axis tick text for a range. */
export function timeLabel(range: HistoryRange): (value: number) => string {
  return range === '1h' || range === '24h' ? (v) => clock(v) : (v) => date(v);
}

function base(t: ThemeTokens): EChartsCoreOption {
  return {
    animation: true,
    animationDuration: 300,
    animationDurationUpdate: 250,
    textStyle: { fontFamily: FONT, color: t.text2, fontSize: 11 },
    tooltip: {
      trigger: 'axis',
      backgroundColor: t.tooltipBg,
      borderWidth: 0,
      padding: [8, 10],
      textStyle: { color: t.tooltipText, fontSize: 12, fontFamily: FONT },
      axisPointer: { type: 'line', lineStyle: { color: t.axis, width: 1 } },
      extraCssText: 'border-radius:8px;box-shadow:0 8px 24px rgba(0,0,0,.25);',
    },
  };
}

function timeAxis(t: ThemeTokens, range: HistoryRange, gridIndex = 0, show = true): Record<string, unknown> {
  return {
    type: 'time',
    gridIndex,
    boundaryGap: false,
    axisLine: { show, lineStyle: { color: t.axis } },
    axisTick: { show: false },
    splitLine: { show: false },
    axisLabel: { show, color: t.text3, fontSize: 11, hideOverlap: true, formatter: timeLabel(range) },
    axisPointer: { label: { show: false } },
  };
}

function valueAxis(t: ThemeTokens, gridIndex: number, formatter: (v: number) => string, extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    type: 'value',
    gridIndex,
    splitNumber: 3,
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: { lineStyle: { color: t.grid } },
    axisLabel: { color: t.text3, fontSize: 11, formatter },
    ...extra,
  };
}

function line(name: string, color: string, data: [number, number | null][], extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    name,
    type: 'line',
    data,
    showSymbol: false,
    symbol: 'circle',
    symbolSize: 8,
    smooth: 0.25,
    sampling: 'lttb',
    connectNulls: false,
    lineStyle: { width: 2, color },
    itemStyle: { color, borderWidth: 2, borderColor: 'transparent' },
    emphasis: { focus: 'none', lineStyle: { width: 2 } },
    ...extra,
  };
}

function area(color: string): Record<string, unknown> {
  return {
    areaStyle: {
      color: {
        type: 'linear',
        x: 0,
        y: 0,
        x2: 0,
        y2: 1,
        colorStops: [
          { offset: 0, color: `${color}38` },
          { offset: 1, color: `${color}00` },
        ],
      },
    },
  };
}

/** How an event is drawn on the memory timeline's event lane: a shape and a colour, never colour alone. */
export interface EventMark {
  glyph: string;
  /** The ECharts symbol of the lane marker; the glyph is its text twin for tooltips and the legend. */
  shape: 'triangle' | 'circle' | 'diamond' | 'rect';
  color: string;
  label: string;
}

/**
 * The lane's vocabulary: a triangle for a load, a circle for a release, a diamond for a release forced by memory pressure
 * or a failure, a square for an engine restart. Only the forced ones use a status colour; the rest are neutral.
 */
export function eventMark(e: PitwallEvent, t: ThemeTokens): EventMark | null {
  if (e.kind === 'modelLoaded') return { glyph: '▲', shape: 'triangle', color: t.text2, label: 'Model loaded' };
  if (e.kind === 'modelUnloaded' || e.kind === 'contextReleased') {
    const reason: ReleaseReason = e.reason ?? 'manual';
    const forced = reason === 'pressure' || reason === 'failed';
    return forced ? { glyph: '◆', shape: 'diamond', color: t.bad, label: `Released (${REASON_LABEL[reason]})` } : { glyph: '●', shape: 'circle', color: t.text2, label: `Released (${REASON_LABEL[reason]})` };
  }
  if (e.kind === 'engineRestarted' || e.kind === 'engineFailed') return { glyph: '■', shape: 'rect', color: t.bad, label: 'Engine restarted' };
  return null;
}

function eventsInStep(events: PitwallEvent[], at: number, stepMs: number): PitwallEvent[] {
  return events.filter((e) => e.at > at - stepMs && e.at <= at);
}

function tooltipRows(params: unknown, unit: (v: number) => string): string {
  const list = (Array.isArray(params) ? params : [params]) as { marker: string; seriesName: string; value: [number, number | null] }[];
  return list
    .filter((p) => p.value && p.value[1] !== null && p.value[1] !== undefined)
    .map((p) => `<div style="display:flex;gap:12px;justify-content:space-between"><span>${p.marker}${p.seriesName}</span><b>${unit(p.value[1]!)}</b></div>`)
    .join('');
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

export interface MemoryChartInput {
  points: HistoryPoint[];
  events: PitwallEvent[];
  range: HistoryRange;
  stepMs: number;
  /** Show the zoom slider (the large chart only). */
  zoom?: boolean;
}

const gbText = (v: number) => `${(v / GB).toFixed(1)} GB`;

/** Height of the event lane under the plot (px), and the room the time axis labels need between them. */
const LANE = 16;
const AXIS_LABELS = 22;

/**
 * RAM, VRAM and Pitwall's own memory over time (the guide's sky blue, orange and bluish green; Pitwall's line dashed),
 * with loads, releases and restarts on their own lane under the plot instead of lines drawn through the data.
 */
export function memoryChart(input: MemoryChartInput, t: ThemeTokens): EChartsCoreOption {
  const { points, events, range, stepMs } = input;
  const [cRam, cVram, cOwn] = t.history;
  const ram = points.map((p) => [p.at, p.ramUsed] as [number, number]);
  const vram = points.map((p) => [p.at, p.vramUsed] as [number, number | null]);
  const own = points.map((p) => [p.at, p.pitwallRss] as [number, number]);
  const total = points[points.length - 1]?.ramTotal ?? null;
  const from = points[0]?.at ?? 0;
  const to = points[points.length - 1]?.at ?? 0;
  const marks = events
    .filter((e) => e.at >= from)
    .map((e) => ({ e, m: eventMark(e, t) }))
    .filter((x): x is { e: PitwallEvent; m: EventMark } => x.m !== null)
    .map(({ e, m }) => ({ name: `${m.label}: ${e.message}`, value: [e.at, 0.5], symbol: m.shape, itemStyle: { color: m.color } }));
  const tf = timeLabel(range);
  const sliderRoom = input.zoom ? 34 : 0;
  const laneBottom = 6 + sliderRoom;
  const x = (gridIndex: number, show: boolean) => ({ ...timeAxis(t, range, gridIndex, show), min: from || undefined, max: to || undefined });
  return {
    ...base(t),
    color: [cRam, cVram, cOwn],
    grid: [
      { left: 8, right: 16, top: 22, bottom: laneBottom + LANE + AXIS_LABELS },
      { left: 8, right: 16, height: LANE, bottom: laneBottom },
    ],
    axisPointer: { link: [{ xAxisIndex: 'all' }] },
    legend: { show: false },
    tooltip: {
      ...(base(t)['tooltip'] as object),
      formatter: (params: unknown) => {
        const first = (Array.isArray(params) ? params[0] : params) as { value: [number, number] } | undefined;
        const at = first?.value[0] ?? 0;
        const evs = eventsInStep(events, at, stepMs)
          .map((e) => `<div style="margin-top:6px;max-width:280px;white-space:normal;opacity:.85">${escapeHtml(e.message)}</div>`)
          .join('');
        return `<div style="opacity:.7;margin-bottom:4px">${range === '1h' || range === '24h' ? clock(at) : date(at, true)}</div>${tooltipRows(params, gbText)}${evs}`;
      },
    },
    xAxis: [x(0, true), { ...x(1, false), axisLine: { show: false } }],
    yAxis: [
      valueAxis(t, 0, (v) => `${Math.round(v / GB)} GB`, { max: total ?? undefined, interval: total ? total / 4 : undefined }),
      { type: 'value', gridIndex: 1, min: 0, max: 1, show: false },
    ],
    dataZoom: [{ type: 'inside', xAxisIndex: [0, 1], filterMode: 'none' }, ...(input.zoom ? [zoomSlider(t, tf)] : [])],
    series: [
      line('RAM used', cRam, ram, { ...area(cRam) }),
      line('VRAM used', cVram, vram),
      line('Pitwall', cOwn, own, { lineStyle: { width: 2, color: cOwn, type: [4, 3] } }),
      {
        name: 'Events',
        type: 'scatter',
        xAxisIndex: 1,
        yAxisIndex: 1,
        data: marks,
        symbolSize: 9,
        animation: false,
        tooltip: { trigger: 'item', formatter: (p: { name: string }) => escapeHtml(p.name) },
        emphasis: { scale: 1.4 },
      },
    ],
  };
}

function zoomSlider(t: ThemeTokens, tf: (v: number) => string): Record<string, unknown> {
  return {
    type: 'slider',
    height: 22,
    bottom: 6,
    filterMode: 'none',
    borderColor: t.grid,
    backgroundColor: 'transparent',
    fillerColor: `${t.accent}1f`,
    dataBackground: { lineStyle: { color: t.axis }, areaStyle: { color: t.grid } },
    selectedDataBackground: { lineStyle: { color: t.accent }, areaStyle: { color: `${t.accent}22` } },
    handleStyle: { color: t.surface, borderColor: t.axis },
    moveHandleStyle: { color: t.axis },
    textStyle: { color: t.text3, fontSize: 10 },
    labelFormatter: (v: number) => tf(v),
  };
}

/** Generation and prompt tokens per second as two stacked plots sharing the time axis (different scales). */
export function throughputChart(points: HistoryPoint[], range: HistoryRange, t: ThemeTokens): EChartsCoreOption {
  const gen = points.map((p) => [p.at, p.genTps || null] as [number, number | null]);
  const prompt = points.map((p) => [p.at, p.promptTps || null] as [number, number | null]);
  return {
    ...base(t),
    tooltip: { ...(base(t)['tooltip'] as object), formatter: (p: unknown) => tooltipRows(p, (v) => `${rate(v)} tok/s`) },
    axisPointer: { link: [{ xAxisIndex: 'all' }] },
    grid: [
      { left: 8, right: 12, top: 30, height: '30%' },
      { left: 8, right: 12, top: '66%', bottom: 4 },
    ],
    title: [
      { text: 'Generation', left: 4, top: 0, textStyle: { fontSize: 11, fontWeight: 500, color: t.text2 } },
      { text: 'Prompt processing', left: 4, top: '52%', textStyle: { fontSize: 11, fontWeight: 500, color: t.text2 } },
    ],
    xAxis: [timeAxis(t, range, 0, false), timeAxis(t, range, 1)],
    yAxis: [valueAxis(t, 0, (v) => `${Math.round(v)}`, { splitNumber: 2 }), valueAxis(t, 1, (v) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}`), { splitNumber: 2 })],
    dataZoom: [{ type: 'inside', xAxisIndex: [0, 1], filterMode: 'none' }],
    series: [line('Generation', seriesColor(t, 0), gen, { ...area(seriesColor(t, 0)) }), line('Prompt', seriesColor(t, 1), prompt, { xAxisIndex: 1, yAxisIndex: 1 })],
  };
}

/** Time to first token p50 and p95. */
export function latencyChart(points: HistoryPoint[], range: HistoryRange, t: ThemeTokens): EChartsCoreOption {
  const ms = (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)} s` : `${Math.round(v)} ms`);
  return {
    ...base(t),
    tooltip: { ...(base(t)['tooltip'] as object), formatter: (p: unknown) => tooltipRows(p, ms) },
    legend: legend(t, ['p50', 'p95']),
    grid: { left: 8, right: 12, top: 30, bottom: 4 },
    xAxis: timeAxis(t, range),
    yAxis: valueAxis(t, 0, ms),
    dataZoom: [{ type: 'inside', filterMode: 'none' }],
    series: [line('p50', seriesColor(t, 0), points.map((p) => [p.at, p.p50Ms])), line('p95', seriesColor(t, 1), points.map((p) => [p.at, p.p95Ms]))],
  };
}

function legend(t: ThemeTokens, names: string[]): Record<string, unknown> {
  return { show: true, data: names, top: 0, right: 4, icon: 'roundRect', itemWidth: 10, itemHeight: 10, itemGap: 14, textStyle: { color: t.text2, fontSize: 11 } };
}

/** At most this many keyed series get their own colour; the rest fold into "Other". */
export const MAX_KEYED = 5;
const OTHER = 'Other';

/** Folds keyed series beyond {@link MAX_KEYED} (smallest totals) into "Other", keeping the given order otherwise. */
export function foldKeyed(series: KeyedSeries[], max = MAX_KEYED): KeyedSeries[] {
  if (series.length <= max) return series;
  const total = (s: KeyedSeries) => s.values.reduce((a, b) => a + b, 0);
  const ranked = [...series].sort((a, b) => total(b) - total(a));
  const keep = new Set(ranked.slice(0, max - 1).map((s) => s.key));
  const kept = series.filter((s) => keep.has(s.key));
  const rest = series.filter((s) => !keep.has(s.key));
  const len = Math.max(0, ...rest.map((s) => s.values.length));
  const other = { key: OTHER, values: Array.from({ length: len }, (_, i) => rest.reduce((sum, s) => sum + (s.values[i] ?? 0), 0)) };
  return [...kept, other];
}

/** At most this many bars per chart: more steps are summed into buckets (thin bars read as noise). */
export const MAX_BARS = 48;

/** Sums consecutive steps into at most `max` buckets, each stamped with its last step's time. */
export function bucketKeyed(at: number[], series: KeyedSeries[], max = MAX_BARS): { at: number[]; series: KeyedSeries[] } {
  if (at.length <= max) return { at, series };
  const size = Math.ceil(at.length / max);
  const buckets = Math.ceil(at.length / size);
  const times = Array.from({ length: buckets }, (_, b) => at[Math.min(at.length - 1, (b + 1) * size - 1)]!);
  const sum = (values: number[]) => Array.from({ length: buckets }, (_, b) => values.slice(b * size, (b + 1) * size).reduce((x, y) => x + y, 0));
  return { at: times, series: series.map((s) => ({ key: s.key, values: sum(s.values) })) };
}

/** Requests per step as stacked bars, one colour per key in a fixed order. */
export function stackedRequestsChart(points: HistoryPoint[], series: KeyedSeries[], range: HistoryRange, t: ThemeTokens, nameOf: (key: string) => string = (k) => k): EChartsCoreOption {
  const { at, series: bucketed } = bucketKeyed(points.map((p) => p.at), foldKeyed(series));
  const folded = bucketed.map((s) => ({ ...s, key: s.key === OTHER ? OTHER : nameOf(s.key) }));
  return {
    ...base(t),
    tooltip: {
      ...(base(t)['tooltip'] as object),
      axisPointer: { type: 'shadow', shadowStyle: { color: `${t.text3}14` } },
      formatter: (p: unknown) => tooltipRows(p, (v) => `${Math.round(v)}`),
    },
    legend: { ...legend(t, folded.map((s) => s.key)), top: 'auto', bottom: 0, left: 0, right: 0 },
    grid: { left: 8, right: 12, top: 10, bottom: folded.length > 3 ? 52 : 32 },
    xAxis: timeAxis(t, range),
    yAxis: valueAxis(t, 0, (v) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}`)),
    dataZoom: [{ type: 'inside', filterMode: 'none' }],
    series: folded.map((s, i) => ({
      name: s.key,
      type: 'bar',
      stack: 'requests',
      barMaxWidth: 14,
      barCategoryGap: '25%',
      data: s.values.map((v, k) => [at[k] ?? 0, v]),
      itemStyle: { color: seriesColor(t, i), borderColor: t.surface, borderWidth: 0.5, borderRadius: i === folded.length - 1 ? [2, 2, 0, 0] : 0 },
      emphasis: { focus: 'series' },
    })),
  };
}

/** What a usage chart plots. */
export type UsageMetric = 'tokens' | 'requests' | 'memory';

/** How the stacks of a usage chart divide: by app (the dashboard) or by kind (one app: prompt/completion, ok/errors). */
export type UsageSplit = 'app' | 'kind';

/**
 * Usage over time from the server's complete buckets. Tokens and requests are stacked bars (by app, or by kind for one
 * app: prompt and completion tokens, successful and failed requests); peak memory is one line per app (the largest
 * model footprint attributed in each bucket, so it is a level, never a sum). Failed requests use the status colour and
 * are named "Errors" in the legend and tooltip, so colour is never the only cue.
 */
export function usageChart(series: UsageSeries, metric: UsageMetric, split: UsageSplit, t: ThemeTokens, nameOf: (appId: string) => string = (k) => k): EChartsCoreOption {
  const at = series.starts;
  const pts = (values: number[]) => values.map((v, i) => [at[i] ?? 0, v] as [number, number]);
  const unit = metric === 'memory' ? (v: number) => bytes(v) : (v: number) => (metric === 'tokens' ? `${compact(v)} tokens` : `${Math.round(v)} requests`);
  type Entry = { name: string; color: string; values: number[] };
  let entries: Entry[];
  if (split === 'kind') {
    const tot = series.total;
    entries =
      metric === 'tokens'
        ? [
            { name: 'Prompt', color: seriesColor(t, 0), values: tot.map((c) => c.promptTokens) },
            { name: 'Completion', color: seriesColor(t, 1), values: tot.map((c) => c.completionTokens) },
          ]
        : metric === 'requests'
          ? [
              { name: 'Successful', color: seriesColor(t, 0), values: tot.map((c) => c.requests - c.errors) },
              { name: 'Errors', color: t.bad, values: tot.map((c) => c.errors) },
            ]
          : [{ name: 'Peak memory', color: seriesColor(t, 0), values: tot.map((c) => c.peakMemoryBytes) }];
  } else {
    const pick = (c: UsageSeries['total'][number]) => (metric === 'tokens' ? c.promptTokens + c.completionTokens : metric === 'requests' ? c.requests : c.peakMemoryBytes);
    entries = series.apps.map((a, i) => ({ name: a.appId === 'other' ? 'Other' : nameOf(a.appId), color: seriesColor(t, i), values: a.points.map(pick) }));
  }
  const bars = metric !== 'memory';
  const wide = Math.max(1, series.stepMs / 3_600_000);
  return {
    ...base(t),
    tooltip: {
      ...(base(t)['tooltip'] as object),
      axisPointer: { type: bars ? 'shadow' : 'line', shadowStyle: { color: `${t.text3}14` }, lineStyle: { color: t.axis, width: 1 } },
      formatter: (p: unknown) => tooltipRows(p, unit),
    },
    legend: { ...legend(t, entries.map((e) => e.name)), top: 'auto', bottom: 0, left: 0, right: 0 },
    grid: { left: 8, right: 12, top: 10, bottom: entries.length > 3 ? 52 : 32 },
    xAxis: { ...timeAxis(t, series.range), boundaryGap: false },
    yAxis: valueAxis(t, 0, (v) => (metric === 'memory' ? bytes(v, 0) : compact(v)), { min: 0 }),
    series: entries.map((e, i) =>
      bars
        ? {
            name: e.name,
            type: 'bar',
            stack: metric,
            barMaxWidth: wide > 1 ? 14 : 10,
            barCategoryGap: '25%',
            data: pts(e.values),
            itemStyle: { color: e.color, borderColor: t.surface, borderWidth: 0.5, borderRadius: i === entries.length - 1 ? [2, 2, 0, 0] : 0 },
            emphasis: { focus: 'series' },
          }
        : line(e.name, e.color, pts(e.values), { step: 'end', smooth: false, ...(entries.length === 1 ? area(e.color) : {}) }),
    ),
  };
}
