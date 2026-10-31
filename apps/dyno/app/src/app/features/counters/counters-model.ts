import type { EChartsCoreOption } from 'echarts/core';
import type { DashboardWidget } from '../../core/generated/widgets';
import type { CounterHistory, CounterReading, CounterUnit } from '../../core/api.types';
import { formatBytes, formatClock, formatPercent, formatRate } from '../../shared/format';
import type { ThemeTokens } from '../../shared/tokens';
import { EVENT_STYLE } from '../dashboard/history-options';

/** The limits the engine enforces (`dyno_core::counters`); the UI keeps to them so a save is never rejected for them. */
export const MAX_WATCHED = 32;
export const MAX_PINNED = 16;
export const MAX_SETS = 12;
export const MAX_SET_NAME = 48;

/** A counter path split for display only (`\Network Interface(Wi-Fi)\Bytes Sent/sec`); the engine does the real parsing. */
export interface PathParts {
  object: string;
  instance: string | null;
  counter: string;
}

export function pathParts(path: string): PathParts {
  const body = path.startsWith('\\') ? path.slice(1) : path;
  const cut = body.lastIndexOf('\\');
  const head = cut >= 0 ? body.slice(0, cut) : body;
  const counter = cut >= 0 ? body.slice(cut + 1) : '';
  const open = head.indexOf('(');
  if (open < 0 || !head.endsWith(')')) return { object: head, instance: null, counter };
  return { object: head.slice(0, open), instance: head.slice(open + 1, -1), counter };
}

/** `Bytes Sent/sec (Wi-Fi)`: the name a row or a legend shows. */
export function counterTitle(path: string): string {
  const p = pathParts(path);
  return p.instance === null ? p.counter : `${p.counter} (${p.instance})`;
}

export const UNIT_WORD: Record<CounterUnit, string> = {
  percent: 'Percent',
  bytes: 'Bytes',
  bytesPerSec: 'Bytes per second',
  perSec: 'Per second',
  count: 'Count',
  milliseconds: 'Milliseconds',
  seconds: 'Seconds',
  number: 'Number',
};

function plain(v: number): string {
  const a = Math.abs(v);
  if (a >= 100 || Number.isInteger(v)) return Math.round(v).toLocaleString();
  return a >= 10 ? v.toFixed(1) : v.toFixed(2);
}

/** A value and its unit, split so the number can be right-aligned and the unit sit in the column header or beside it. */
export function formatCounter(value: number, unit: CounterUnit): [string, string] {
  switch (unit) {
    case 'percent':
      return [formatPercent(value).replace('%', ''), '%'];
    case 'bytes': {
      const [n, u] = formatBytes(value).split(' ');
      return [n ?? '—', u ?? ''];
    }
    case 'bytesPerSec': {
      const [n, u] = formatRate(value).split(' ');
      return [n ?? '—', u ?? ''];
    }
    case 'perSec':
      return [plain(value), '/s'];
    case 'milliseconds':
      return [plain(value), 'ms'];
    case 'seconds':
      return [plain(value), 's'];
    case 'count':
    case 'number':
      return [plain(value), ''];
  }
}

/** One line for a tooltip or a screen reader: `1.2 MB/s`. */
export function counterText(value: number, unit: CounterUnit): string {
  return formatCounter(value, unit).filter(Boolean).join(' ');
}

/** The number a reading shows, or why not. */
export function readingNumber(r: CounterReading | undefined): number | null {
  return r && 'available' in r.value ? r.value.available : null;
}

export function readingReason(r: CounterReading | undefined): string | null {
  return r && 'unavailable' in r.value ? r.value.unavailable : null;
}

/** Adds `path` to a list unless it is there or the list is full; the input is not changed. */
export function withPath(list: readonly string[], path: string, max: number): string[] {
  return list.includes(path) || list.length >= max ? [...list] : [...list, path];
}

export function withoutPath(list: readonly string[], path: string): string[] {
  return list.filter((p) => p !== path);
}

/** The paths of the counters that dashboard widgets show. */
export function widgetCounterPaths(widgets: readonly DashboardWidget[]): string[] {
  const out: string[] = [];
  for (const w of widgets) {
    const m = w.metric;
    if (typeof m === 'object' && 'perfCounter' in m && !out.includes(m.perfCounter)) out.push(m.perfCounter);
  }
  return out;
}

const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif';

/** What a chart of counters shows: the option, and the counters it left out because it draws two units at most. */
export interface CounterChart {
  option: EChartsCoreOption;
  /** Paths not drawn (a third unit). */
  skipped: string[];
  /** The units on the left and right axes. */
  units: CounterUnit[];
}

const axisLabel = (unit: CounterUnit) => (v: number) => (unit === 'percent' ? `${v}%` : counterText(v, unit));

/**
 * The counters over time as lines (colour from the CVD-checked palette, never colour alone: the legend names each line),
 * one axis per unit (two at most, left and right), a 0 baseline, and the app's events on a lane under the plot.
 * `hidden` holds the paths the legend switched off.
 */
export function counterChartOption(history: CounterHistory, t: ThemeTokens, hidden: ReadonlySet<string> = new Set()): CounterChart {
  const units: CounterUnit[] = [];
  const skipped: string[] = [];
  const drawn = history.series.filter((s) => {
    if (!units.includes(s.unit)) {
      if (units.length >= 2) {
        skipped.push(s.path);
        return false;
      }
      units.push(s.unit);
    }
    return true;
  });
  const lane = history.events.map((e) => {
    const style = EVENT_STYLE[e.kind];
    return { name: e.label, value: [e.at, 0.5, e.kind], symbol: style.symbol, symbolSize: 11, itemStyle: { color: style.color(t) } };
  });
  const text = { color: t.text3, fontFamily: FONT, fontSize: 11 };
  const option: EChartsCoreOption = {
    animationDuration: 250,
    textStyle: { fontFamily: FONT },
    grid: [
      { left: 56, right: units.length > 1 ? 56 : 16, top: 34, bottom: 58 },
      { left: 56, right: units.length > 1 ? 56 : 16, height: 14, bottom: 14 },
    ],
    legend: { top: 0, left: 0, itemWidth: 14, itemHeight: 3, itemGap: 18, textStyle: { color: t.text2, fontFamily: FONT, fontSize: 12 }, icon: 'roundRect', selectedMode: false, data: drawn.map((s) => counterTitle(s.path)) },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      axisPointer: { type: 'line', lineStyle: { color: t.borderStrong } },
      formatter: (raw: unknown) => {
        const list = (raw as { axisValue: number; marker: string; seriesName: string; seriesType: string; value: [number, number]; data: { unit: CounterUnit } }[]).filter((p) => p.seriesType === 'line');
        if (!list.length) return '';
        const rows = list.map((p) => `${p.marker} ${p.seriesName}&nbsp;<b>${counterText(p.value[1], p.data.unit)}</b>`);
        return `${formatClock(list[0]!.axisValue)}<br/>${rows.join('<br/>')}`;
      },
    },
    xAxis: [
      { type: 'time', gridIndex: 0, min: history.from, max: history.to, axisLine: { lineStyle: { color: t.border } }, axisTick: { show: false }, axisLabel: { ...text, hideOverlap: true, formatter: '{HH}:{mm}:{ss}' }, splitLine: { show: false } },
      { type: 'time', gridIndex: 1, min: history.from, max: history.to, show: false },
    ],
    yAxis: [
      ...units.map((unit, i) => ({
        type: 'value',
        gridIndex: 0,
        position: i === 0 ? 'left' : 'right',
        min: 0,
        ...(unit === 'percent' ? { max: 100 } : {}),
        axisLabel: { ...text, formatter: axisLabel(unit) },
        splitLine: i === 0 ? { lineStyle: { color: t.chartGrid, type: 'dashed' } } : { show: false },
      })),
      { type: 'value', gridIndex: 1, min: 0, max: 1, show: false },
    ],
    dataZoom: [{ type: 'inside', xAxisIndex: [0, 1], filterMode: 'none' }],
    series: [
      ...drawn.map((s, i) => {
        const color = t.chart[i % t.chart.length]!;
        return {
          name: counterTitle(s.path),
          type: 'line',
          xAxisIndex: 0,
          yAxisIndex: units.indexOf(s.unit),
          showSymbol: false,
          smooth: 0.15,
          sampling: 'lttb',
          data: hidden.has(s.path) ? [] : s.points.map((p) => ({ value: [p.at, p.value], unit: s.unit })),
          lineStyle: { width: 1.5, color },
          itemStyle: { color },
          z: 3,
        };
      }),
      {
        name: 'Events',
        type: 'scatter',
        xAxisIndex: 1,
        yAxisIndex: units.length,
        data: lane,
        tooltip: { trigger: 'item', confine: true, backgroundColor: t.overlay, borderColor: t.border, textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 }, formatter: (p: { name: string; value: [number] }) => `${formatClock(p.value[0])} · ${p.name}` },
        z: 4,
      },
    ],
  };
  return { option, skipped, units };
}
