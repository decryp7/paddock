import type { EChartsCoreOption } from 'echarts/core';
import type { BenchmarkResult } from '../../core/api.types';
import { rate } from '../format';
import { seriesColor, type ThemeTokens } from '../theme-tokens';

/** Room for the row labels (model · build) left of the first plot. */
const LABEL_W = 200;
const FONT = "Inter, system-ui, -apple-system, 'Segoe UI', sans-serif";

export interface BenchRow {
  label: string;
  genTps: number;
  ttftMs: number;
}

/** The newest result per model and build, fastest generation first. */
export function latestPerRun(results: BenchmarkResult[], nameOf: (modelId: string) => string): BenchRow[] {
  const latest = new Map<string, BenchmarkResult>();
  for (const r of results) {
    const key = `${r.modelId}|${r.build}`;
    const seen = latest.get(key);
    if (!seen || r.at > seen.at) latest.set(key, r);
  }
  return [...latest.values()]
    .map((r) => ({ label: `${nameOf(r.modelId)} · ${r.build.split(' · ')[0]}`, genTps: r.genTps, ttftMs: r.ttftMs }))
    .sort((a, b) => b.genTps - a.genTps);
}

/**
 * Generation speed and time to first token per model and build, as two horizontal bar plots side by side that share
 * the row labels (two measures, two axes on two plots; never a dual axis).
 */
export function benchmarkChart(rows: BenchRow[], t: ThemeTokens): EChartsCoreOption {
  // ECharts draws the first category at the bottom: reverse so the fastest is on top.
  const ordered = [...rows].reverse();
  const labels = ordered.map((r) => r.label);
  const axisLine = { show: false };
  const category = (gridIndex: number, show: boolean) => ({
    type: 'category',
    gridIndex,
    data: labels,
    axisLine,
    axisTick: { show: false },
    axisLabel: { show, color: t.text2, fontSize: 11, width: LABEL_W - 16, overflow: 'truncate' },
  });
  const value = (gridIndex: number) => ({
    type: 'value',
    gridIndex,
    splitNumber: 3,
    axisLine,
    axisTick: { show: false },
    splitLine: { lineStyle: { color: t.grid } },
    axisLabel: { color: t.text3, fontSize: 10 },
  });
  const bar = (name: string, data: number[], color: string, gridIndex: number, fmt: (v: number) => string) => ({
    name,
    type: 'bar',
    xAxisIndex: gridIndex,
    yAxisIndex: gridIndex,
    data,
    barMaxWidth: 14,
    itemStyle: { color, borderRadius: [0, 4, 4, 0] },
    label: { show: true, position: 'right', color: t.text2, fontSize: 11, formatter: (p: { value: number }) => fmt(p.value) },
    emphasis: { disabled: true },
  });
  return {
    animationDuration: 300,
    textStyle: { fontFamily: FONT, color: t.text2 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow', shadowStyle: { color: `${t.text3}14` } },
      backgroundColor: t.tooltipBg,
      borderWidth: 0,
      textStyle: { color: t.tooltipText, fontSize: 12, fontFamily: FONT },
      extraCssText: 'border-radius:8px;',
      formatter: (params: unknown) => {
        const list = (Array.isArray(params) ? params : [params]) as { name: string; marker: string; seriesName: string; value: number }[];
        const head = list[0]?.name ?? '';
        const unit = (s: string, v: number) => (s === 'TTFT' ? `${Math.round(v)} ms` : `${rate(v)} tok/s`);
        return `<div style="opacity:.7;margin-bottom:4px">${head.replace(/[&<>"']/g, '')}</div>${list.map((p) => `${p.marker}${p.seriesName} <b>${unit(p.seriesName, p.value)}</b>`).join('<br>')}`;
      },
    },
    title: [
      { text: 'Generation · tok/s', left: LABEL_W, top: 0, textStyle: { fontSize: 11, fontWeight: 500, color: t.text2 } },
      { text: 'Time to first token · ms', left: '58%', top: 0, textStyle: { fontSize: 11, fontWeight: 500, color: t.text2 } },
    ],
    grid: [
      { left: LABEL_W, right: '48%', top: 26, bottom: 22 },
      { left: '58%', right: 40, top: 26, bottom: 22 },
    ],
    xAxis: [value(0), value(1)],
    yAxis: [category(0, true), category(1, false)],
    series: [
      bar('Generation', ordered.map((r) => r.genTps), seriesColor(t, 0), 0, (v) => rate(v)),
      bar('TTFT', ordered.map((r) => r.ttftMs), seriesColor(t, 1), 1, (v) => `${Math.round(v)}`),
    ],
  };
}
