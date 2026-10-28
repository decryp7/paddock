import type { JobDay } from '../../core/api.types';
import type { ChartTokens } from '../../core/theme.service';
import { diskOption, durationOption, escapeHtml, jobsPerDayOption, loadOption, queueOption, sumJobDays, tooltipHtml } from './chart-options';

const t: ChartTokens = {
  theme: 'light',
  font: 'sans',
  text: '#111',
  text2: '#222',
  text3: '#333',
  surface: '#fff',
  grid: '#eee',
  axis: '#ddd',
  series: ['#0000ff', '#ff8800', '#00aa00', '#aaaa00'],
  muted: '#999',
  faint: '#ccc',
  success: '#0a0',
  info: '#00a',
  warning: '#a80',
  danger: '#a00',
  accent: '#55f',
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Opt = any;

const DAY = 86_400_000;
const day = (i: number, over: Partial<JobDay> = {}): JobDay => ({
  start: i * DAY,
  success: 0,
  failure: 0,
  cancelled: 0,
  unknown: 0,
  avgQueuedMs: null,
  ...over,
});

describe('escapeHtml / tooltipHtml', () => {
  it('escapes markup characters', () => {
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe('&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;');
  });
  it('escapes every part of a tooltip', () => {
    const html = tooltipHtml('<b>t</b>', [{ color: 'red"', label: '<i>l</i>', value: '1&2' }]);
    expect(html).toContain('&lt;b&gt;t&lt;/b&gt;');
    expect(html).toContain('&lt;i&gt;l&lt;/i&gt;');
    expect(html).toContain('1&amp;2');
    expect(html).not.toContain('<i>');
  });
});

describe('sumJobDays', () => {
  it('adds counts per day, averages queue waits and sorts', () => {
    const out = sumJobDays([
      [day(2, { success: 1, avgQueuedMs: 1000 }), day(1, { failure: 2 })],
      [day(2, { success: 3, cancelled: 1, unknown: 2, avgQueuedMs: 2001 })],
    ]);
    expect(out.map((d) => d.start)).toEqual([DAY, 2 * DAY]);
    expect(out[0]).toEqual(day(1, { failure: 2 }));
    expect(out[1]).toEqual(day(2, { success: 4, cancelled: 1, unknown: 2, avgQueuedMs: 1501 }));
  });
  it('is empty for no input', () => {
    expect(sumJobDays([])).toEqual([]);
  });
});

describe('jobsPerDayOption', () => {
  it('stacks only the outcomes that occur and rounds the top bar', () => {
    const days = [day(0, { success: 2, failure: 1 }), day(1, { success: 1 })];
    const o = jobsPerDayOption(days, t, false) as Opt;
    expect(o['series'].map((s: Opt) => s.name)).toEqual(['Succeeded', 'Failed']);
    expect(o['series'][0].itemStyle.color).toBe(t.series[0]);
    expect(o['series'][1].itemStyle.color).toBe(t.series[1]);
    expect(o['series'][1].data[0].itemStyle).toEqual({ borderRadius: [4, 4, 0, 0] });
    expect(o['series'][0].data[1].itemStyle).toEqual({ borderRadius: [4, 4, 0, 0] });
    expect(o['series'][0].data[0].itemStyle).toBeUndefined();
    expect(o['dataZoom']).toEqual([]);
    expect(o['grid'].bottom).toBe(8);
  });
  it('shows an empty success series when nothing ran and a zoom slider when asked', () => {
    const o = jobsPerDayOption([day(0)], t, true) as Opt;
    expect(o['series']).toHaveLength(1);
    expect(o['dataZoom']).toHaveLength(2);
    expect(o['grid'].bottom).toBe(28);
  });
  it('labels hourly buckets with the hour', () => {
    const o = jobsPerDayOption([day(0, { success: 1 })], t, false, true) as Opt;
    expect(o['xAxis'].data[0]).toMatch(/\d{2}[:.]\d{2}/);
    expect(o['tooltip'].formatter([{ dataIndex: 0 }])).toMatch(/\d{2}[:.]\d{2} · 1 job/);
  });
  it('formats the tooltip and axis', () => {
    const days = [day(0, { success: 1 }), ...Array.from({ length: 15 }, (_, i) => day(i + 1, { success: 2, failure: 1 }))];
    const o = jobsPerDayOption(days, t, false) as Opt;
    const html = o['tooltip'].formatter([{ dataIndex: 0 }]);
    expect(html).toContain('1 job');
    expect(html).toContain('Succeeded');
    expect(o['tooltip'].formatter([{ dataIndex: 1 }])).toContain('3 jobs');
    expect(o['tooltip'].formatter([{ dataIndex: 99 }])).toBe('');
    expect(o['tooltip'].formatter([])).toContain('1 job');
    expect(o['xAxis'].data).toHaveLength(16);
    expect(o['yAxis'].axisLabel.formatter(3)).toBe('3');
    const short = jobsPerDayOption([day(0, { success: 1 })], t, false) as Opt;
    expect(short['xAxis'].data).toHaveLength(1);
  });
});

describe('durationOption', () => {
  const series = [
    { name: 'a', points: [[0, 1000], [DAY, 0], [2 * DAY, 60_000]] as [number, number][] },
    { name: 'b', points: [[0, 5000]] as [number, number][] },
  ];
  it('builds a line per runner without zero points and shows the legend for several', () => {
    const o = durationOption(series, t, 24 * 7) as Opt;
    expect(o['series']).toHaveLength(2);
    expect(o['series'][0].data).toEqual([[0, 1000], [2 * DAY, 60_000]]);
    expect(o['series'][1].lineStyle.color).toBe(t.series[1]);
    expect(o['legend'].show).toBe(true);
    expect(o['grid'].top).toBe(36);
    const single = durationOption([series[0]!], t, 24) as Opt;
    expect(single['legend'].show).toBe(false);
    expect(single['grid'].top).toBe(12);
  });
  it('formats tooltip and axes', () => {
    const o = durationOption(series, t, 24 * 7) as Opt;
    const html = o['tooltip'].formatter([{ seriesName: 'a', value: [0, 61_000], color: 'red' }]);
    expect(html).toContain('1m 01s');
    expect(o['tooltip'].formatter([])).toContain('min-width');
    expect(o['yAxis'].axisLabel.formatter(120_000)).toBe('2 min');
    expect(typeof o['xAxis'].axisLabel.formatter(0)).toBe('string');
    const hourly = durationOption(series, t, 24) as Opt;
    expect(hourly['xAxis'].axisLabel.formatter(0)).not.toBe(o['xAxis'].axisLabel.formatter(0));
  });
});

describe('queueOption', () => {
  it('plots waits at mid-day with gaps for missing data', () => {
    const days = [day(0, { avgQueuedMs: 5000 }), day(1)];
    const o = queueOption(days, t) as Opt;
    expect(o['series'][0].data).toEqual([[DAY / 2, 5000], [DAY + DAY / 2, null]]);
    expect(o['series'][0].showSymbol).toBe(true);
    expect(o['legend']).toEqual({ show: false });
    const tip = o['tooltip'].formatter;
    expect(tip([{ value: [0, 5000] }])).toContain('5s');
    expect(tip([{ value: [0, null] }])).toContain('no data');
    expect(tip([])).toBe('');
    expect(o['yAxis'].axisLabel.formatter(5000)).toBe('5s');
    const many = queueOption(Array.from({ length: 12 }, (_, i) => day(i)), t) as Opt;
    expect(many['series'][0].showSymbol).toBe(false);
  });
});

describe('diskOption', () => {
  const series = [
    { name: 'a', points: [[0, 1024], [1000, 2048]] as [number, number][] },
    { name: 'b', points: [] as [number, number][] },
  ];
  it('marks cleanups on the first series only', () => {
    const o = diskOption(series, [500, 800], t, 24) as Opt;
    expect(o['series'][0].markLine.data).toEqual([{ xAxis: 500 }, { xAxis: 800 }]);
    expect(o['series'][1].markLine).toBeUndefined();
    expect(o['legend'].show).toBe(true);
    const none = diskOption([series[0]!], [], t, 24) as Opt;
    expect(none['series'][0].markLine).toBeUndefined();
    expect(none['legend'].show).toBe(false);
    expect(none['grid'].top).toBe(12);
  });
  it('formats tooltip and value axis', () => {
    const o = diskOption(series, [], t, 24) as Opt;
    expect(o['tooltip'].formatter([{ seriesName: 'a', value: [0, 2048], color: 'red' }])).toContain('2.0 KB');
    expect(o['tooltip'].formatter([])).toContain('min-width');
    const long = diskOption(series, [], t, 24 * 7) as Opt;
    expect(long['tooltip'].formatter([{ seriesName: 'a', value: [0, 1], color: 'red' }])).toContain('1 B');
    expect(o['yAxis'].axisLabel.formatter(2048)).toBe('2 KB');
  });
});

describe('loadOption', () => {
  const pts = [
    { at: 0, runners: 300, machine: 800 },
    { at: 1000, runners: 900, machine: 500 }, // runners above the machine total (sampling skew)
  ];
  it('stacks the runners over the rest of the machine and never exceeds the total', () => {
    const o = loadOption(pts, (v) => `${v / 10}%`, 1000, t, 24) as Opt;
    expect(o['series'].map((x: Opt) => x.name)).toEqual(['Runners', 'Rest of machine']);
    expect(o['series'][0].data).toEqual([[0, 300], [1000, 500]]);
    expect(o['series'][1].data).toEqual([[0, 500], [1000, 0]]);
    expect(o['series'][0].stack).toBe(o['series'][1].stack);
    expect(o['series'][0].lineStyle.color).toBe(t.series[0]);
    expect(o['series'][1].lineStyle.color).toBe(t.muted);
    expect(o['yAxis'].max).toBe(1000);
    // The ticks divide the maximum evenly, so the top tick is the maximum and no two labels crowd each other ("32 GB" over "28 GB").
    expect(o['yAxis'].interval).toBe(250);
    expect(o['legend'].show).toBe(true);
  });
  it('has a free axis without a maximum and a tooltip with all three numbers', () => {
    const o = loadOption(pts, (v) => `${v}u`, undefined, t, 24 * 7) as Opt;
    expect('max' in o['yAxis']).toBe(false);
    const html = o['tooltip'].formatter([
      { seriesName: 'Runners', value: [0, 300], color: 'x' },
      { seriesName: 'Rest of machine', value: [0, 500], color: 'y' },
    ]);
    expect(html).toContain('300u');
    expect(html).toContain('500u');
    expect(html).toContain('800u');
    expect(o['tooltip'].formatter([])).toContain('Machine');
    expect(o['yAxis'].axisLabel.formatter(5)).toBe('5u');
  });
});
