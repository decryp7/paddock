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

/** The ECharts option for stacked bars of releases per bucket (colours and fonts come from the paddock theme). */
export function releasesOption(d: Dashboard, series: readonly ReleaseSeries[]): Record<string, unknown> {
  return {
    grid: { left: 28, right: 8, top: 36, bottom: 24 },
    legend: { type: 'scroll' },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    xAxis: { type: 'category', data: d.bucketsMs.map(bucketLabel), axisTick: { show: false } },
    yAxis: { type: 'value', minInterval: 1 },
    series: series.map((s) => ({ name: s.name, type: 'bar', stack: 'releases', data: s.data, emphasis: { focus: 'series' } })),
  };
}
