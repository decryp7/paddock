import { describe, expect, it } from 'vitest';
import type { ChartSpec } from '../generated/view';
import { chartSpecOf, chartWords, scatterOption } from './chart';

const c = (kind: ChartSpec['kind'], over: Partial<ChartSpec> = {}): ChartSpec => ({ kind, title: 'T', unit: '', categories: ['a', 'b'], series: [{ name: 'S', values: [1, 4] }], ...over });

describe('view chart', () => {
  it('maps line, area and bar to the generic spec', () => {
    for (const k of ['line', 'area', 'bar'] as const) expect(chartSpecOf(c(k))).toEqual({ kind: k, x: ['a', 'b'], series: [{ name: 'S', values: [1, 4] }] });
  });
  it('passes the unit on to the generic spec and the scatter axis, and into the words', () => {
    expect(chartSpecOf(c('line', { unit: 's' }))).toMatchObject({ unit: 's' });
    expect(chartSpecOf(c('pie', { unit: '%' }))).toMatchObject({ unit: '%' });
    expect(chartSpecOf(c('line'))).not.toHaveProperty('unit');
    expect(scatterOption(c('scatter', { unit: 'ms' }))['yAxis']).toEqual({ type: 'value' });
    expect(chartWords(c('line', { unit: 's' }))).toBe('S from 1 s to 4 s over 2 points, last 4 s');
  });
  it('colours scatter series from the tokens it is given, like the other kinds', () => {
    const o = scatterOption(c('scatter'), ['#123456']) as { series: { itemStyle?: { color: string } }[] };
    expect(o.series[0]?.itemStyle?.color).toBe('#123456');
    expect((scatterOption(c('scatter')) as { series: { itemStyle?: unknown }[] }).series[0]?.itemStyle).toBeUndefined();
  });
  it('draws a pie as a donut of its first series', () => {
    const s = chartSpecOf(c('pie', { series: [{ name: 'S', values: [1, 4] }, { name: 'Other', values: [2, 2] }] }));
    expect(s).toEqual({ kind: 'donut', x: ['a', 'b'], series: [{ name: 'S', values: [1, 4] }] });
  });
  it('has no generic spec for a scatter and builds its own option', () => {
    expect(chartSpecOf(c('scatter'))).toBeNull();
    const o = scatterOption(c('scatter')) as { series: { type: string; data: number[] }[]; legend: { show: boolean } };
    expect(o.series[0]).toMatchObject({ type: 'scatter', data: [1, 4] });
    expect(o.legend.show).toBe(false);
    expect((scatterOption(c('scatter', { series: [{ name: 'A', values: [1, 2] }, { name: 'B', values: [3, 4] }] })) as { legend: { show: boolean } }).legend.show).toBe(true);
  });
  it('describes a chart in words, and an empty one', () => {
    expect(chartWords(c('line'))).toBe('S from 1 to 4 over 2 points, last 4');
    expect(chartWords(c('line', { series: [] }))).toBe('No data.');
    expect(chartWords(c('line', { categories: [], series: [{ name: 'S', values: [] }] }))).toBe('No data.');
  });
});
