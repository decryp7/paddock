import { ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, input, signal } from '@angular/core';
import { readPaddockTokens } from '@decryp7/paddock-ui';
import { PdChart, seriesColor, type PdChartSpec } from '@decryp7/paddock-ui/chart';
import type { ChartSpec } from '../generated/view';

const unitOf = (c: ChartSpec): { unit?: string } => (c.unit ? { unit: c.unit } : {});

/** The `pd-chart` spec of a view chart: line, area and bar map to themselves, a pie to the donut (the first series, one slice per category). `scatter` has no generic spec: see {@link scatterOption}. */
export function chartSpecOf(c: ChartSpec): PdChartSpec | null {
  if (c.kind === 'scatter') return null;
  if (c.kind === 'pie') {
    return { kind: 'donut', x: c.categories, series: c.series.slice(0, 1).map((s) => ({ name: s.name, values: s.values })), ...unitOf(c) };
  }
  return { kind: c.kind, x: c.categories, series: c.series.map((s) => ({ name: s.name, values: s.values })), ...unitOf(c) };
}

/**
 * An ECharts option for a scatter chart (points at category positions, one series each). `colors` are the series colours from the
 * design tokens (one series: the accent, like the line and the bar; several: the palette in order); without them the theme's own apply.
 */
export function scatterOption(c: ChartSpec, colors: readonly string[] = []): Record<string, unknown> {
  return {
    grid: { left: 48, right: 16, top: 32, bottom: 32 },
    legend: { show: c.series.length > 1, left: 12 },
    tooltip: { trigger: 'item' },
    xAxis: { type: 'category', data: c.categories, boundaryGap: true },
    yAxis: { type: 'value', axisLabel: c.unit ? { formatter: `{value} ${c.unit}` } : {} },
    series: c.series.map((s, i) => ({ name: s.name, type: 'scatter', symbolSize: 8, data: s.values, ...(colors[i] ? { itemStyle: { color: colors[i] } } : {}) })),
  };
}

/** A words-only description of a chart for assistive technology: each series' range and last value. */
export function chartWords(c: ChartSpec): string {
  if (c.series.length === 0 || c.categories.length === 0) return $localize`:@@pd.view.chart.no-data:No data.`;
  return c.series
    .map((s) => {
      const nums = s.values;
      const lo = Math.min(...nums);
      const hi = Math.max(...nums);
      const last = nums[nums.length - 1];
      const [a, b, z] = [lo, hi, last].map((v) => (c.unit ? `${v} ${c.unit}` : String(v)));
      return $localize`:@@pd.view.chart.words:${s.name}:name: from ${a}:lo: to ${b}:hi: over ${c.categories.length}:n: points, last ${z}:last:`;
    })
    .join('; ');
}

/**
 * A `Chart` node over the family's `pd-chart` (Apache ECharts, themed from the design tokens, with an accessible name and a text
 * summary): line, area, bar and pie (as the donut) use its generic spec; scatter draws through its `option` input. The chart's
 * title is shown above it; the series colours come from the palette, never from the plugin.
 */
@Component({
  selector: 'pd-view-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdChart],
  template: `<pd-chart [label]="name()" [spec]="spec()" [option]="option()" [summary]="words()" [height]="280" />`,
  styles: `:host { display: block; min-width: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }`,
})
export class PdViewChart {
  readonly chart = input.required<ChartSpec>();
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef);
  /** Counts appearance changes (theme, accent, contrast are attributes of the root), so the scatter colours are read again. */
  private readonly appearance = signal(0);
  protected readonly name = computed(() => this.chart().title || $localize`:@@pd.view.chart.name:Chart`);
  protected readonly spec = computed(() => chartSpecOf(this.chart()));
  protected readonly option = computed(() => {
    const c = this.chart();
    if (c.kind !== 'scatter') return null;
    this.appearance();
    const tokens = readPaddockTokens(this.element.nativeElement);
    return scatterOption(c, c.series.map((_, i) => seriesColor(tokens, undefined, i, c.series.length === 1)));
  });
  protected readonly words = computed(() => chartWords(this.chart()));

  constructor() {
    if (typeof MutationObserver === 'undefined') return;
    const watch = new MutationObserver(() => this.appearance.update((n) => n + 1));
    watch.observe(document.documentElement, { attributes: true });
    inject(DestroyRef).onDestroy(() => watch.disconnect());
  }
}
