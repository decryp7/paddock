import type { EChartsCoreOption } from 'echarts/core';
import type { Consumer, DiskTrend, History, HistoryPoint, TimelineEvent, TimelineEventKind } from '../../core/api.types';
import { forecastText } from '../../shared/labels';
import { formatBytes, formatClock, formatDate, formatPercent } from '../../shared/format';
import type { ThemeTokens } from '../../shared/tokens';

/** How each kind of timeline event is drawn: a colour and a distinct shape (never colour alone). */
export interface EventStyle {
  symbol: string;
  color: (t: ThemeTokens) => string;
}

export const EVENT_STYLE: Record<TimelineEventKind, EventStyle> = {
  throttle: { symbol: 'triangle', color: (t) => t.chart[1]! },
  cap: { symbol: 'rect', color: (t) => t.chart[4]! },
  release: { symbol: 'circle', color: (t) => t.chart[2]! },
  cleanup: { symbol: 'diamond', color: (t) => t.chart[0]! },
  tuning: { symbol: 'roundRect', color: (t) => t.chart[5]! },
  alert: { symbol: 'pin', color: (t) => t.bad },
  boot: { symbol: 'arrow', color: (t) => t.chart[3]! },
  network: { symbol: 'emptyCircle', color: (t) => t.chart[6]! },
};

const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif';

const axisText = (t: ThemeTokens) => ({ color: t.text3, fontFamily: FONT, fontSize: 11 });

/** The value of a series at the point nearest to `at` (events sit on the line). */
export function nearestValue(points: readonly HistoryPoint[], at: number, pick: (p: HistoryPoint) => number): number {
  if (points.length === 0) return 0;
  let best = points[0]!;
  for (const p of points) if (Math.abs(p.at - at) < Math.abs(best.at - at)) best = p;
  return pick(best);
}

/** Counts of each event kind, in the order the legend shows them. */
export function eventCounts(events: readonly TimelineEvent[]): { kind: TimelineEventKind; count: number }[] {
  const counts = new Map<TimelineEventKind, number>();
  for (const e of events) counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
  return (Object.keys(EVENT_STYLE) as TimelineEventKind[]).filter((k) => counts.has(k)).map((kind) => ({ kind, count: counts.get(kind)! }));
}

function axisTime(history: History): string {
  return history.range === 'hour' || history.range === 'day' ? '{HH}:{mm}' : '{d} {MMM}';
}

function tooltipTime(at: number, range: History['range']): string {
  return range === 'hour' || range === 'day' ? formatClock(at) : `${formatDate(at)} ${formatClock(at)}`;
}

/**
 * CPU and memory over time (lines with the peak band for CPU), with the rule actions, cleanups, tuning and alerts of
 * the range marked on the CPU line as points and as faint vertical lines, a zoom slider and mouse-wheel zoom.
 * `kinds` hides events whose kind the legend switched off.
 */
export function historyOption(history: History, t: ThemeTokens, hidden: ReadonlySet<TimelineEventKind> = new Set()): EChartsCoreOption {
  const events = history.events.filter((e) => !hidden.has(e.kind));
  const cpuColor = t.chart[0]!;
  const memColor = t.chart[1]!;
  // Events sit on their own 14 px lane under the plot (one marker per event, shape-coded), not on the lines.
  const lane = events.map((e) => {
    const style = EVENT_STYLE[e.kind];
    return { name: e.label, value: [e.at, 0.5, e.kind], symbol: style.symbol, symbolSize: 11, itemStyle: { color: style.color(t) } };
  });
  return {
    animationDuration: 250,
    textStyle: { fontFamily: FONT },
    grid: [
      { left: 40, right: 16, top: 34, bottom: 58 },
      { left: 40, right: 16, height: 14, bottom: 14 },
    ],
    legend: { top: 0, left: 0, itemWidth: 14, itemHeight: 3, itemGap: 18, textStyle: { color: t.text2, fontFamily: FONT, fontSize: 12 }, icon: 'roundRect', selectedMode: true, data: ['CPU', 'Memory'] },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      axisPointer: { type: 'line', lineStyle: { color: t.borderStrong } },
      formatter: (raw: unknown) => {
        const list = (raw as { axisValue: number; marker: string; seriesName: string; seriesType: string; value: [number, number] }[]).filter((p) => p.seriesType === 'line');
        if (!list.length) return '';
        const rows = list.map((p) => `${p.marker} ${p.seriesName}&nbsp;<b>${formatPercent(p.value[1])}</b>`);
        return `${tooltipTime(list[0]!.axisValue, history.range)}<br/>${rows.join('<br/>')}`;
      },
    },
    xAxis: [
      {
        type: 'time',
        gridIndex: 0,
        min: history.from,
        max: history.to,
        axisLine: { lineStyle: { color: t.border } },
        axisTick: { show: false },
        axisLabel: { ...axisText(t), hideOverlap: true, formatter: axisTime(history) },
        splitLine: { show: false },
      },
      { type: 'time', gridIndex: 1, min: history.from, max: history.to, show: false },
    ],
    yAxis: [
      {
        type: 'value',
        gridIndex: 0,
        min: 0,
        max: 100,
        interval: 50,
        axisLabel: { ...axisText(t), formatter: '{value}%' },
        splitLine: { lineStyle: { color: t.chartGrid, type: 'dashed' } },
      },
      { type: 'value', gridIndex: 1, min: 0, max: 1, show: false },
    ],
    dataZoom: [{ type: 'inside', xAxisIndex: [0, 1], filterMode: 'none' }],
    series: [
      {
        name: 'CPU',
        type: 'line',
        xAxisIndex: 0,
        yAxisIndex: 0,
        showSymbol: false,
        smooth: 0.2,
        sampling: 'lttb',
        data: history.points.map((p) => [p.at, p.cpu]),
        lineStyle: { width: 1.5, color: cpuColor },
        itemStyle: { color: cpuColor },
        areaStyle: { color: { type: 'linear', x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: `${cpuColor}47` }, { offset: 1, color: `${cpuColor}00` }] } },
        z: 3,
      },
      {
        name: 'Memory',
        type: 'line',
        xAxisIndex: 0,
        yAxisIndex: 0,
        showSymbol: false,
        smooth: 0.2,
        sampling: 'lttb',
        data: history.points.map((p) => [p.at, p.memory]),
        lineStyle: { width: 1.5, color: memColor },
        itemStyle: { color: memColor },
        z: 2,
      },
      {
        name: 'Events',
        type: 'scatter',
        xAxisIndex: 1,
        yAxisIndex: 1,
        data: lane,
        tooltip: { trigger: 'item', confine: true, backgroundColor: t.overlay, borderColor: t.border, textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 }, formatter: (p: { name: string; value: [number] }) => `${formatClock(p.value[0])} · ${p.name}` },
        z: 4,
      },
    ],
  };
}

/** The apps that used the most over the range, as a horizontal bar chart of average CPU. */
export function consumersOption(consumers: readonly Consumer[], t: ThemeTokens): EChartsCoreOption {
  const top = [...consumers].sort((a, b) => b.cpuAvg - a.cpuAvg).slice(0, 6).reverse();
  return {
    animationDuration: 250,
    textStyle: { fontFamily: FONT },
    grid: { left: 4, right: 48, top: 6, bottom: 6, containLabel: true },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'none' },
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      formatter: (raw: unknown) => {
        const p = (raw as { dataIndex: number }[])[0];
        const c = p ? top[p.dataIndex] : undefined;
        return c ? `${c.app}<br/>CPU <b>${formatPercent(c.cpuAvg)}</b> · memory <b>${formatBytes(c.memoryAvg)}</b>` : '';
      },
    },
    xAxis: { type: 'value', show: false, max: (v: { max: number }) => v.max * 1.15 },
    yAxis: {
      type: 'category',
      data: top.map((c) => c.app),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: t.text2, fontFamily: FONT, fontSize: 12, width: 130, overflow: 'truncate' },
    },
    series: [
      {
        type: 'bar',
        barWidth: 10,
        data: top.map((c) => c.cpuAvg),
        itemStyle: { color: t.chart[0]!, borderRadius: 3 },
        label: { show: true, position: 'right', color: t.text2, fontFamily: FONT, fontSize: 11, formatter: (p: { value: number }) => formatPercent(p.value) },
      },
    ],
  };
}

/** The line a disk's free space follows, then (when the engine forecasts it) a dashed line to the day it would be full. */
export function diskTrendOption(trend: DiskTrend, t: ThemeTokens): EChartsCoreOption {
  const last = trend.points[trend.points.length - 1];
  const day = 24 * 3600 * 1000;
  const projection =
    last && typeof trend.forecast === 'object' ? [[last.at, last.free], [last.at + trend.forecast.fullInDays * day, 0]] : [];
  const color = t.chart[2]!;
  return {
    animationDuration: 250,
    textStyle: { fontFamily: FONT },
    grid: { left: 8, right: 14, top: 12, bottom: 22, containLabel: true },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      formatter: (raw: unknown) => {
        const list = raw as { axisValue: number; seriesName: string; value: [number, number] }[];
        const p = list[0];
        return p ? `${formatDate(p.axisValue)}<br/>${p.seriesName}: <b>${formatBytes(p.value[1])}</b> free` : '';
      },
    },
    xAxis: { type: 'time', axisLine: { lineStyle: { color: t.border } }, axisTick: { show: false }, axisLabel: { ...axisText(t), hideOverlap: true, formatter: '{d} {MMM}' }, splitLine: { show: false } },
    yAxis: {
      type: 'value',
      min: 0,
      axisLabel: { ...axisText(t), formatter: (v: number) => formatBytes(v) },
      splitLine: { lineStyle: { color: t.chartGrid } },
    },
    series: [
      {
        name: 'Free space',
        type: 'line',
        showSymbol: false,
        data: trend.points.map((p) => [p.at, p.free]),
        lineStyle: { width: 2, color },
        itemStyle: { color },
        areaStyle: { color, opacity: 0.1 },
      },
      ...(projection.length
        ? [
            {
              name: `Forecast (${forecastText(trend.forecast)})`,
              type: 'line',
              showSymbol: false,
              data: projection,
              lineStyle: { width: 2, color: t.bad, type: 'dashed' as const },
              itemStyle: { color: t.bad },
              markPoint: { symbol: 'circle', symbolSize: 8, itemStyle: { color: t.bad }, label: { show: false }, data: [{ coord: projection[1] }] },
            },
          ]
        : []),
    ],
  };
}
