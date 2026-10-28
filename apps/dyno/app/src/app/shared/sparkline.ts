import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/**
 * The SVG path of a line through `values` in a `width × 100` box (0 at the bottom), scaled to `max`. The box holds
 * `capacity` samples; fewer are drawn right-aligned, so a new line grows in from the right.
 */
export function sparkPath(values: readonly number[], max: number, width: number, capacity = values.length, min = 0): string {
  if (values.length === 0 || max <= min) return '';
  const slots = Math.max(capacity, values.length);
  const step = slots > 1 ? width / (slots - 1) : 0;
  const x0 = (slots - values.length) * step;
  return values
    .map((v, i) => {
      const y = 100 - ((Math.min(Math.max(v, min), max) - min) / (max - min)) * 100;
      return `${i === 0 ? 'M' : 'L'}${(x0 + i * step).toFixed(2)} ${y.toFixed(2)}`;
    })
    .join('');
}

/**
 * A zoomed scale for a slowly moving level (memory use): the data's own range with 15% padding, never narrower than
 * `minSpan` and kept inside 0..`ceiling`, so a flat 50% still draws a readable line instead of a slab.
 */
export function zoomRange(values: readonly number[], minSpan = 8, ceiling = 100): { min: number; max: number } {
  if (values.length === 0) return { min: 0, max: ceiling };
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const span = Math.max(minSpan, (hi - lo) * 1.3);
  const mid = (lo + hi) / 2;
  let min = mid - span / 2;
  let max = mid + span / 2;
  if (min < 0) [min, max] = [0, span];
  if (max > ceiling) [min, max] = [Math.max(0, ceiling - span), ceiling];
  return { min, max };
}

/** The scale for a set of series: the fixed `max` when given, else the highest value with 15% headroom. */
export function sparkMax(series: readonly (readonly number[])[], max: number | null): number {
  if (max !== null) return max;
  const top = Math.max(0, ...series.flat());
  return top > 0 ? top * 1.15 : 1;
}

/**
 * A tiny live chart: one or two series as lines with a soft fill under the first. Plain SVG, so it costs nothing
 * per tick and never shifts the layout (the box is fixed; the drawing stretches).
 */
@Component({
  selector: 'dy-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'img', '[attr.aria-label]': 'label()' },
  template: `
    <!-- conformance: svg-ok — sparkline --><svg [attr.viewBox]="'0 0 ' + width + ' 100'" preserveAspectRatio="none" aria-hidden="true">
      @if (fill() && area()) {
        <path class="area" [attr.d]="area()" [style.fill]="color()" />
      }
      <path class="line" [attr.d]="path()" [style.stroke]="color()" />
      @if (path2()) {
        <path class="line second" [attr.d]="path2()" [style.stroke]="color2()" />
      }
    </svg>
  `,
  styles: `
    :host { display: block; height: var(--spark-h, 36px); width: 100%; }
    svg { width: 100%; height: 100%; display: block; overflow: visible; }
    .line { fill: none; stroke-width: 1.5px; vector-effect: non-scaling-stroke; stroke-linejoin: round; stroke-linecap: round; }
    .second { stroke-dasharray: 3 2; }
    .area { opacity: 0.16; stroke: none; }
  `,
})
export class Sparkline {
  readonly values = input.required<readonly number[]>();
  readonly values2 = input<readonly number[] | null>(null);
  /** A fixed scale (e.g. 100 for percentages); `null` scales to the data. */
  readonly max = input<number | null>(null);
  readonly color = input('var(--text-3)');
  readonly color2 = input('var(--chart-2)');
  readonly fill = input(true);
  readonly label = input('');
  /** How many samples the box holds; fewer values are drawn right-aligned so the line grows in from the right. */
  readonly capacity = input(60);
  /** Zoom the scale to the data (for levels that barely move); the fixed `max` is then only the ceiling. */
  readonly zoom = input(false);
  protected readonly width = 100;

  private readonly range = computed(() =>
    this.zoom() ? zoomRange(this.values(), 8, this.max() ?? 100) : { min: 0, max: sparkMax([this.values(), this.values2() ?? []], this.max()) },
  );
  private readonly scale = computed(() => this.range().max);

  protected readonly path = computed(() => sparkPath(this.values(), this.scale(), this.width, this.capacity(), this.range().min));
  protected readonly path2 = computed(() => {
    const v = this.values2();
    return v ? sparkPath(v, this.scale(), this.width, this.capacity(), this.range().min) : '';
  });
  protected readonly area = computed(() => {
    const n = this.values().length;
    if (n < 2) return '';
    const slots = Math.max(this.capacity(), n);
    const x0 = ((slots - n) * this.width) / (slots - 1);
    return `${this.path()}L${this.width} 100L${x0.toFixed(2)} 100Z`;
  });
}
