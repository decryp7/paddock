import type { PaddockChartTokens } from '@decryp7/paddock-ui';
import type { Dashboard, WatchSummary } from '../../core/api.types';
import { bucketLabel } from '../format';

/** One stacked series of the releases chart. */
export interface ReleaseSeries {
  name: string;
  data: number[];
}

/** Series for the releases-over-time chart: watches with releases in the range, the busiest first, the rest as "Other". */
export function releaseSeries(d: Dashboard, watches: readonly Pick<WatchSummary, 'id' | 'name'>[], max = 6): ReleaseSeries[] {
  const named = d.watches
    .map((n) => ({ name: watches.find((w) => w.id === n.watchId)?.name ?? n.watchId, data: n.releases, total: n.releases.reduce((a, b) => a + b, 0) }))
    .filter((s) => s.total > 0)
    .sort((a, b) => b.total - a.total);
  const top = named.slice(0, max).map(({ name, data }) => ({ name, data }));
  const rest = named.slice(max);
  if (rest.length) top.push({ name: 'Other', data: d.bucketsMs.map((_, i) => rest.reduce((sum, s) => sum + (s.data[i] ?? 0), 0)) });
  return top;
}

/** A sentence describing the chart for assistive technology. */
export function releaseChartSummary(d: Dashboard, series: readonly ReleaseSeries[]): string {
  const total = series.reduce((a, s) => a + s.data.reduce((x, y) => x + y, 0), 0);
  const unit = d.range === '1y' ? 'week' : 'day';
  return `Releases found per ${unit}: ${total} in total across ${series.length} watch${series.length === 1 ? '' : 'es'}.`;
}

/** Releases per bucket across every watch: the one number the chart shows. */
export function releaseTotals(d: Dashboard): number[] {
  return d.bucketsMs.map((_, i) => d.watches.reduce((sum, n) => sum + (n.releases[i] ?? 0), 0));
}

/**
 * The ECharts option for the releases chart: one series of bars from zero in the accent colour, the unit on the axis
 * and the day (or week) in the tooltip. Fonts and the axis colours come from the paddock theme.
 */
export function releasesOption(d: Dashboard, _series?: readonly ReleaseSeries[]): Record<string, unknown> {
  const unit = d.range === '1y' ? 'week' : 'day';
  return {
    grid: { left: 36, right: 8, top: 24, bottom: 24 },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    xAxis: { type: 'category', data: d.bucketsMs.map(bucketLabel), axisTick: { show: false } },
    yAxis: { type: 'value', minInterval: 1, min: 0, name: `Releases per ${unit}`, nameTextStyle: { align: 'left' } },
    series: [{ name: 'Releases', type: 'bar', data: releaseTotals(d), barMaxWidth: 18, emphasis: { focus: 'series' } }],
  };
}

/** The colours the reliability and downloads charts use (a subset of the paddock chart tokens, so tests can pass plain values). */
export type OutcomeColors = Pick<PaddockChartTokens, 'neutralBar' | 'accent' | 'danger' | 'textMuted'>;

/** Diagonal hatching for a failure series: failures are told apart from successes by pattern as well as colour (colour-vision safe). */
const FAILED_DECAL = { symbol: 'rect', dashArrayX: [1, 0], dashArrayY: [2, 3], rotation: Math.PI / 4, color: 'rgba(0,0,0,0.45)' };

const MIB = 1024 * 1024;

/** Checks per bucket across every watch, split into the ones that worked and the ones that failed. */
export function reliabilityTotals(d: Dashboard): { succeeded: number[]; failed: number[] } {
  const failed = d.bucketsMs.map((_, i) => d.watches.reduce((sum, n) => sum + (n.failedChecks[i] ?? 0), 0));
  const succeeded = d.bucketsMs.map((_, i) => Math.max(0, d.watches.reduce((sum, n) => sum + (n.checks[i] ?? 0), 0) - (failed[i] ?? 0)));
  return { succeeded, failed };
}

/** How many checks ran and how many failed in the range. */
export function reliabilityCounts(d: Dashboard): { total: number; failed: number } {
  const t = reliabilityTotals(d);
  const failed = t.failed.reduce((a, b) => a + b, 0);
  return { total: failed + t.succeeded.reduce((a, b) => a + b, 0), failed };
}

/** A sentence describing the reliability chart for assistive technology. */
export function reliabilitySummary(d: Dashboard): string {
  const { total, failed } = reliabilityCounts(d);
  const unit = d.range === '1y' ? 'week' : 'day';
  return `Checks per ${unit}: ${total} in total, ${failed} failed.`;
}

/**
 * The reliability chart: checks per day (week for a year) as stacked bars from zero, the ones that worked in the neutral
 * bar colour and the failed ones hatched in the danger colour on top, with a legend because there are two series.
 */
export function reliabilityOption(d: Dashboard, c: OutcomeColors): Record<string, unknown> {
  const unit = d.range === '1y' ? 'week' : 'day';
  const t = reliabilityTotals(d);
  return {
    grid: { left: 40, right: 8, top: 56, bottom: 28 },
    legend: { top: 0, left: 0, data: ['Succeeded', 'Failed'] },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    xAxis: { type: 'category', data: d.bucketsMs.map(bucketLabel), axisTick: { show: false } },
    yAxis: { type: 'value', minInterval: 1, min: 0, name: `Checks per ${unit}`, nameTextStyle: { align: 'left' } },
    series: [
      { name: 'Succeeded', type: 'bar', stack: 'checks', data: t.succeeded, barMaxWidth: 18, itemStyle: { color: c.neutralBar } },
      { name: 'Failed', type: 'bar', stack: 'checks', data: t.failed, barMaxWidth: 18, itemStyle: { color: c.danger, decal: FAILED_DECAL } },
    ],
  };
}

/** Downloads per bucket and the data they moved, in MiB. */
export function downloadTotals(d: Dashboard): { completed: number[]; failed: number[]; mebibytes: number[] } {
  return {
    completed: d.downloads.map((b) => Math.max(0, b.count - b.failed)),
    failed: d.downloads.map((b) => b.failed),
    mebibytes: d.downloads.map((b) => Math.round((b.bytes / MIB) * 10) / 10),
  };
}

/** How many downloads ran and how many failed in the range. */
export function downloadCounts(d: Dashboard): { total: number; failed: number } {
  return { total: d.downloads.reduce((a, b) => a + b.count, 0), failed: d.downloads.reduce((a, b) => a + b.failed, 0) };
}

/** A sentence describing the downloads chart for assistive technology. */
export function downloadsSummary(d: Dashboard): string {
  const { total, failed } = downloadCounts(d);
  const mib = Math.round(d.downloads.reduce((a, b) => a + b.bytes, 0) / MIB);
  return `Downloads: ${total} in total, ${failed} failed, ${mib} MiB.`;
}

/**
 * The downloads chart, two panels that share the time axis: the count per day as stacked bars (completed in the accent
 * colour, failed hatched in the danger colour) above, and the data downloaded in MiB as one line below.
 */
export function downloadsOption(d: Dashboard, c: OutcomeColors): Record<string, unknown> {
  const unit = d.range === '1y' ? 'week' : 'day';
  const t = downloadTotals(d);
  const labels = d.bucketsMs.map(bucketLabel);
  return {
    grid: [
      { left: 48, right: 8, top: 56, height: 96 },
      { left: 48, right: 8, top: 204, height: 80 },
    ],
    legend: { top: 0, left: 0, data: ['Completed', 'Failed', 'Downloaded'] },
    tooltip: { trigger: 'axis', axisPointer: { type: 'line', link: [{ xAxisIndex: 'all' }] } },
    xAxis: [
      { type: 'category', gridIndex: 0, data: labels, axisTick: { show: false }, axisLabel: { show: false } },
      { type: 'category', gridIndex: 1, data: labels, axisTick: { show: false } },
    ],
    yAxis: [
      { type: 'value', gridIndex: 0, minInterval: 1, min: 0, name: `Downloads per ${unit}`, nameTextStyle: { align: 'left' } },
      { type: 'value', gridIndex: 1, min: 0, splitNumber: 2, name: 'Downloaded (MiB)', nameTextStyle: { align: 'left' } },
    ],
    series: [
      { name: 'Completed', type: 'bar', stack: 'count', xAxisIndex: 0, yAxisIndex: 0, data: t.completed, barMaxWidth: 18, itemStyle: { color: c.accent } },
      { name: 'Failed', type: 'bar', stack: 'count', xAxisIndex: 0, yAxisIndex: 0, data: t.failed, barMaxWidth: 18, itemStyle: { color: c.danger, decal: FAILED_DECAL } },
      { name: 'Downloaded', type: 'line', xAxisIndex: 1, yAxisIndex: 1, data: t.mebibytes, lineStyle: { color: c.textMuted }, itemStyle: { color: c.textMuted }, showSymbol: false },
    ],
  };
}
