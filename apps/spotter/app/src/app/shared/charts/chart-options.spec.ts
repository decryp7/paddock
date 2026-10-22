import type { Dashboard } from '../../core/api.types';
import { releaseChartSummary, releaseSeries, releaseTotals, releasesOption } from './chart-options';

const num = (id: string, releases: number[]) => ({ watchId: id, averageIntervalMs: null, sinceLastMs: null, releases, failedChecks: [], checks: [], sparkline: [] });
const d: Dashboard = {
  range: '30d',
  bucketsMs: [0, 86_400_000, 172_800_000],
  whatsNew: [],
  watches: [num('a', [1, 0, 2]), num('b', [0, 0, 0]), num('c', [1, 1, 1]), num('d', [5, 0, 0])],
  downloads: [],
  computeMicros: 1,
};
const names = [{ id: 'a', name: 'Alpha' }, { id: 'c', name: 'Gamma' }, { id: 'd', name: 'Delta' }];

describe('chart options', () => {
  it('keeps watches with releases, the busiest first', () => {
    expect(releaseSeries(d, names).map((s) => s.name)).toEqual(['Delta', 'Alpha', 'Gamma']);
  });

  it('folds the rest into Other', () => {
    const s = releaseSeries(d, names, 1);
    expect(s.map((x) => x.name)).toEqual(['Delta', 'Other']);
    expect(s[1]?.data).toEqual([2, 1, 3]);
  });

  it('falls back to the id for an unknown watch', () => {
    expect(releaseSeries(d, [], 9)[0]?.name).toBe('d');
  });

  it('sums every watch per bucket', () => {
    expect(releaseTotals(d)).toEqual([7, 1, 3]);
    expect(releaseTotals({ ...d, watches: [] })).toEqual([0, 0, 0]);
    expect(releaseTotals({ ...d, watches: [num('x', [1])] })).toEqual([1, 0, 0]);
  });

  it('summarises the releases', () => {
    const series = releaseSeries(d, names);
    expect(releaseChartSummary(d, series)).toContain('11 in total across 3 watches');
    expect(releaseChartSummary({ ...d, range: '1y' }, series.slice(0, 1))).toContain('per week');
    expect(releaseChartSummary(d, series.slice(0, 1))).toContain('1 watch.');
  });

  it('builds one series of per-bucket totals from zero', () => {
    const o = releasesOption(d) as { series: { name: string; data: number[]; stack?: string }[]; xAxis: { data: string[] }; yAxis: { min: number; name: string } };
    expect(o.series.length).toBe(1);
    expect(o.series[0]?.data).toEqual([7, 1, 3]);
    expect(o.series[0]?.stack).toBeUndefined();
    expect(o.xAxis.data.length).toBe(3);
    expect(o.yAxis.min).toBe(0);
    expect(o.yAxis.name).toBe('Releases per day');
    expect((releasesOption({ ...d, range: '1y' }) as { yAxis: { name: string } }).yAxis.name).toBe('Releases per week');
  });
});
