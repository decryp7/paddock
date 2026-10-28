import type { HistoryPoint, PitwallEvent } from '../../core/api.types';
import { demoHistory, demoUsageSeries } from '../../core/mock-data';
import { THEME_TOKENS } from '../theme-tokens';
import { MAX_BARS, bucketKeyed, eventMark, foldKeyed, latencyChart, memoryChart, stackedRequestsChart, throughputChart, timeLabel, usageChart } from './chart-options';

const t = THEME_TOKENS.dark;
const NOW = new Date(2026, 9, 22, 14, 30).getTime();
const h = demoHistory('1h', NOW);

type Opt = Record<string, unknown> & { series: Record<string, unknown>[] };
const ev = (kind: PitwallEvent['kind'], reason: PitwallEvent['reason'], at = NOW - 1000): PitwallEvent => ({ id: 1, at, kind, severity: 'info', message: 'm <b>', modelId: null, appId: null, reason });

describe('chart options', () => {
  it('does not animate for a user who prefers reduced motion (and so a documentation shot is the same every run)', () => {
    const real = window.matchMedia;
    const stub = (reduce: boolean) =>
      (window.matchMedia = ((q: string) => ({ matches: reduce && q.includes('prefers-reduced-motion'), media: q }) as MediaQueryList) as typeof window.matchMedia);
    try {
      stub(true);
      expect((memoryChart({ points: h.points, events: h.events, range: '1h', stepMs: h.stepMs, zoom: true }, t) as unknown as Opt)['animation']).toBe(false);
      stub(false);
      expect((memoryChart({ points: h.points, events: h.events, range: '1h', stepMs: h.stepMs, zoom: true }, t) as unknown as Opt)['animation']).toBe(true);
    } finally {
      window.matchMedia = real;
    }
  });

  it('labels time by range', () => {
    const at = new Date(2026, 9, 22, 9, 5).getTime();
    expect(timeLabel('1h')(at)).toBe('09:05');
    expect(timeLabel('24h')(at)).toBe('09:05');
    expect(timeLabel('7d')(at)).toBe('22 Oct');
    expect(timeLabel('30d')(at)).toBe('22 Oct');
  });

  it('marks loads, releases by reason and restarts', () => {
    expect(eventMark(ev('modelLoaded', null), t)).toMatchObject({ glyph: '▲', shape: 'triangle', color: t.text2 });
    expect(eventMark(ev('modelUnloaded', 'pressure'), t)).toMatchObject({ shape: 'diamond', color: t.bad });
    expect(eventMark(ev('modelUnloaded', 'failed'), t)!.color).toBe(t.bad);
    expect(eventMark(ev('modelUnloaded', 'manual'), t)).toMatchObject({ shape: 'circle', color: t.text2 });
    expect(eventMark(ev('contextReleased', 'idle'), t)!.color).toBe(t.text2);
    expect(eventMark(ev('contextReleased', 'idle'), t)!.label).toBe('Released (idle)');
    expect(eventMark(ev('modelUnloaded', null), t)!.label).toBe('Released (manual)');
    expect(eventMark(ev('modelUnloaded', 'evicted'), t)!.label).toBe('Released (made room)');
    expect(eventMark(ev('engineRestarted', null), t)).toMatchObject({ glyph: '■', shape: 'rect' });
    expect(eventMark(ev('download', null), t)).toBeNull();
  });

  it('draws memory with one axis, event marks and an escaping tooltip', () => {
    const o = memoryChart({ points: h.points, events: [...h.events, ev('download', null)], range: '1h', stepMs: h.stepMs, zoom: true }, t) as unknown as Opt;
    expect(o.series.map((s) => s['name'])).toEqual(['RAM used', 'VRAM used', 'Pitwall', 'Events']);
    // The events sit on their own lane (a second grid), not as lines through the data.
    expect(o.series.at(-1)!['name']).toBe('Events');
    expect((o['grid'] as unknown[]).length).toBe(2);
    const marks = o.series.at(-1)!['data'] as { symbol: string; name: string }[];
    expect(marks.length).toBe(h.events.length);
    expect(marks.every((m) => ['triangle', 'circle', 'diamond', 'rect'].includes(m.symbol))).toBe(true);
    const laneTip = (o.series.at(-1)!['tooltip'] as { formatter: (p: { name: string }) => string }).formatter({ name: '<b>x</b>' });
    expect(laneTip).toBe('&lt;b&gt;x&lt;/b&gt;');
    expect(o.series.slice(0, 3).map((s) => (s['lineStyle'] as { color: string }).color)).toEqual([...t.history]);
    expect((o['dataZoom'] as unknown[]).length).toBe(2);
    const tooltip = o['tooltip'] as { formatter: (p: unknown) => string };
    const at = h.events[0]!.at;
    const html = tooltip.formatter([{ marker: '•', seriesName: 'RAM used', value: [at + h.stepMs / 2, 8 * 1024 ** 3] }, { marker: '•', seriesName: 'VRAM used', value: [at, null] }]);
    expect(html).toContain('8 GB');
    expect(html).not.toContain('VRAM used');
    const withEvent = memoryChart({ points: h.points, events: [ev('modelLoaded', null, at)], range: '7d', stepMs: h.stepMs }, t) as unknown as Opt;
    expect((withEvent['tooltip'] as { formatter: (p: unknown) => string }).formatter({ marker: '', seriesName: 'RAM used', value: [at, 1] })).toContain('m &lt;b&gt;');
    expect((withEvent['dataZoom'] as unknown[]).length).toBe(1);
    const empty = memoryChart({ points: [], events: [], range: '1h', stepMs: 1 }, t) as unknown as Opt;
    expect(empty.series[0]!['data']).toEqual([]);
    expect((empty['tooltip'] as { formatter: (p: unknown) => string }).formatter([])).toContain('<div');
  });

  it('formats the zoom slider labels', () => {
    const o = memoryChart({ points: h.points, events: [], range: '1h', stepMs: h.stepMs, zoom: true }, t) as unknown as Opt;
    const slider = (o['dataZoom'] as { labelFormatter?: (v: number) => string }[])[1]!;
    expect(slider.labelFormatter!(new Date(2026, 0, 1, 7, 3).getTime())).toBe('07:03');
    const yLabel = (o['yAxis'] as { axisLabel: { formatter: (v: number) => string } }[])[0]!.axisLabel.formatter(16 * 1024 ** 3);
    expect(yLabel).toBe('16 GB');
  });

  it('splits throughput into two plots with gaps for idle steps', () => {
    const pts: HistoryPoint[] = [{ ...h.points[0]!, genTps: 0, promptTps: 2500 }, h.points[1]!];
    const o = throughputChart(pts, '24h', t) as unknown as Opt;
    expect((o['grid'] as unknown[]).length).toBe(2);
    expect((o.series[0]!['data'] as unknown[][])[0]![1]).toBeNull();
    const fmt = (o['tooltip'] as { formatter: (p: unknown) => string }).formatter([{ marker: '', seriesName: 'Generation', value: [1, 42.66] }]);
    expect(fmt).toContain('42.7 tok/s');
    const yAxes = o['yAxis'] as { axisLabel: { formatter: (v: number) => string } }[];
    expect(yAxes[0]!.axisLabel.formatter(60)).toBe('60');
    expect(yAxes[1]!.axisLabel.formatter(2500)).toBe('2.5k');
    expect(yAxes[1]!.axisLabel.formatter(500)).toBe('500');
  });

  it('shows latency percentiles in ms and s', () => {
    const o = latencyChart(h.points, '1h', t) as unknown as Opt;
    expect(o.series.map((s) => s['name'])).toEqual(['p50', 'p95']);
    const y = (o['yAxis'] as { axisLabel: { formatter: (v: number) => string } }).axisLabel.formatter;
    expect(y(250)).toBe('250 ms');
    expect(y(2500)).toBe('2.5 s');
  });

  it('folds extra series into Other and buckets dense bars', () => {
    const many = Array.from({ length: 8 }, (_, i) => ({ key: `k${i}`, values: [i, i] }));
    const folded = foldKeyed(many);
    expect(folded.length).toBe(5);
    expect(folded[4]).toEqual({ key: 'Other', values: [0 + 1 + 2 + 3, 0 + 1 + 2 + 3] });
    const three = many.slice(0, 3);
    expect(foldKeyed(three)).toBe(three);
    const at = Array.from({ length: 120 }, (_, i) => i);
    const b = bucketKeyed(at, [{ key: 'a', values: at.map(() => 1) }]);
    expect(b.at.length).toBeLessThanOrEqual(MAX_BARS);
    expect(b.series[0]!.values.reduce((x, y) => x + y, 0)).toBe(120);
    expect(b.at[0]).toBe(2);
    const small = bucketKeyed([1, 2], []);
    expect(small.at).toEqual([1, 2]);
  });

  it('stacks requests per key with names and fixed colours', () => {
    const o = stackedRequestsChart(h.points, h.requestsByApp, '1h', t, (k) => k.toUpperCase()) as unknown as Opt;
    expect(o.series.map((s) => s['name'])).toEqual(['DEBRIEF', 'SPOTTER', 'NIGHTLY-REPORT', 'PITCREW', 'DYNO']);
    expect((o.series[0]!['itemStyle'] as { color: string }).color).toBe(t.series[0]);
    expect((o.series[0]!['data'] as unknown[]).length).toBeLessThanOrEqual(MAX_BARS);
    const fmt = (o['tooltip'] as { formatter: (p: unknown) => string }).formatter([{ marker: '', seriesName: 'Debrief', value: [1, 3.4] }]);
    expect(fmt).toContain('<b>3</b>');
    const y = (o['yAxis'] as { axisLabel: { formatter: (v: number) => string } }).axisLabel.formatter;
    expect(y(1500)).toBe('1.5k');
    expect(y(15)).toBe('15');
    const other = stackedRequestsChart([], Array.from({ length: 7 }, (_, i) => ({ key: `m${i}`, values: [i] })), '7d', t) as unknown as Opt;
    expect(other.series[4]!['name']).toBe('Other');
  });

  describe('usageChart', () => {
    const NOW2 = new Date(2026, 9, 22, 14, 30).getTime();
    const series = demoUsageSeries('24h', NOW2, null);
    const one = demoUsageSeries('7d', NOW2, 'spotter');
    const names = (o: Opt) => o.series.map((x) => x['name']);

    it('stacks tokens per app, one fixed colour each, with a readable legend', () => {
      const o = usageChart(series, 'tokens', 'app', t, (k) => k.toUpperCase()) as unknown as Opt;
      expect(names(o)).toEqual(series.apps.map((a) => a.appId.toUpperCase()));
      expect(o.series.every((x) => x['type'] === 'bar' && x['stack'] === 'tokens')).toBe(true);
      expect(o.series.map((x) => (x['itemStyle'] as { color: string }).color)).toEqual(series.apps.map((_, i) => t.series[Math.min(i, t.series.length - 1)]));
      expect((o.series[0]!['data'] as unknown[]).length).toBe(24);
      const tip = (o['tooltip'] as { formatter: (p: unknown) => string }).formatter;
      expect(tip([{ marker: '', seriesName: 'A', value: [0, 12_345] }])).toContain('12k tokens');
    });

    it('folds the apps beyond the busiest into Other', () => {
      const withOther = { ...series, apps: [...series.apps, { ...series.apps[0]!, appId: 'other' }] };
      expect(names(usageChart(withOther, 'requests', 'app', t) as unknown as Opt)).toContain('Other');
    });

    it('splits one app into prompt and completion tokens', () => {
      const o = usageChart(one, 'tokens', 'kind', t) as unknown as Opt;
      expect(names(o)).toEqual(['Prompt', 'Completion']);
      expect((o.series[0]!['data'] as unknown[]).length).toBe(168);
    });

    it('splits requests into successful and failed ones, errors in the status colour and named in words', () => {
      const o = usageChart(one, 'requests', 'kind', t) as unknown as Opt;
      expect(names(o)).toEqual(['Successful', 'Errors']);
      expect((o.series[1]!['itemStyle'] as { color: string }).color).toBe(t.bad);
      const first = (o.series[0]!['data'] as [number, number][])[0]![1];
      expect(first).toBe(one.total[0]!.requests - one.total[0]!.errors);
      const tip = (o['tooltip'] as { formatter: (p: unknown) => string }).formatter;
      expect(tip([{ marker: '', seriesName: 'Errors', value: [0, 3] }])).toContain('3 requests');
    });

    it('draws peak memory as a level, not a sum, in bytes', () => {
      const o = usageChart(one, 'memory', 'kind', t) as unknown as Opt;
      expect(names(o)).toEqual(['Peak memory']);
      expect(o.series[0]!['type']).toBe('line');
      expect(o.series[0]!['step']).toBe('end');
      expect(o.series[0]).toHaveProperty('areaStyle');
      const multi = usageChart(series, 'memory', 'app', t) as unknown as Opt;
      expect(multi.series.every((x) => x['type'] === 'line' && !('areaStyle' in x))).toBe(true);
      const tip = (multi['tooltip'] as { formatter: (p: unknown) => string }).formatter;
      expect(tip([{ marker: '', seriesName: 'A', value: [0, 3 * 1024 ** 3] }])).toContain('3 GB');
      const axis = o['yAxis'] as { axisLabel: { formatter: (v: number) => string } };
      expect(axis.axisLabel.formatter(2 * 1024 ** 3)).toBe('2 GB');
    });

    it('labels daily buckets by date and gives a legend row of room for many apps', () => {
      const month = demoUsageSeries('30d', NOW2, null);
      const o = usageChart(month, 'tokens', 'app', t) as unknown as Opt;
      expect((o['xAxis'] as { axisLabel: { formatter: (v: number) => string } }).axisLabel.formatter(NOW2)).toBe('22 Oct');
      expect((o['grid'] as { bottom: number }).bottom).toBe(64);
      expect((usageChart(one, 'tokens', 'kind', t) as unknown as { grid: { bottom: number } }).grid.bottom).toBe(44);
    });
  });
});
