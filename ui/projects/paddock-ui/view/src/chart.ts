import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdChart, type PdChartSpec } from '@decryp7/paddock-ui/chart';
import type { ChartSpec } from '../generated/view';

/** The `pd-chart` spec of a view chart: line, area and bar map to themselves, a pie to the donut (the first series, one slice per category). `scatter` has no generic spec: see {@link scatterOption}. */
export function chartSpecOf(c: ChartSpec): PdChartSpec | null {
  if (c.kind === 'scatter') return null;
  if (c.kind === 'pie') {
    return { kind: 'donut', x: c.categories, series: c.series.slice(0, 1).map((s) => ({ name: s.name, values: s.values })) };
  }
  return { kind: c.kind, x: c.categories, series: c.series.map((s) => ({ name: s.name, values: s.values })) };
}

/** An ECharts option for a scatter chart (points at category positions, one series each); the paddock theme colours it. */
export function scatterOption(c: ChartSpec): Record<string, unknown> {
  return {
    grid: { left: 48, right: 16, top: 32, bottom: 32 },
    legend: { show: c.series.length > 1, left: 12 },
    tooltip: { trigger: 'item' },
    xAxis: { type: 'category', data: c.categories, boundaryGap: true },
    yAxis: { type: 'value' },
    series: c.series.map((s) => ({ name: s.name, type: 'scatter', symbolSize: 8, data: s.values })),
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
      return $localize`:@@pd.view.chart.words:${s.name}:name: from ${lo}:lo: to ${hi}:hi: over ${c.categories.length}:n: points, last ${last}:last:`;
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
  styles: `:host { display: block; min-width: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); }`,
})
export class PdViewChart {
  readonly chart = input.required<ChartSpec>();
  protected readonly name = computed(() => this.chart().title || $localize`:@@pd.view.chart.name:Chart`);
  protected readonly spec = computed(() => chartSpecOf(this.chart()));
  protected readonly option = computed(() => (this.chart().kind === 'scatter' ? scatterOption(this.chart()) : null));
  protected readonly words = computed(() => chartWords(this.chart()));
}
