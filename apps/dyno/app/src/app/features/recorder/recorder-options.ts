import type { EChartsCoreOption } from 'echarts/core';
import type { RecorderSample } from '../../core/api.types';
import { formatBytes } from '../../shared/format';
import type { ThemeTokens } from '../../shared/tokens';

const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif';
const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

/** The index of the sample with the highest CPU (the first one on a tie; 0 when there are none). */
export function peakIndex(samples: readonly RecorderSample[]): number {
  let best = 0;
  samples.forEach((s, i) => {
    if (s.cpuTotal > (samples[best]?.cpuTotal ?? -1)) best = i;
  });
  return best;
}

/** Clock time with seconds, `14:05:12` (24 h). */
export function clockSeconds(at: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }).format(at);
}

const mb = (bps: number) => bps / MIB;
const num = (v: number) => (v >= 100 ? v.toFixed(0) : v.toFixed(1));

/** One line the tooltip and the panel titles show for a sample: every metric with its unit. */
export function sampleReadout(s: RecorderSample): { cpu: string; memory: string; disk: string; net: string } {
  return {
    cpu: `${num(s.cpuTotal)}%`,
    memory: `${(s.memoryUsed / GIB).toFixed(1)} of ${(s.memoryTotal / GIB).toFixed(0)} GB`,
    disk: `read ${num(mb(s.diskReadBps))} · write ${num(mb(s.diskWriteBps))} MB/s`,
    net: `down ${num(mb(s.netRxBps))} · up ${num(mb(s.netTxBps))} MB/s`,
  };
}

/** The text a screen reader gets for the whole timeline. */
export function timelineSummary(samples: readonly RecorderSample[]): string {
  if (samples.length === 0) return 'No samples.';
  const peak = samples[peakIndex(samples)]!;
  const maxMem = Math.max(...samples.map((s) => s.memoryUsed));
  return (
    `Timeline of ${samples.length} one-second samples from ${clockSeconds(samples[0]!.at)} to ${clockSeconds(samples[samples.length - 1]!.at)}: ` +
    `CPU peaked at ${num(peak.cpuTotal)}% at ${clockSeconds(peak.at)}; memory used reached ${formatBytes(maxMem)} of ${formatBytes(peak.memoryTotal)}; ` +
    `disk and network rates are charted below. Use the left and right arrow keys to move the picked moment.`
  );
}

/** A round axis maximum at or above `v` (1, 2, 5 or 10 times a power of ten), so the half-way label is round too. */
export function niceMax(v: number): number {
  if (!(v > 1)) return 1;
  const pow = 10 ** Math.floor(Math.log10(v));
  const f = v / pow;
  return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * pow;
}

/** A picked stretch of the timeline: first and last sample index, both included. */
export interface SampleRange {
  from: number;
  to: number;
}

/**
 * The sample range a drag on the overview strip selects. `x0` and `x1` are pixel offsets (either order) inside a strip
 * `width` px wide that shows `count` samples. A drag shorter than a few samples is a click, not a range (null); the
 * result is clamped to the samples that exist.
 */
export function rangeFromDrag(x0: number, x1: number, width: number, count: number): SampleRange | null {
  if (!(width > 0) || count < 2) return null;
  const at = (x: number) => Math.min(count - 1, Math.max(0, Math.round((x / width) * (count - 1))));
  const a = at(Math.min(x0, x1));
  const b = at(Math.max(x0, x1));
  return b - a >= 3 ? { from: a, to: b } : null;
}

/** Moves the picked moment into the range when it lies outside it (so the line is always in view). */
export function momentInRange(moment: number, range: SampleRange | null): number {
  return range ? Math.min(range.to, Math.max(range.from, moment)) : moment;
}

/** The overview strip's CPU outline as SVG polyline points in a 0..100 by 0..100 box (100 is the top of the strip). */
export function overviewPoints(samples: readonly RecorderSample[], maxPoints = 120): string {
  const n = samples.length;
  if (n < 2) return '';
  const step = Math.max(1, Math.ceil(n / maxPoints));
  const pts: string[] = [];
  for (let i = 0; i < n; i += step) pts.push(`${((i / (n - 1)) * 100).toFixed(2)},${(100 - Math.min(100, Math.max(0, samples[i]!.cpuTotal))).toFixed(1)}`);
  pts.push(`100,${(100 - Math.min(100, Math.max(0, samples[n - 1]!.cpuTotal))).toFixed(1)}`);
  return pts.join(' ');
}

const PANEL_TOP = 6;
const TITLE_H = 26;
const PLOT_H = 62;
const GAP = 14;
const HEAT_H = 96;
/** The height the chart needs for its five panels and the time axis. */
export const TIMELINE_HEIGHT = PANEL_TOP + 4 * (TITLE_H + PLOT_H + GAP) + TITLE_H + HEAT_H + 30;

/** The heading of each panel: its title and unit, the legend keys it needs and the picked moment's value. `top` is in px. */
export interface PanelHeading {
  top: number;
  title: string;
  keys: { label: string; series: 'a' | 'b'; dashed: boolean }[];
  value: string;
}

/** The five panel headings (HTML over the chart: ECharts' title component is not loaded). */
export function panelHeadings(at: RecorderSample | undefined): PanelHeading[] {
  const r = at ? sampleReadout(at) : null;
  const y = (k: number) => PANEL_TOP + k * (TITLE_H + PLOT_H + GAP);
  return [
    { top: y(0), title: 'CPU total, %', keys: [], value: r?.cpu ?? '' },
    { top: y(1), title: 'Memory used, GB', keys: [], value: r?.memory ?? '' },
    { top: y(2), title: 'Disk, MB/s', keys: [{ label: 'read', series: 'a', dashed: false }, { label: 'write', series: 'b', dashed: true }], value: r?.disk ?? '' },
    { top: y(3), title: 'Network, MB/s', keys: [{ label: 'down', series: 'a', dashed: false }, { label: 'up', series: 'b', dashed: true }], value: r?.net ?? '' },
    { top: y(4), title: 'CPU per core, % (core number on the left)', keys: [], value: '' },
  ];
}

/**
 * The timeline as ONE chart so every panel shares the time axis and the hover crosshair: CPU total, memory, disk, network
 * and a per-core CPU heatmap. A transparent full-height bar per panel makes any point in a panel clickable; the picked
 * moment is a vertical line and the panel titles read out its values.
 */
export function timelineOption(samples: readonly RecorderSample[], moment: number, t: ThemeTokens, range: SampleRange | null = null): EChartsCoreOption {
  const n = samples.length;
  const cores = samples.reduce((m, s) => Math.max(m, s.cpuCores.length), 0);
  const totalGb = (samples[0]?.memoryTotal ?? 0) / GIB;
  const cpuColor = t.chart[0]!;
  const memColor = t.chart[1]!;
  const aColor = t.chart[2]!;
  const bColor = t.chart[4]!;
  const indices = samples.map((_, i) => String(i));
  const labelEvery = Math.max(1, Math.round(n / 10));

  const top = (k: number) => PANEL_TOP + k * (TITLE_H + PLOT_H + GAP);
  const grids = [0, 1, 2, 3].map((k) => ({ left: 52, right: 16, top: top(k) + TITLE_H, height: PLOT_H }));
  grids.push({ left: 52, right: 16, top: top(4) + TITLE_H, height: HEAT_H });

  const axisText = { color: t.text3, fontFamily: FONT, fontSize: 11 };
  const xAxis = grids.map((_, g) => ({
    type: 'category' as const,
    gridIndex: g,
    data: indices,
    boundaryGap: g === 4,
    axisLine: { lineStyle: { color: t.border } },
    axisTick: { show: false },
    axisLabel: g === 4 ? { ...axisText, interval: labelEvery - 1, formatter: (v: string) => clockSeconds(samples[Number(v)]?.at ?? 0).slice(0, 5) } : { show: false },
    splitLine: { show: false },
    axisPointer: { show: true, lineStyle: { color: t.borderStrong, width: 1 }, label: { show: false } },
  }));
  const value = (g: number, max: number | undefined, fmt: (v: number) => string, interval?: number) => ({
    type: 'value' as const,
    gridIndex: g,
    min: 0,
    ...(max !== undefined ? { max } : {}),
    ...(interval ? { interval } : { splitNumber: 2 }),
    axisLabel: { ...axisText, formatter: fmt },
    splitLine: { lineStyle: { color: t.chartGrid, type: 'dashed' as const } },
  });
  const rateMax = (a: (s: RecorderSample) => number, b: (s: RecorderSample) => number) =>
    niceMax(samples.reduce((m, s) => Math.max(m, mb(a(s)), mb(b(s))), 0));
  const diskMax = rateMax((s) => s.diskReadBps, (s) => s.diskWriteBps);
  const netMax = rateMax((s) => s.netRxBps, (s) => s.netTxBps);
  const yAxis = [
    value(0, 100, (v) => `${v}%`, 50),
    value(1, totalGb || undefined, (v) => `${Math.round(v)}`, totalGb ? totalGb / 2 : undefined),
    value(2, diskMax, (v) => `${Math.round(v)}`),
    value(3, netMax, (v) => `${Math.round(v)}`),
    { type: 'category' as const, gridIndex: 4, data: Array.from({ length: cores }, (_, i) => `Core ${i + 1}`), inverse: true, axisLine: { show: false }, axisTick: { show: false }, axisLabel: { ...axisText, hideOverlap: true, formatter: (v: string) => v.replace('Core ', '') }, splitLine: { show: false } },
  ];

  /** The invisible bar that makes a panel clickable anywhere, and carries the picked-moment line. */
  const pick = (g: number, height: number) => ({
    type: 'bar' as const,
    name: `pick-${g}`,
    xAxisIndex: g,
    yAxisIndex: g,
    silent: false,
    barWidth: '100%',
    barCategoryGap: '0%',
    itemStyle: { color: 'transparent' },
    emphasis: { disabled: true },
    tooltip: { show: false },
    data: indices.map(() => height),
    markLine: { silent: true, symbol: 'none', animation: false, label: { show: false }, lineStyle: { color: t.accent, width: 1.5, type: 'solid' as const }, data: [{ xAxis: Math.min(moment, n - 1) }] },
    z: 1,
  });
  const line = (name: string, g: number, data: number[], color: string, opts: { dashed?: boolean; area?: boolean } = {}) => ({
    type: 'line' as const,
    name,
    xAxisIndex: g,
    yAxisIndex: g,
    showSymbol: false,
    silent: true,
    data,
    lineStyle: { width: 1.5, color, type: opts.dashed ? ('dashed' as const) : ('solid' as const) },
    itemStyle: { color },
    ...(opts.area ? { areaStyle: { color, opacity: 0.12 } } : {}),
    z: 3,
  });

  return {
    animationDuration: 250,
    textStyle: { fontFamily: FONT },
    grid: grids,
    xAxis,
    yAxis,
    axisPointer: { link: [{ xAxisIndex: 'all' }] },
    // The brush strip sets the range; the axes zoom to it without any gesture of their own.
    ...(range ? { dataZoom: [{ type: 'inside' as const, xAxisIndex: [0, 1, 2, 3, 4], startValue: range.from, endValue: range.to, disabled: true }] } : {}),
    visualMap: { show: false, min: 0, max: 100, seriesIndex: 10, inRange: { color: [t.surface2, t.accent] } },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: FONT, fontSize: 12 },
      axisPointer: { type: 'line', lineStyle: { color: t.borderStrong } },
      formatter: (raw: unknown) => {
        const first = (raw as { dataIndex: number }[])[0];
        const s = first ? samples[first.dataIndex] : undefined;
        if (!s) return '';
        const r = sampleReadout(s);
        return `${clockSeconds(s.at)}<br/>CPU <b>${r.cpu}</b><br/>Memory <b>${r.memory}</b><br/>Disk <b>${r.disk}</b><br/>Network <b>${r.net}</b>`;
      },
    },
    series: [
      pick(0, 100),
      pick(1, totalGb || 1),
      pick(2, diskMax),
      pick(3, netMax),
      line('CPU total', 0, samples.map((s) => s.cpuTotal), cpuColor, { area: true }),
      line('Memory used', 1, samples.map((s) => s.memoryUsed / GIB), memColor, { area: true }),
      line('Disk read', 2, samples.map((s) => mb(s.diskReadBps)), aColor),
      line('Disk write', 2, samples.map((s) => mb(s.diskWriteBps)), bColor, { dashed: true }),
      line('Network down', 3, samples.map((s) => mb(s.netRxBps)), aColor),
      line('Network up', 3, samples.map((s) => mb(s.netTxBps)), bColor, { dashed: true }),
      {
        type: 'heatmap',
        name: 'CPU per core',
        xAxisIndex: 4,
        yAxisIndex: 4,
        data: samples.flatMap((s, x) => s.cpuCores.map((v, y) => [x, y, v])),
        progressive: 0,
        emphasis: { disabled: true },
        itemStyle: { borderWidth: 0 },
        markLine: { silent: true, symbol: 'none', animation: false, label: { show: false }, lineStyle: { color: t.accent, width: 1.5 }, data: [{ xAxis: Math.min(moment, n - 1) }] },
      },
    ],
  };
}
