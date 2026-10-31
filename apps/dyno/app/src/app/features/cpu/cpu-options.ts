import type { EChartsCoreOption } from 'echarts/core';
import type { CoreKind, History } from '../../core/api.types';
import { formatClock, formatDate, formatPercent } from '../../shared/format';
import type { ThemeTokens } from '../../shared/tokens';
import { coreOrder } from './core-layout';

const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif';

/** One heatmap cell: [time index, row index, average, min, max]. */
export type HeatCell = [number, number, number, number, number];

/** The cells of the cores x time heatmap, rows in display order (Performance first, then Efficiency). Pure. */
export function heatCells(history: History, kinds: readonly CoreKind[]): { cells: HeatCell[]; labels: string[]; times: number[] } {
  const count = history.corePoints[0]?.cores.length ?? 0;
  const order = coreOrder(kinds, count);
  const cells: HeatCell[] = [];
  history.corePoints.forEach((b, x) => {
    order.forEach((slot, row) => {
      const c = b.cores[slot.index];
      if (c) cells.push([x, row, c.avg, c.min, c.max]);
    });
  });
  return { cells, labels: order.map((o) => o.label), times: history.corePoints.map((b) => b.at) };
}

/** Index of the bucket nearest to `at` (events are drawn on the category axis). */
export function nearestIndex(times: readonly number[], at: number): number {
  let best = 0;
  times.forEach((t, i) => {
    if (Math.abs(t - at) < Math.abs(times[best]! - at)) best = i;
  });
  return best;
}

/** Whether the time label `label` (`14:30`, or `22 Oct 14:30`) is a round tick for `range`. */
export function roundTick(range: History['range'], label: string): boolean {
  const m = /(\d{1,2}):(\d{2})$/.exec(label);
  if (!m) return false;
  const hour = Number(m[1]);
  const minute = Number(m[2]);
  if (range === 'hour') return minute % 10 === 0;
  if (range === 'day') return minute === 0 && hour % 3 === 0;
  return minute === 0 && hour === 0;
}

/** The per-core heatmap: one row per core, one column per bucket, colour = average load, with the range's events marked. */
export function coreHeatmapOption(history: History, kinds: readonly CoreKind[], t: ThemeTokens): EChartsCoreOption {
  const { cells, labels, times } = heatCells(history, kinds);
  const short = history.range === 'hour' || history.range === 'day';
  const stamp = (at: number) => (short ? formatClock(at) : `${formatDate(at)} ${formatClock(at)}`);
  return {
    animation: false,
    textStyle: { fontFamily: FONT },
    grid: { left: 36, right: 16, top: 8, bottom: 64 },
    tooltip: {
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      formatter: (raw: unknown) => {
        const v = (raw as { value: HeatCell }).value;
        return `${labels[v[1]]} · ${stamp(times[v[0]] ?? 0)}<br/>Average <b>${formatPercent(v[2])}</b> · min ${formatPercent(v[3])} · max ${formatPercent(v[4])}`;
      },
    },
    xAxis: {
      type: 'category',
      data: times.map((x) => stamp(x)),
      splitArea: { show: false },
      axisLine: { lineStyle: { color: t.border } },
      axisTick: { show: false },
      // Round times only: every ten minutes in the hour, every three hours in the day, midnight in the week and the month.
      axisLabel: { color: t.text3, fontFamily: FONT, fontSize: 11, hideOverlap: true, interval: (_i: number, label: string) => roundTick(history.range, label) },
    },
    yAxis: { type: 'category', data: labels, inverse: true, axisLine: { show: false }, axisTick: { show: false }, axisLabel: { color: t.text2, fontFamily: FONT, fontSize: 11 } },
    visualMap: {
      type: 'continuous',
      min: 0,
      max: 100,
      calculable: false,
      orient: 'horizontal',
      left: 'center',
      bottom: 6,
      itemWidth: 10,
      itemHeight: 160,
      text: ['100%', '0%'],
      textStyle: { color: t.text3, fontFamily: FONT, fontSize: 11 },
      inRange: { color: [t.surface3, t.accentSoft, t.accent] },
    },
    series: [
      {
        type: 'heatmap',
        data: cells,
        itemStyle: { borderColor: t.surface2, borderWidth: 0.5 },
        progressive: 0,
        // Event lines are left off the heatmap (they hide the cells); the dashboard history chart carries the events lane.
        markLine: { symbol: 'none', silent: true, animation: false, label: { show: false }, data: [] },
        emphasis: { itemStyle: { borderColor: t.text1, borderWidth: 1 } },
      },
    ],
  };
}
