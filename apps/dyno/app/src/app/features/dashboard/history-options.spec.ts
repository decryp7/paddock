import type { DiskTrend, History, HistoryPoint, TimelineEvent } from '../../core/api.types';
import { DARK, LIGHT } from '../../shared/tokens';
import { EVENT_STYLE, consumersOption, diskTrendOption, eventCounts, historyOption, nearestValue } from './history-options';

const pt = (at: number, cpu: number, memory = 50): HistoryPoint => ({
  at, cpu, cpuMin: cpu, cpuMax: cpu, memory, memoryMin: memory, memoryMax: memory, diskReadBps: 0, diskWriteBps: 0, netRxBps: 0, netTxBps: 0, gpu: null, temperature: null,
});
const ev = (at: number, kind: TimelineEvent['kind']): TimelineEvent => ({ at, kind, label: `${kind}@${at}` });
const history = (over: Partial<History> = {}): History => ({
  range: 'day', from: 0, to: 300, resolutionMs: 100, points: [pt(100, 10), pt(200, 40), pt(300, 20)],
  events: [ev(190, 'boot'), ev(290, 'alert'), ev(100, 'boot')], topConsumers: [], diskTrends: [], boots: [], crashes: [], corePoints: [], ...over,
});
type Series = { name: string; data: unknown[]; markPoint?: { data: unknown[] }; markLine?: { data: unknown[] }; lineStyle?: { type?: string } };
const series = (o: unknown) => (o as { series: Series[] }).series;

describe('nearestValue', () => {
  const points = [pt(100, 1), pt(200, 2), pt(300, 3)];
  it('picks the closest point', () => {
    expect(nearestValue(points, 190, (p) => p.cpu)).toBe(2);
    expect(nearestValue(points, 0, (p) => p.cpu)).toBe(1);
    expect(nearestValue(points, 1e9, (p) => p.cpu)).toBe(3);
  });
  it('is 0 without points', () => {
    expect(nearestValue([], 5, (p) => p.cpu)).toBe(0);
  });
});

describe('eventCounts', () => {
  it('counts kinds in legend order and omits absent ones', () => {
    const out = eventCounts([ev(1, 'alert'), ev(2, 'boot'), ev(3, 'alert'), ev(4, 'boot'), ev(5, 'boot')]);
    expect(out).toEqual([{ kind: 'alert', count: 2 }, { kind: 'boot', count: 3 }]);
    expect(eventCounts([])).toEqual([]);
  });
  it('gives every kind a distinct shape', () => {
    const shapes = Object.values(EVENT_STYLE).map((s) => s.symbol);
    expect(new Set(shapes).size).toBe(shapes.length);
    for (const s of Object.values(EVENT_STYLE)) expect(s.color(LIGHT)).toEqual(expect.any(String));
  });
});

describe('historyOption', () => {
  it('has CPU and Memory lines over the points and an events lane', () => {
    const s = series(historyOption(history(), LIGHT));
    expect(s.map((x) => x.name)).toEqual(['CPU', 'Memory', 'Events']);
    expect(s[0]!.data).toEqual([[100, 10], [200, 40], [300, 20]]);
    expect(s[1]!.data).toEqual([[100, 50], [200, 50], [300, 50]]);
  });
  it('puts every event on the lane with a shape, not on the lines', () => {
    const [cpu, , lane] = series(historyOption(history(), LIGHT)) as [Series, Series, Series];
    expect(cpu.markPoint).toBeUndefined();
    expect(lane.data).toHaveLength(3);
    expect(lane.data[0]).toMatchObject({ symbol: EVENT_STYLE.boot.symbol, value: [190, 0.5, 'boot'] });
    expect(lane.data[1]).toMatchObject({ symbol: 'pin' });
  });
  it('honours hidden kinds', () => {
    const lane = series(historyOption(history(), LIGHT, new Set(['boot'])))[2]!;
    expect(lane.data).toHaveLength(1);
    expect(series(historyOption(history(), LIGHT, new Set(['boot', 'alert'])))[2]!.data).toEqual([]);
  });
  it('fixes the y axis to 0-100% with three gridlines', () => {
    const o = historyOption(history(), LIGHT) as { yAxis: { min?: number; max?: number; interval?: number }[] };
    expect(o.yAxis[0]).toMatchObject({ min: 0, max: 100, interval: 50 });
  });
  it('spans the history on a time axis with the right label format per range', () => {
    const o = historyOption(history({ from: 5, to: 99 }), DARK) as unknown as { xAxis: { min: number; max: number; axisLabel: { formatter: string } }[] };
    expect([o.xAxis[0]!.min, o.xAxis[0]!.max]).toEqual([5, 99]);
    expect(o.xAxis[0]!.axisLabel.formatter).toBe('{HH}:{mm}');
    expect((historyOption(history({ range: 'hour' }), DARK) as unknown as typeof o).xAxis[0]!.axisLabel.formatter).toBe('{HH}:{mm}');
    expect((historyOption(history({ range: 'week' }), DARK) as unknown as typeof o).xAxis[0]!.axisLabel.formatter).toBe('{d} {MMM}');
  });
  it('the tooltip formatter prints the time and each series percentage', () => {
    const at = new Date(2024, 0, 15, 9, 30).getTime();
    const o = historyOption(history(), LIGHT) as { tooltip: { formatter: (raw: unknown) => string } };
    expect(o.tooltip.formatter([])).toBe('');
    const html = o.tooltip.formatter([{ axisValue: at, marker: '*', seriesName: 'CPU', seriesType: 'line', value: [at, 42] }]);
    expect(html).toMatch(/09\D30/);
    expect(html).toContain('CPU');
    expect(html).toContain('<b>42%</b>');
    const week = historyOption(history({ range: 'week' }), LIGHT) as typeof o;
    expect(week.tooltip.formatter([{ axisValue: at, marker: '', seriesName: 'CPU', seriesType: 'line', value: [at, 1] }])).toMatch(/Jan/);
  });
  it('handles an empty history', () => {
    const cpu = series(historyOption(history({ points: [], events: [] }), LIGHT))[0]!;
    expect(cpu.data).toEqual([]);
    expect(series(historyOption(history({ points: [], events: [] }), LIGHT))[2]!.data).toEqual([]);
  });
});

describe('consumersOption', () => {
  const consumers = Array.from({ length: 8 }, (_, i) => ({ app: `app${i}`, cpuAvg: i + 1, memoryAvg: 1024 ** 3 }));
  it('keeps the top six, largest at the top of the bar chart', () => {
    const o = consumersOption(consumers, LIGHT) as { yAxis: { data: string[] } };
    expect(o.yAxis.data).toEqual(['app2', 'app3', 'app4', 'app5', 'app6', 'app7']);
    expect(series(consumersOption(consumers, LIGHT))[0]!.data).toEqual([3, 4, 5, 6, 7, 8]);
  });
  it('does not mutate its input and handles none', () => {
    const copy = [...consumers];
    consumersOption(consumers, LIGHT);
    expect(consumers).toEqual(copy);
    expect(series(consumersOption([], LIGHT))[0]!.data).toEqual([]);
  });
  it('the tooltip names the app with CPU and memory', () => {
    const o = consumersOption(consumers.slice(0, 2), LIGHT) as { tooltip: { formatter: (r: unknown) => string }; xAxis: { max: (v: { max: number }) => number } };
    expect(o.tooltip.formatter([{ dataIndex: 0 }])).toContain('app0');
    expect(o.tooltip.formatter([{ dataIndex: 0 }])).toContain('1 GB');
    expect(o.tooltip.formatter([])).toBe('');
    expect(o.tooltip.formatter([{ dataIndex: 9 }])).toBe('');
    expect(o.xAxis.max({ max: 100 })).toBeCloseTo(115);
  });
});

describe('diskTrendOption', () => {
  const day = 24 * 3600 * 1000;
  const trend = (forecast: DiskTrend['forecast'], points = [{ at: 0, free: 100 }, { at: day, free: 80 }]): DiskTrend => ({ disk: 'C:', total: 1000, points, forecast });

  it('adds a dashed forecast series only when the forecast is an object', () => {
    const withForecast = series(diskTrendOption(trend({ fullInDays: 10 }), LIGHT));
    expect(withForecast.map((s) => s.name)).toEqual(['Free space', 'Forecast (Full in ~10 days)']);
    expect(withForecast[1]!.lineStyle?.type).toBe('dashed');
    expect(withForecast[1]!.data).toEqual([[day, 80], [day + 10 * day, 0]]);
    for (const f of ['notFilling', 'notEnoughData'] as const) {
      expect(series(diskTrendOption(trend(f), LIGHT)).map((s) => s.name)).toEqual(['Free space']);
    }
  });
  it('has no forecast without points', () => {
    expect(series(diskTrendOption(trend({ fullInDays: 3 }, []), LIGHT)).map((s) => s.name)).toEqual(['Free space']);
  });
  it('the tooltip formats the free space', () => {
    const o = diskTrendOption(trend('notFilling'), LIGHT) as { tooltip: { formatter: (r: unknown) => string }; yAxis: { axisLabel: { formatter: (v: number) => string } } };
    expect(o.tooltip.formatter([])).toBe('');
    expect(o.tooltip.formatter([{ axisValue: day, seriesName: 'Free space', value: [day, 2048] }])).toContain('<b>2 KB</b> free');
    expect(o.yAxis.axisLabel.formatter(1024)).toBe('1 KB');
  });
});
