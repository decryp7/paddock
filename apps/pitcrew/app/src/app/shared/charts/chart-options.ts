import type { EChartsCoreOption } from 'echarts/core';
import type { JobDay } from '../../core/api.types';
import type { ChartTokens } from '../../core/theme.service';
import { formatAxisDuration, formatBytes, formatDay, formatDuration, formatShortDate, formatTime } from '../format';

/**
 * Pure builders of ECharts options from what the backend returned and the theme's chart tokens. Shaping only: sums
 * for display, axis formats, colours. Chart text is escaped (ECharts writes tooltips as HTML).
 */

/** A named time series (`[time, value]` points). */
export interface TimeSeries {
  name: string;
  points: [number, number][];
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch] ?? ch);
}

interface Row {
  color: string;
  label: string;
  value: string;
}

/** Tooltip markup: a title and one row per series, all text escaped. */
export function tooltipHtml(title: string, rows: Row[]): string {
  const lines = rows
    .map(
      (r) =>
        `<div style="display:flex;align-items:center;gap:8px;justify-content:space-between">` +
        `<span style="display:inline-flex;align-items:center;gap:6px"><span style="width:8px;height:8px;border-radius:2px;background:${escapeHtml(r.color)}"></span>${escapeHtml(r.label)}</span>` +
        `<b style="font-weight:600;font-variant-numeric:tabular-nums">${escapeHtml(r.value)}</b></div>`,
    )
    .join('');
  return `<div style="min-width:150px;display:grid;gap:4px"><div style="opacity:.7">${escapeHtml(title)}</div>${lines}</div>`;
}

/** Sums several runners' job days by day (for "all runners"). */
export function sumJobDays(lists: JobDay[][]): JobDay[] {
  const byStart = new Map<number, JobDay & { queued: number; queuedDays: number }>();
  for (const list of lists) {
    for (const d of list) {
      const acc = byStart.get(d.start) ?? { start: d.start, success: 0, failure: 0, cancelled: 0, unknown: 0, avgQueuedMs: null, queued: 0, queuedDays: 0 };
      acc.success += d.success;
      acc.failure += d.failure;
      acc.cancelled += d.cancelled;
      acc.unknown += d.unknown;
      if (d.avgQueuedMs !== null) {
        acc.queued += d.avgQueuedMs;
        acc.queuedDays += 1;
      }
      byStart.set(d.start, acc);
    }
  }
  return [...byStart.values()]
    .sort((a, b) => a.start - b.start)
    .map(({ queued, queuedDays, ...d }) => ({ ...d, avgQueuedMs: queuedDays > 0 ? Math.round(queued / queuedDays) : null }));
}

function base(t: ChartTokens): EChartsCoreOption {
  return {
    animationDuration: 300,
    animationDurationUpdate: 200,
    textStyle: { fontFamily: t.font, color: t.text2, fontSize: 11 },
    grid: { left: 8, right: 12, top: 36, bottom: 8, containLabel: true },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.surface,
      borderColor: t.axis,
      borderWidth: 1,
      padding: [8, 10],
      textStyle: { color: t.text, fontFamily: t.font, fontSize: 12 },
      extraCssText: 'border-radius:8px;box-shadow:0 8px 24px rgba(0,0,0,.18);',
      axisPointer: { type: 'line', lineStyle: { color: t.axis, width: 1 } },
    },
    legend: {
      type: 'scroll',
      top: 0,
      left: 0,
      pageIconColor: t.text2,
      pageIconInactiveColor: t.faint,
      pageTextStyle: { color: t.text3 },
      icon: 'roundRect',
      itemWidth: 10,
      itemHeight: 10,
      itemGap: 14,
      textStyle: { color: t.text2, fontSize: 12, fontFamily: t.font },
      inactiveColor: t.faint,
    },
  };
}

function valueAxis(t: ChartTokens, format: (v: number) => string): EChartsCoreOption {
  return {
    type: 'value',
    splitNumber: 3,
    axisLabel: { color: t.text3, formatter: format },
    splitLine: { lineStyle: { color: t.grid, width: 1 } },
    axisLine: { show: false },
    axisTick: { show: false },
  };
}

function timeAxis(t: ChartTokens, hours: number): EChartsCoreOption {
  return {
    type: 'time',
    axisLabel: { color: t.text3, hideOverlap: true, formatter: (v: number) => (hours <= 24 ? formatTime(v) : formatShortDate(v)) },
    axisLine: { lineStyle: { color: t.axis } },
    axisTick: { show: false },
    splitLine: { show: false },
  };
}

function zoom(enabled: boolean, t: ChartTokens): EChartsCoreOption[] {
  if (!enabled) return [];
  return [
    { type: 'inside', throttle: 50 },
    {
      type: 'slider',
      height: 14,
      bottom: 4,
      borderColor: 'transparent',
      backgroundColor: t.grid,
      fillerColor: `${t.series[0]}33`,
      dataBackground: { lineStyle: { color: t.muted }, areaStyle: { color: t.faint } },
      handleStyle: { color: t.surface, borderColor: t.axis },
      moveHandleSize: 0,
      showDetail: false,
      textStyle: { color: t.text3 },
    },
  ];
}

const OUTCOMES = [
  { key: 'success', label: 'Succeeded' },
  { key: 'failure', label: 'Failed' },
  { key: 'cancelled', label: 'Cancelled' },
  { key: 'unknown', label: 'Outcome unknown' },
] as const;

/** Jobs per day stacked by outcome. Outcomes: blue (succeeded) and orange (failed) stay apart under every CVD. */
export function jobsPerDayOption(days: JobDay[], t: ChartTokens, withZoom: boolean): EChartsCoreOption {
  const colors: Record<(typeof OUTCOMES)[number]['key'], string> = { success: t.series[0], failure: t.series[1], cancelled: t.muted, unknown: t.faint };
  const present = OUTCOMES.filter((o) => days.some((d) => d[o.key] > 0));
  const shown = present.length > 0 ? present : OUTCOMES.slice(0, 1);
  const top = days.map((d) => {
    for (let i = shown.length - 1; i >= 0; i--) if (d[shown[i]!.key] > 0) return i;
    return -1;
  });
  const o = base(t);
  return {
    ...o,
    grid: { ...(o['grid'] as object), bottom: withZoom ? 28 : 8 },
    tooltip: {
      ...(o['tooltip'] as object),
      axisPointer: { type: 'shadow', shadowStyle: { color: `${t.muted}22` } },
      formatter: (params: { dataIndex: number }[]) => {
        const d = days[params[0]?.dataIndex ?? 0];
        if (!d) return '';
        const total = d.success + d.failure + d.cancelled + d.unknown;
        return tooltipHtml(`${formatDay(d.start)} · ${total} ${total === 1 ? 'job' : 'jobs'}`, shown.map((s) => ({ color: colors[s.key], label: s.label, value: String(d[s.key]) })));
      },
    },
    xAxis: {
      type: 'category',
      data: days.map((d) => (days.length > 14 ? formatShortDate(d.start) : formatDay(d.start))),
      axisLabel: { color: t.text3, hideOverlap: true },
      axisLine: { lineStyle: { color: t.axis } },
      axisTick: { show: false },
    },
    yAxis: valueAxis(t, (v) => String(v)),
    dataZoom: zoom(withZoom, t),
    series: shown.map((s, si) => ({
      name: s.label,
      type: 'bar',
      stack: 'jobs',
      barMaxWidth: 24,
      itemStyle: { color: colors[s.key], borderColor: t.surface, borderWidth: 1 },
      emphasis: { focus: 'series' },
      data: days.map((d, di) => ({ value: d[s.key], itemStyle: top[di] === si ? { borderRadius: [4, 4, 0, 0] } : undefined })),
    })),
  };
}

/** Average job duration per day, one line per runner (categorical colours in fixed order). */
export function durationOption(series: TimeSeries[], t: ChartTokens, hours: number): EChartsCoreOption {
  const o = base(t);
  return {
    ...o,
    legend: { ...(o['legend'] as object), show: series.length > 1 },
    grid: { ...(o['grid'] as object), top: series.length > 1 ? 36 : 12 },
    tooltip: {
      ...(o['tooltip'] as object),
      formatter: (params: { seriesName: string; value: [number, number]; color: string }[]) =>
        tooltipHtml(
          formatDay(params[0]?.value[0] ?? 0),
          params.map((p) => ({ color: p.color, label: p.seriesName, value: formatDuration(p.value[1]) })),
        ),
    },
    xAxis: timeAxis(t, hours),
    yAxis: valueAxis(t, (v) => formatAxisDuration(v)),
    dataZoom: [{ type: 'inside', throttle: 50 }],
    series: series.map((s, i) => ({
      name: s.name,
      type: 'line',
      symbol: 'circle',
      symbolSize: 6,
      showSymbol: s.points.length <= 10,
      lineStyle: { width: 2, color: t.series[i % 4] },
      itemStyle: { color: t.series[i % 4], borderColor: t.surface, borderWidth: 2 },
      data: s.points.filter((p) => p[1] > 0),
    })),
  };
}

/** Average queue wait per day (needs a Gitea read token; days without data are gaps). */
export function queueOption(days: JobDay[], t: ChartTokens): EChartsCoreOption {
  const o = base(t);
  return {
    ...o,
    legend: { show: false },
    grid: { ...(o['grid'] as object), top: 12 },
    tooltip: {
      ...(o['tooltip'] as object),
      formatter: (params: { value: [number, number | null] }[]) => {
        const p = params[0];
        return p ? tooltipHtml(formatDay(p.value[0]), [{ color: t.series[2], label: 'Average wait', value: p.value[1] === null ? 'no data' : formatDuration(p.value[1]) }]) : '';
      },
    },
    xAxis: timeAxis(t, days.length * 24),
    yAxis: valueAxis(t, (v) => formatAxisDuration(v)),
    series: [
      {
        name: 'Queue wait',
        type: 'line',
        connectNulls: false,
        symbol: 'circle',
        symbolSize: 6,
        showSymbol: days.length <= 10,
        lineStyle: { width: 2, color: t.series[2] },
        itemStyle: { color: t.series[2], borderColor: t.surface, borderWidth: 2 },
        areaStyle: { color: `${t.series[2]}1a` },
        data: days.map((d) => [d.start + 12 * 3_600_000, d.avgQueuedMs]),
      },
    ],
  };
}

/** Runner folder sizes over time with the cleanups marked as thin vertical lines. */
export function diskOption(series: TimeSeries[], cleanups: number[], t: ChartTokens, hours: number): EChartsCoreOption {
  const o = base(t);
  return {
    ...o,
    legend: { ...(o['legend'] as object), show: series.length > 1 },
    grid: { ...(o['grid'] as object), top: series.length > 1 ? 36 : 12 },
    tooltip: {
      ...(o['tooltip'] as object),
      formatter: (params: { seriesName: string; value: [number, number]; color: string }[]) => {
        const at = params[0]?.value[0] ?? 0;
        return tooltipHtml(
          hours <= 24 ? formatTime(at) : `${formatShortDate(at)} ${formatTime(at)}`,
          params.map((p) => ({ color: p.color, label: p.seriesName, value: formatBytes(p.value[1]) })),
        );
      },
    },
    xAxis: timeAxis(t, hours),
    yAxis: valueAxis(t, (v) => formatBytes(v, 0)),
    dataZoom: [{ type: 'inside', throttle: 50 }],
    series: series.map((s, i) => ({
      name: s.name,
      type: 'line',
      showSymbol: false,
      lineStyle: { width: 2, color: t.series[i % 4] },
      itemStyle: { color: t.series[i % 4] },
      data: s.points,
      markLine:
        i === 0 && cleanups.length > 0
          ? {
              silent: true,
              symbol: 'none',
              label: { show: false },
              lineStyle: { color: t.muted, width: 1, type: 'solid' },
              data: cleanups.map((x) => ({ xAxis: x })),
            }
          : undefined,
    })),
  };
}
