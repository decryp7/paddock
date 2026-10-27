import type { Dashboard, WatchNumbers } from '../../core/api.types';
import {
  downloadCounts,
  downloadsOption,
  downloadsSummary,
  downloadTotals,
  reliabilityCounts,
  reliabilityOption,
  reliabilitySummary,
  reliabilityTotals,
  type OutcomeColors,
} from './chart-options';

const colors: OutcomeColors = { neutralBar: '#5e6775', accent: '#2cc4b0', danger: '#ff6b6b', textMuted: '#999999' };
const num = (id: string, checks: number[], failedChecks: number[]): WatchNumbers => ({
  watchId: id,
  averageIntervalMs: null,
  sinceLastMs: null,
  releases: [],
  failedChecks,
  checks,
  sparkline: [],
});
const d: Dashboard = {
  range: '30d',
  bucketsMs: [0, 86_400_000, 172_800_000],
  whatsNew: [],
  watches: [num('a', [3, 3, 3], [0, 1, 0]), num('b', [2, 2], [1, 0])],
  downloads: [
    { count: 0, failed: 0, bytes: 0 },
    { count: 3, failed: 1, bytes: 3 * 1048576 + 52429 },
    { count: 1, failed: 0, bytes: 10 * 1048576 },
  ],
  computeMicros: 1,
};

describe('reliability chart', () => {
  it('splits the checks of every watch into succeeded and failed per bucket', () => {
    expect(reliabilityTotals(d)).toEqual({ succeeded: [4, 4, 3], failed: [1, 1, 0] });
    expect(reliabilityCounts(d)).toEqual({ total: 13, failed: 2 });
    expect(reliabilityCounts({ ...d, watches: [] })).toEqual({ total: 0, failed: 0 });
  });

  it('never goes below zero when more failures than checks are reported', () => {
    expect(reliabilityTotals({ ...d, watches: [num('a', [1], [3])] }).succeeded[0]).toBe(0);
  });

  it('stacks neutral successes under hatched failures, from zero, with a legend and the unit', () => {
    const o = reliabilityOption(d, colors) as {
      legend: { data: string[] };
      yAxis: { min: number; name: string };
      series: { name: string; stack: string; data: number[]; itemStyle: { color: string; decal?: { dashArrayY: number[] } } }[];
    };
    expect(o.legend.data).toEqual(['Succeeded', 'Failed']);
    expect(o.yAxis).toMatchObject({ min: 0, name: 'Checks per day' });
    expect(o.series.map((s) => s.name)).toEqual(['Succeeded', 'Failed']);
    expect(new Set(o.series.map((s) => s.stack)).size).toBe(1);
    expect(o.series[0]!.itemStyle).toEqual({ color: colors.neutralBar });
    expect(o.series[1]!.itemStyle.color).toBe(colors.danger);
    expect(o.series[1]!.itemStyle.decal, 'failures are told apart by pattern, not only by colour').toBeDefined();
    expect((reliabilityOption({ ...d, range: '1y' }, colors) as { yAxis: { name: string } }).yAxis.name).toBe('Checks per week');
  });

  it('describes itself in words', () => {
    expect(reliabilitySummary(d)).toBe('Checks per day: 13 in total, 2 failed.');
    expect(reliabilitySummary({ ...d, range: '1y' })).toContain('per week');
  });
});

describe('downloads chart', () => {
  it('counts completed and failed downloads and converts bytes to MiB', () => {
    expect(downloadTotals(d)).toEqual({ completed: [0, 2, 1], failed: [0, 1, 0], mebibytes: [0, 3.1, 10] });
    expect(downloadCounts(d)).toEqual({ total: 4, failed: 1 });
    expect(downloadCounts({ ...d, downloads: [] })).toEqual({ total: 0, failed: 0 });
  });

  it('draws two panels on one time axis: counts as stacked bars above, MiB as a line below', () => {
    const o = downloadsOption(d, colors) as {
      grid: { top: number; height: number }[];
      xAxis: { gridIndex: number; axisLabel?: { show: boolean } }[];
      yAxis: { gridIndex: number; name: string; min: number }[];
      series: { name: string; type: string; stack?: string; xAxisIndex: number; itemStyle: { color: string; decal?: unknown } }[];
      legend: { data: string[] };
      tooltip: { axisPointer: { link: unknown[] } };
    };
    expect(o.grid).toHaveLength(2);
    const [g0, g1] = o.grid as [{ top: number; height: number }, { top: number; height: number }];
    expect(g0.top + g0.height, 'the panels do not overlap').toBeLessThan(g1.top);
    expect(o.xAxis[0]!.axisLabel).toEqual({ show: false });
    expect(o.yAxis.map((a) => a.name)).toEqual(['Downloads per day', 'Downloaded (MiB)']);
    expect(o.yAxis.every((a) => a.min === 0)).toBe(true);
    expect(o.series.map((s) => [s.name, s.type, s.xAxisIndex])).toEqual([
      ['Completed', 'bar', 0],
      ['Failed', 'bar', 0],
      ['Downloaded', 'line', 1],
    ]);
    expect(o.series[0]!.itemStyle.color).toBe(colors.accent);
    expect(o.series[1]!.itemStyle.color).toBe(colors.danger);
    expect(o.series[1]!.itemStyle.decal).toBeDefined();
    expect(o.legend.data).toEqual(['Completed', 'Failed', 'Downloaded']);
    expect(o.tooltip.axisPointer.link).toHaveLength(1);
    expect((downloadsOption({ ...d, range: '1y' }, colors) as { yAxis: { name: string }[] }).yAxis[0]!.name).toBe('Downloads per week');
  });

  it('describes itself in words', () => {
    expect(downloadsSummary(d)).toBe('Downloads: 4 in total, 1 failed, 13 MiB.');
  });
});
