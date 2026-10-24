import type { PaddockChartTokens } from '@decryp7/paddock-ui';
import { describe, expect, it } from 'vitest';
import { chartOption, chartSummary, escapeHtml, seriesColor, tooltipHtml, type PdChartSpec } from './options';

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
    expect(o['yAxis']).toMatchObject({ type: 'value', name: 'req' });
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

  it('draws a donut with a legend, slice colours from the palette and shares in the tooltip', () => {
    const o = opt({ kind: 'donut', x: ['Disk', 'Memory'], series: [{ name: 'Use', values: [75, 25] }], unit: 'GB' });
    expect(o['legend']).toMatchObject({ show: true, data: ['Disk', 'Memory'] });
    expect(o['series'][0]).toMatchObject({ type: 'pie' });
    expect(o['series'][0].data.map((d: any) => [d.name, d.value, d.itemStyle.color])).toEqual([['Disk', 75, '#60aeeb'], ['Memory', 25, '#f3aa25']]);
    expect(o['tooltip'].formatter({ name: 'Disk', value: 75, color: '#60aeeb', percent: 75.4 })).toContain('75%');
    expect(opt({ kind: 'donut', x: ['A'], series: [], legend: false })['series'][0].data[0].value).toBe(0);
  });

  it('summarises the chart in words: range and latest per series, or the shares of a donut', () => {
    expect(chartSummary(base)).toBe('Requests: 3 req to 9 req, latest 9 req');
    expect(chartSummary({ ...base, series: [{ name: 'None', values: [null, null, null] }] })).toBe('None: no data');
    expect(chartSummary({ ...base, series: [], kind: 'bar' })).toBe('');
    expect(chartSummary({ kind: 'donut', x: ['Disk', 'Memory'], series: [{ name: 'Use', values: [75, 25] }] })).toBe('Disk: 75 (75%); Memory: 25 (25%)');
    expect(chartSummary({ kind: 'donut', x: ['Disk'], series: [{ name: 'Use', values: [0] }] })).toBe('Disk: 0');
    expect(chartSummary({ kind: 'donut', x: ['A'], series: [] })).toBe('A: 0');
  });
});
