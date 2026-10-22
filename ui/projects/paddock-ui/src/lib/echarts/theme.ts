import { type PaddockChartTokens, sequentialRamp, withAlpha } from './tokens';

/** Gradient definition ECharts accepts without importing its classes (top 28% alpha to 0 at the baseline). */
export interface AreaGradient {
  type: 'linear';
  x: number;
  y: number;
  x2: number;
  y2: number;
  colorStops: { offset: number; color: string }[];
}

export function areaGradient(color: string, topAlpha = 0.28): AreaGradient {
  return {
    type: 'linear',
    x: 0,
    y: 0,
    x2: 0,
    y2: 1,
    colorStops: [
      { offset: 0, color: withAlpha(color, topAlpha) },
      { offset: 1, color: withAlpha(color, 0) },
    ],
  };
}

/** Event marker shapes: circle, triangle, diamond, square. Meaning is carried by shape plus a toggling legend, not colour. */
export const EVENT_SHAPES = ['circle', 'triangle', 'diamond', 'rect'] as const;
export type EventShape = (typeof EVENT_SHAPES)[number];

/**
 * The ECharts theme (guide section 7) built from tokens: system font, 11 px axis labels in the subtle colour,
 * no axis lines except the baseline, hairline dashed horizontal gridlines, 1.5 px lines without symbols and
 * without connecting gaps, bars from zero with a 4 px top radius, an interactive top-left legend, surface-3
 * crosshair tooltips, 300 ms draw animation on first draw only.
 */
export function createEchartsTheme(t: PaddockChartTokens): Record<string, unknown> {
  const axisLabel = { color: t.textSubtle, fontSize: 11, fontFamily: t.fontFamily };
  const baseline = { show: true, lineStyle: { color: t.borderStrong, width: 1 } };
  const grid = { show: true, lineStyle: { color: t.hairline, width: 1, type: [2, 4] } };
  const axis = {
    axisLine: { show: false },
    axisTick: { show: false },
    axisLabel,
    splitLine: { show: false },
    splitArea: { show: false },
    nameTextStyle: axisLabel,
  };
  return {
    darkMode: t.scheme === 'dark',
    color: t.palette,
    backgroundColor: 'transparent',
    textStyle: { fontFamily: t.fontFamily, fontSize: 11, color: t.textMuted },
    animation: true,
    animationDuration: 300,
    animationEasing: 'cubicOut',
    // Live updates do not animate between ticks.
    animationDurationUpdate: 0,
    title: { textStyle: { color: t.text, fontFamily: t.fontFamily, fontSize: 13, fontWeight: 600 }, subtextStyle: { color: t.textMuted, fontSize: 12 } },
    grid: { left: 8, right: 8, top: 32, bottom: 8, containLabel: true },
    categoryAxis: { ...axis, axisLine: baseline, splitLine: { show: false } },
    timeAxis: { ...axis, axisLine: baseline, splitLine: { show: false } },
    valueAxis: { ...axis, splitLine: grid },
    logAxis: { ...axis, splitLine: grid },
    line: {
      showSymbol: false,
      symbol: 'circle',
      symbolSize: 4,
      sampling: 'lttb',
      connectNulls: false,
      smooth: false,
      lineStyle: { width: 1.5 },
      emphasis: { focus: 'series', lineStyle: { width: 2 } },
    },
    bar: { barCategoryGap: '25%', itemStyle: { borderRadius: [4, 4, 0, 0], color: t.neutralBar }, emphasis: { itemStyle: { opacity: 0.85 } } },
    scatter: { symbolSize: 10, itemStyle: { borderColor: t.surface1, borderWidth: 1 } },
    legend: {
      left: 0,
      top: 0,
      icon: 'roundRect',
      itemWidth: 10,
      itemHeight: 10,
      itemGap: 12,
      textStyle: { color: t.textMuted, fontSize: 12, fontFamily: t.fontFamily },
      inactiveColor: t.textSubtle,
    },
    tooltip: {
      trigger: 'axis',
      confine: true,
      backgroundColor: t.surface3,
      borderColor: t.hairline,
      borderWidth: 1,
      padding: [8, 12],
      textStyle: { color: t.text, fontSize: 12, fontFamily: t.fontFamily },
      extraCssText: 'border-radius:10px;box-shadow:0 8px 24px rgba(0,0,0,.25);font-variant-numeric:tabular-nums;',
      axisPointer: { type: 'cross', lineStyle: { color: t.borderStrong, width: 1, type: 'solid' }, crossStyle: { color: t.borderStrong }, label: { show: false } },
    },
    markLine: { symbol: 'none', lineStyle: { color: t.textSubtle, width: 1, type: [4, 3] }, label: { color: t.textMuted, fontSize: 11, position: 'insideEndTop' } },
    visualMap: { color: [...sequentialRamp(t)].reverse(), textStyle: { color: t.textMuted, fontSize: 11 } },
    toolbox: { iconStyle: { borderColor: t.textMuted } },
    // Brush-to-zoom instead of slider chrome: pair it with a "Reset zoom" chip in the page.
    brush: { brushStyle: { color: withAlpha(t.accent, 0.12), borderColor: t.accent } },
    dataZoom: { show: false },
  };
}

export interface ChartEvent {
  /** Milliseconds since epoch (a time axis) or a category value. */
  at: number | string;
  kind: string;
  label?: string;
}

export interface EventLaneOptions {
  /** Maps an event kind to its marker shape (shape-coded, never colour only). Unknown kinds use a circle. */
  shapes: Record<string, EventShape>;
  /** Index of the x axis the lane aligns with. Default 0. */
  xAxisIndex?: number;
  /** Space below the plot reserved for the lane, in px. Default 14 (the guide's lane). */
  laneHeight?: number;
  /** Where the plot ends (px from the bottom of the chart). Default 8. */
  bottom?: number;
}

/**
 * An option fragment that puts event markers on a dedicated lane under the plot instead of vertical lines across it.
 * Merge into the chart option: `grid`, `xAxis`, `yAxis` and `series` are arrays; append them to the chart's own.
 * `gridIndex` is the index the lane grid will have in the merged option.
 */
export function eventLaneFragment(events: readonly ChartEvent[], t: PaddockChartTokens, gridIndex: number, opts: EventLaneOptions) {
  const lane = opts.laneHeight ?? 14;
  const bottom = opts.bottom ?? 8;
  return {
    plotBottom: bottom + lane + 8,
    grid: { left: 8, right: 8, bottom, height: lane, containLabel: false, show: false },
    xAxis: { type: 'time', gridIndex, show: false, axisPointer: { show: false } },
    yAxis: { type: 'value', gridIndex, show: false, min: 0, max: 1 },
    series: {
      type: 'scatter',
      name: 'Events',
      xAxisIndex: gridIndex,
      yAxisIndex: gridIndex,
      symbolSize: 9,
      itemStyle: { color: t.textMuted, borderColor: t.surface1, borderWidth: 1 },
      data: events.map((e) => ({ value: [e.at, 0.5], name: e.label ?? e.kind, symbol: opts.shapes[e.kind] ?? 'circle' })),
      tooltip: { trigger: 'item', formatter: (p: { name: string }) => p.name },
      z: 5,
    },
  };
}

/** Minimal structural type of the `echarts` namespace this package needs: `registerTheme` only. */
export interface EchartsRegistry {
  registerTheme(name: string, theme: object): void;
}

/** Theme names registered by {@link registerPaddockTheme}. */
export const PADDOCK_THEME_LIGHT = 'paddock-light';
export const PADDOCK_THEME_DARK = 'paddock-dark';

/**
 * Registers the paddock theme for the tokens' scheme and returns the name to pass to `echarts.init(el, name)`.
 * Call again on a scheme or accent change (tokens re-read) and `setTheme`/re-init the chart; data need not reload.
 */
export function registerPaddockTheme(echarts: EchartsRegistry, tokens: PaddockChartTokens): string {
  const name = tokens.scheme === 'light' ? PADDOCK_THEME_LIGHT : PADDOCK_THEME_DARK;
  echarts.registerTheme(name, createEchartsTheme(tokens));
  return name;
}
