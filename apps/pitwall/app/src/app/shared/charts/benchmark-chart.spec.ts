import type { BenchmarkResult } from '../../core/api.types';
import { THEME_TOKENS } from '../theme-tokens';
import { benchmarkChart, latestPerRun } from './benchmark-chart';

const r = (modelId: string, build: string, at: number, genTps: number, ttftMs = 100): BenchmarkResult => ({ modelId, build, at, promptTps: 1000, genTps, ttftMs, contextTokens: 4096 });
const name = (id: string) => id.toUpperCase();

describe('latestPerRun', () => {
  it('keeps the newest result per model and build, fastest first, with the build trimmed to its name', () => {
    const rows = latestPerRun([r('a', 'cuda-13 · b1', 1, 10), r('a', 'cuda-13 · b1', 5, 30), r('a', 'vulkan · b1', 2, 20), r('b', 'cpu', 3, 40, 7)], name);
    expect(rows).toEqual([
      { label: 'B · cpu', genTps: 40, ttftMs: 7 },
      { label: 'A · cuda-13', genTps: 30, ttftMs: 100 },
      { label: 'A · vulkan', genTps: 20, ttftMs: 100 },
    ]);
  });

  it('is empty without results and does not let an older result replace a newer one', () => {
    expect(latestPerRun([], name)).toEqual([]);
    expect(latestPerRun([r('a', 'x', 9, 9), r('a', 'x', 1, 1)], name)[0]!.genTps).toBe(9);
  });
});

describe('benchmarkChart', () => {
  const rows = [
    { label: 'Fast', genTps: 120.44, ttftMs: 70.4 },
    { label: 'Slow', genTps: 30.26, ttftMs: 400 },
  ];
  const option = benchmarkChart(rows, THEME_TOKENS.light) as any;

  it('draws two bar plots that share the row labels, fastest on top (reversed for ECharts)', () => {
    expect(option.series.map((s: any) => s.name)).toEqual(['Generation', 'TTFT']);
    expect(option.yAxis[0].data).toEqual(['Slow', 'Fast']);
    expect(option.series[0].data).toEqual([30.26, 120.44]);
    expect(option.series[1].data).toEqual([400, 70.4]);
    expect(option.yAxis[0].axisLabel.show).toBe(true);
    expect(option.yAxis[1].axisLabel.show).toBe(false);
    expect(option.series[0].itemStyle.color).not.toBe(option.series[1].itemStyle.color);
  });

  it('formats the bar labels and the tooltip with units', () => {
    expect(option.series[0].label.formatter({ value: 120.44 })).toBe('120');
    expect(option.series[0].label.formatter({ value: 30.26 })).toBe('30.3');
    expect(option.series[1].label.formatter({ value: 70.4 })).toBe('70');
    const tip = option.tooltip.formatter([
      { name: 'Fast <b>', marker: '*', seriesName: 'Generation', value: 61.8 },
      { name: 'Fast <b>', marker: '#', seriesName: 'TTFT', value: 214.4 },
    ]);
    expect(tip).toContain('61.8 tok/s');
    expect(tip).toContain('214 ms');
    expect(tip).toContain('Fast b');
    expect(tip).not.toContain('<b>Fast');
    expect(option.tooltip.formatter({ name: 'Solo', marker: '', seriesName: 'Generation', value: 5 })).toContain('5.0 tok/s');
    expect(option.tooltip.formatter([])).toContain('<div');
  });

  it('follows the theme tokens and handles no rows', () => {
    const dark = benchmarkChart([], THEME_TOKENS.dark) as any;
    expect(dark.series[0].data).toEqual([]);
    expect(dark.tooltip.backgroundColor).toBe(THEME_TOKENS.dark.tooltipBg);
    expect(option.tooltip.backgroundColor).toBe(THEME_TOKENS.light.tooltipBg);
  });
});
