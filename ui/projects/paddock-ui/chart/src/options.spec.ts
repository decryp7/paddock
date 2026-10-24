import type { PaddockChartTokens } from '@decryp7/paddock-ui';
import { describe, expect, it } from 'vitest';
import { chartOption, chartSummary, donutNarrowHeight, escapeHtml, seriesColor, shares, tooltipHtml, type PdChartSpec } from './options';

const tokens: PaddockChartTokens = {
  scheme: 'dark',
  fontFamily: 'system-ui',
  text: '#eceef1',
  textMuted: '#a0a7b2',
  textSubtle: '#8b93a0',
  surface1: '#15171b',
  surface2: '#1b1e23',
  surface3: '#22262c',
  hairline: 'rgba(255, 255, 255, 0.08)',
  borderStrong: 'rgba(255, 255, 255, 0.16)',
  accent: '#5b9bff',
  palette: ['#60aeeb', '#f3aa25', '#3dd39d', '#d68cc2', '#f07d3a', '#8fa3ff', '#e8dc5a', '#a2acb1'],
  divergingNeg: '#60aeeb',
  divergingMid: '#22262c',
  divergingPos: '#f3aa25',
  neutralBar: '#5e6775',
  success: '#3dd68c',
  warning: '#f2b84b',
  danger: '#ff6b6b',
  info: '#7db2e8',
};

const base: PdChartSpec = { kind: 'line', x: ['Mon', 'Tue', 'Wed'], series: [{ name: 'Requests', values: [3, null, 9] }], unit: 'req' };
const opt = (spec: PdChartSpec, animate = true) => chartOption(spec, tokens, { animate }) as Record<string, any>;

describe('chart options', () => {
  it('escapes what comes from data before it reaches an HTML tooltip', () => {
    expect(escapeHtml(`<img src=x onerror="a('b')">&`)).toBe('&lt;img src=x onerror=&quot;a(&#39;b&#39;)&quot;&gt;&amp;');
    const html = tooltipHtml('<b>Tue</b>', [{ color: 'red"', name: '<script>', value: '1 & 2' }]);
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<b>Tue');
    expect(html).toContain('&lt;script&gt;');
    expect(html).toContain('1 &amp; 2');
  });

  it('takes series colours from the palette in order, or from the token asked for', () => {
    expect(seriesColor(tokens, undefined, 0)).toBe('#60aeeb');
    expect(seriesColor(tokens, undefined, 9)).toBe('#f3aa25');
    expect(seriesColor(tokens, 2, 0)).toBe('#3dd39d');
    expect(seriesColor(tokens, -1, 0)).toBe('#a2acb1');
    expect([seriesColor(tokens, 'accent', 0), seriesColor(tokens, 'neutral', 0), seriesColor(tokens, 'success', 0), seriesColor(tokens, 'warning', 0), seriesColor(tokens, 'danger', 0)]).toEqual([
      '#5b9bff',
      '#5e6775',
      '#3dd68c',
      '#f2b84b',
      '#ff6b6b',
    ]);
  });

  it('draws a line chart with its gap kept, the unit on the axis and no legend for one series', () => {
    const o = opt(base);
    expect(o['series'][0]).toMatchObject({ type: 'line', data: [3, null, 9] });
    expect(o['yAxis']).toMatchObject({ type: 'value' });
    // The unit is in the chart's title, not floating on the axis next to the legend.
    expect(o['yAxis'].name).toBeUndefined();
    expect(o['yAxis'].min).toBeUndefined();
    expect(o['xAxis']).toMatchObject({ type: 'category', data: ['Mon', 'Tue', 'Wed'] });
    expect(o['legend'].show).toBe(false);
    expect(o['animation']).toBe(true);
  });

  it('adds a legend for two series, a zero baseline for bars and areas, and a shared stack', () => {
    const two: PdChartSpec = { ...base, kind: 'stackedBar', series: [...base.series, { name: 'Errors', values: [1, 1, 1], color: 'danger' }] };
    const o = opt(two);
    expect(o['legend']).toMatchObject({ show: true, data: ['Requests', 'Errors'] });
    expect(o['yAxis'].min).toBe(0);
    expect(o['series'].map((s: any) => [s.type, s.stack])).toEqual([['bar', 'total'], ['bar', 'total']]);
    expect(o['series'][1].color).toBe('#ff6b6b');
    expect(opt({ ...base, kind: 'bar' })['series'][0].stack).toBeUndefined();
    const area = opt({ ...base, kind: 'area' });
    expect(area['yAxis'].min).toBe(0);
    expect(area['series'][0].areaStyle.color.type).toBe('linear');
    expect(opt({ ...base, legend: true })['legend'].show).toBe(true);
  });

  it('rounds only the top segment of each stacked bar, never the ones underneath', () => {
    const o = opt({ kind: 'stackedBar', x: ['Mon', 'Tue', 'Wed'], series: [{ name: 'A', values: [3, 2, 0] }, { name: 'B', values: [1, 0, 0] }, { name: 'C', values: [null, null, 4] }] });
    const radius = (s: number, k: number) => o['series'][s].data[k].itemStyle.borderRadius;
    expect([radius(0, 0), radius(1, 0), radius(2, 0)]).toEqual([0, [4, 4, 0, 0], 0]);
    expect([radius(0, 1), radius(1, 1)]).toEqual([[4, 4, 0, 0], 0]);
    expect(radius(2, 2)).toEqual([4, 4, 0, 0]);
  });

  it('marks a value with no neighbour, so a lone point between gaps is not lost', () => {
    const o = opt({ kind: 'line', x: ['a', 'b', 'c', 'd', 'e'], series: [{ name: 'S', values: [null, 5, null, 3, 4] }] });
    const size = o['series'][0].symbolSize as (v: unknown, p: { dataIndex: number }) => number;
    expect([0, 1, 2, 3, 4].map((i) => size(null, { dataIndex: i }))).toEqual([0, 6, 0, 0, 0]);
    expect(o['series'][0].showSymbol).toBe(true);
  });

  it('isolates a series on hover from four series on, because colour alone does not tell four lines apart', () => {
    const many = (n: number) => opt({ kind: 'line', x: ['a', 'b'], series: Array.from({ length: n }, (_, i) => ({ name: `S${i}`, values: [1, 2] })) });
    expect(many(3)['series'][0].emphasis).toBeUndefined();
    expect(many(4)['series'].every((s: any) => s.emphasis?.focus === 'series')).toBe(true);
  });

  it('uses the accent for a lone series (the same blue in every chart) and the palette for several', () => {
    expect(opt(base)['series'][0].color).toBe('#5b9bff');
    const two = opt({ ...base, series: [{ name: 'A', values: [1, 2, 3] }, { name: 'B', values: [1, 2, 3] }] });
    expect(two['series'].map((s: any) => s.color)).toEqual(['#60aeeb', '#f3aa25']);
  });

  it('uses a time axis with value pairs for epoch x values, and zoom when asked', () => {
    const time = opt({ kind: 'line', x: [1_000, 2_000], series: [{ name: 'A', values: [1, 2] }], time: true, zoom: true, max: 10 });
    expect(time['xAxis'].type).toBe('time');
    expect(time['series'][0].data).toEqual([[1_000, 1], [2_000, 2]]);
    expect(time['yAxis'].max).toBe(10);
    expect(time['dataZoom']).toEqual([{ type: 'inside' }]);
    expect(time['grid'].bottom).toBeGreaterThan(8);
  });

  it('turns the animation off for reduced motion', () => {
    const o = opt(base, false);
    expect(o['animation']).toBe(false);
    expect(o['animationDuration']).toBe(0);
  });

  it('formats the axis, the category labels and the tooltip with the spec formatters', () => {
    const o = opt({ ...base, format: (v) => `${v}!`, formatX: (x) => `<${x}>` });
    expect(o['yAxis'].axisLabel.formatter(4)).toBe('4!');
    expect(o['xAxis'].axisLabel.formatter('Mon')).toBe('<Mon>');
    const tip = o['tooltip'].formatter([
      { axisValue: 'Mon', seriesName: 'Requests', value: 4, color: '#fff' },
      { axisValue: 'Mon', seriesName: 'Gap', value: [1, null], color: '#000' },
    ]);
    expect(tip).toContain('&lt;Mon&gt;');
    expect(tip).toContain('4! req');
    expect(tip).toContain('–');
    expect(o['tooltip'].formatter([])).toBe('<div style="font-weight:500;margin-bottom:4px"></div>');
    const timeTip = opt({ kind: 'line', x: [0], series: [{ name: 'A', values: [1] }], time: true })['tooltip'].formatter([{ axisValue: 0, seriesName: 'A', value: [0, 1], color: '#fff' }]);
    expect(timeTip).toContain('1970');
  });

  it('draws a donut with the total in the middle and every slice named with its value and share beside the ring', () => {
    const o = opt({ kind: 'donut', x: ['Disk', 'Memory', 'Cache'], series: [{ name: 'Use', values: [60, 30, 10] }], unit: 'GB' });
    expect(o['legend']).toMatchObject({ show: true, orient: 'vertical', data: ['Disk', 'Memory', 'Cache'] });
    expect(o['legend'].formatter('Disk')).toBe('Disk  60 GB  60%');
    expect(o['legend'].formatter('Nope')).toBe('Nope');
    expect(o['graphic'][0].style.text).toBe('100 GB');
    // One line, smaller when the total is long, so it stays inside the hole of the ring.
    expect(o['graphic'][0].style.font).toContain('16px');
    expect(opt({ kind: 'donut', x: ['A'], series: [{ name: 'S', values: [1234567] }], unit: 'req' })['graphic'][0].style.font).toContain('13px');
    expect(o['series'][0]).toMatchObject({ type: 'pie' });
    expect(o['series'][0].data.map((d: any) => [d.name, d.value])).toEqual([['Disk', 60], ['Memory', 30], ['Cache', 10]]);
    expect(o['tooltip'].formatter({ name: 'Disk', value: 60, color: '#60aeeb' })).toContain('60%');
    // The ring and the total are drawn around ONE centre, in pixels.
    expect(o['series'][0].center).toEqual([o['graphic'][0].x, o['graphic'][0].y]);
    // Wide: the list starts right of the ring; narrow: the list is under the ring, centred, and the ring is centred in the box.
    const [cx, radius] = [o['series'][0].center[0], o['series'][0].radius[1]];
    expect(o['legend'].left).toBeGreaterThan(cx + radius);
    const narrow = opt({ kind: 'donut', x: ['A', 'B'], series: [{ name: 'S', values: [1, 1] }] }, true) as Record<string, any>;
    const n = chartOption({ kind: 'donut', x: ['A', 'B'], series: [{ name: 'S', values: [1, 1] }] }, tokens, { animate: true, narrow: true, width: 320 }) as Record<string, any>;
    expect(n['legend']).toMatchObject({ left: 'center', bottom: 4 });
    expect(n['series'][0].center[0]).toBe(160);
    expect(n['series'][0].center[0] - n['series'][0].radius[1]).toBeGreaterThanOrEqual(0);
    expect(n['series'][0].center[1] + n['series'][0].radius[1]).toBeLessThan(donutNarrowHeight(2));
    expect(narrow['legend'].top).toBe('middle');
  });

  it('rounds shares so they add up to 100, and shows nothing for an empty ring', () => {
    expect(shares([42, 27, 14, 5])).toEqual([48, 31, 16, 5]);
    expect(shares([1, 1, 1])).toEqual([34, 33, 33]);
    expect(shares([0, 0])).toEqual([0, 0]);
    expect(shares([50, 50]).reduce((a, b) => a + b, 0)).toBe(100);
  });

  it('colours slices in an order that keeps neighbours apart, and merges more than five slices into one neutral "Other"', () => {
    const six = opt({ kind: 'donut', x: ['A', 'B', 'C', 'D', 'E', 'F'], series: [{ name: 'S', values: [1, 2, 3, 4, 5, 6] }] });
    const data = six['series'][0].data as { name: string; value: number; itemStyle: { color: string } }[];
    expect(data).toHaveLength(5);
    expect(data[4]).toMatchObject({ name: 'Other', value: 11 });
    expect(data[4]?.itemStyle.color).toBe(tokens.neutralBar);
    const colours = data.slice(0, 4).map((d) => d.itemStyle.color);
    expect(new Set(colours).size).toBe(4);
    expect(opt({ kind: 'donut', x: ['A'], series: [], legend: false })['series'][0].data[0].value).toBe(0);
    expect(opt({ kind: 'donut', x: ['A', 'B'], series: [{ name: 'S', values: [0, 0] }] })['legend'].formatter('A')).toContain('0%');
  });

  it('summarises the chart in words: range and latest per series, or the shares of a donut', () => {
    expect(chartSummary(base)).toBe('Requests: 3 req to 9 req, latest 9 req');
    expect(chartSummary({ ...base, series: [{ name: 'None', values: [null, null, null] }] })).toBe('None: no data');
    expect(chartSummary({ ...base, series: [], kind: 'bar' })).toBe('');
    expect(chartSummary({ kind: 'donut', x: ['Disk', 'Memory'], series: [{ name: 'Use', values: [75, 25] }] })).toBe('Disk: 75 (75%); Memory: 25 (25%)');
    expect(chartSummary({ kind: 'donut', x: ['A', 'B', 'C'], series: [{ name: 'S', values: [1, 1, 1] }] })).toBe('A: 1 (34%); B: 1 (33%); C: 1 (33%)');
    expect(chartSummary({ kind: 'donut', x: ['Disk'], series: [{ name: 'Use', values: [0] }] })).toBe('Disk: 0');
    expect(chartSummary({ kind: 'donut', x: ['A', 'B', 'C', 'D', 'E', 'F'], series: [{ name: 'S', values: [1, 1, 1, 1, 1, 1] }] })).toContain('Other: 2 (33%)');
    expect(chartSummary({ kind: 'donut', x: ['A'], series: [] })).toBe('A: 0');
  });
});
