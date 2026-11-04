import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { BenchMetric, BenchTracePoint } from '../../core/api.types';
import { formatMetric } from '../../shared/format';
import { chartGeometry, clock, plural, sweepPoints, traceReadout } from './bench-model';

const W = 560;
const H = 150;
const PAD = 12;

const AXES_CSS = `
    :host { display: block; }
    .chart { margin: 0; display: grid; gap: var(--sp-1); }
    .plot { display: grid; grid-template-columns: auto minmax(0, 1fr); column-gap: var(--sp-2); }
    .yaxis { display: flex; flex-direction: column; justify-content: space-between; align-items: flex-end; color: var(--text-3); font-size: var(--fs-xs); padding: var(--sp-1) 0; white-space: nowrap; }
    .xaxis { grid-column: 2; display: flex; justify-content: space-between; color: var(--text-3); font-size: var(--fs-xs); padding-top: var(--sp-1); }
    svg { display: block; width: 100%; background: var(--surface-inset); border-radius: var(--r-md); }
    .grid { stroke: var(--border-subtle); stroke-width: 1; }
    .line { fill: none; stroke: var(--accent); stroke-width: 2; vector-effect: non-scaling-stroke; }
    figcaption { color: var(--text-2); font-size: var(--fs-sm); }
    .faint { color: var(--text-3); }
    .num { font-variant-numeric: tabular-nums; }
`;

/** The throughput of a sustained run, one point a second, with its axes and the clock and temperature beside the newest point in words. */
@Component({
  selector: 'dy-sustained-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <figure class="chart">
      <div class="plot">
        <div class="yaxis num" aria-hidden="true"><span>{{ top() }} {{ unit() }}</span><span>0</span></div>
        <svg class="s150" [attr.viewBox]="'0 0 ' + w + ' ' + h" role="img" [attr.aria-label]="summary()" preserveAspectRatio="none">
          <line class="grid" [attr.x1]="pad" [attr.x2]="w - pad" [attr.y1]="h - pad" [attr.y2]="h - pad" />
          <line class="grid" [attr.x1]="pad" [attr.x2]="w - pad" [attr.y1]="pad" [attr.y2]="pad" />
          @if (geo().points) {
            <polyline class="line" [attr.points]="geo().points" />
          }
        </svg>
        <div class="xaxis num" aria-hidden="true">
          @for (t of ticks(); track $index) { <span>{{ t }}</span> }
        </div>
      </div>
      <figcaption>
        <span class="num">{{ summary() }}</span>
        @if (readout(); as r) { <span class="num faint"> · Now {{ r }}</span> }
      </figcaption>
    </figure>
  `,
  styles: AXES_CSS + '    .s150 { height: 150px; }',
})
export class SustainedChart {
  readonly trace = input.required<BenchTracePoint[]>();
  readonly unit = input('GFLOPS');
  protected readonly w = W;
  protected readonly h = H;
  protected readonly pad = PAD;
  protected readonly geo = computed(() => chartGeometry(this.trace().map((p) => p.throughput), W, H, PAD));
  protected readonly readout = computed(() => traceReadout(this.trace().at(-1)));
  protected readonly top = computed(() => formatMetric(this.geo().max));
  /** Elapsed time at the start, the middle and the end, each as m:ss. */
  protected readonly ticks = computed(() => {
    const n = this.trace().length;
    return [clock(0), clock(n / 2), clock(n)];
  });
  protected readonly summary = computed(() => {
    const t = this.trace();
    const last = t.at(-1);
    return last ? `${plural(t.length, 'second')} measured (${clock(t.length)}), now ${formatMetric(last.throughput)} ${this.unit()}, highest ${formatMetric(this.geo().max)} ${this.unit()}` : 'No points yet';
  });
}

/** Memory latency against the size of the memory walked, on a log scale, so the cache levels show as steps. */
@Component({
  selector: 'dy-sweep-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (points().length > 1) {
      <figure class="chart">
        <div class="plot">
          <div class="yaxis num" aria-hidden="true"><span>{{ top() }} ns</span><span>{{ bottom() }} ns</span></div>
          <svg class="s130" [attr.viewBox]="'0 0 ' + w + ' ' + h" role="img" [attr.aria-label]="summary()" preserveAspectRatio="none">
            <line class="grid" [attr.x1]="pad" [attr.x2]="w - pad" [attr.y1]="h - pad" [attr.y2]="h - pad" />
            <polyline class="line" [attr.points]="geo().points" />
          </svg>
          <div class="xaxis num" aria-hidden="true"><span>{{ first() }}</span><span>{{ middle() }}</span><span>{{ last() }}</span></div>
        </div>
        <figcaption class="num">{{ summary() }} (log scale)</figcaption>
      </figure>
    }
  `,
  styles: AXES_CSS + '    .s130 { height: 130px; }',
})
export class SweepChart {
  readonly metrics = input.required<BenchMetric[]>();
  protected readonly w = W;
  protected readonly h = 130;
  protected readonly pad = PAD;
  protected readonly points = computed(() => sweepPoints(this.metrics()));
  protected readonly geo = computed(() => chartGeometry(this.points().map((p) => p.ns), W, this.h, PAD, true));
  protected readonly first = computed(() => this.points()[0]?.label ?? '');
  protected readonly middle = computed(() => this.points()[Math.floor(this.points().length / 2)]?.label ?? '');
  protected readonly last = computed(() => this.points().at(-1)?.label ?? '');
  protected readonly top = computed(() => formatMetric(Math.max(...this.points().map((p) => p.ns))));
  protected readonly bottom = computed(() => formatMetric(Math.min(...this.points().map((p) => p.ns))));
  protected readonly summary = computed(() => {
    const p = this.points();
    const first = p[0];
    const last = p.at(-1);
    return first && last ? `Latency rises from ${formatMetric(first.ns)} ns at ${first.label} to ${formatMetric(last.ns)} ns at ${last.label}` : '';
  });
}
