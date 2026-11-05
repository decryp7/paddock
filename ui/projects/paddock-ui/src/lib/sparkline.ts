import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { formatNumber } from '@decryp7/paddock-ui/format';

export interface SparklineGeometry {
  line: string;
  area: string;
  end: { x: number; y: number } | null;
}

/** Optional geometry settings; all off by default, so a plain sparkline is unchanged. */
export interface SparklineOptions {
  /** How many samples the box holds; fewer values are drawn right-aligned, so a new line grows in from the right. */
  capacity?: number;
  /** Clamp every value into `min`..`max` instead of letting it leave the box. */
  clamp?: boolean;
}

/** How a sparkline is scaled: `fit` to the data, `zero` from 0 up, or `zoom` to the first series' own range. */
export type SparklineScaleMode = 'fit' | 'zero' | 'zoom';

/** The scale of a sparkline: a fixed `min`/`max`, or `undefined` for a side fitted to the data. */
export interface SparklineScale {
  min?: number;
  max?: number;
}

/**
 * A zoomed scale for a slowly moving level (memory use): the data's own range with 15% padding, never narrower than
 * `minSpan` and kept inside 0..`ceiling`, so a flat 50% still draws a readable line instead of a slab.
 */
export function zoomRange(values: readonly number[], minSpan = 8, ceiling = 100): { min: number; max: number } {
  const finite = values.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return { min: 0, max: ceiling };
  const lo = Math.min(...finite);
  const hi = Math.max(...finite);
  const span = Math.max(minSpan, (hi - lo) * 1.3);
  const mid = (lo + hi) / 2;
  let min = mid - span / 2;
  let max = mid + span / 2;
  if (min < 0) [min, max] = [0, span];
  if (max > ceiling) [min, max] = [Math.max(0, ceiling - span), ceiling];
  return { min, max };
}

/** The top of a scale from zero: the fixed `max` when given, else the highest value of every series plus 15% headroom (1 when all are zero). */
export function headroomMax(series: readonly (readonly number[])[], max?: number): number {
  if (max !== undefined) return max;
  const top = Math.max(0, ...series.flat().filter((v) => Number.isFinite(v)));
  return top > 0 ? top * 1.15 : 1;
}

/**
 * The scale a sparkline draws with. `zoom` follows the first series' range (`max` is the ceiling, 100 when omitted); `zero` starts
 * at 0 and ends at `max` or the peak of both series plus headroom; `fit` fits both series to one range.
 */
export function sparklineScale(values: readonly number[], values2: readonly number[] | null | undefined, mode: SparklineScaleMode, max?: number): SparklineScale {
  // `max` is the fixed top of `zero` and the ceiling of `zoom`; `fit` always follows the data.
  if (mode === 'zoom') return zoomRange(values, 8, max ?? 100);
  if (mode === 'zero') return { min: 0, max: headroomMax([values, values2 ?? []], max) };
  const all = [...values, ...(values2 ?? [])].filter((v) => Number.isFinite(v));
  if (all.length === 0) return {};
  return { min: Math.min(...all), max: Math.max(...all) };
}

/** Pure geometry of a sparkline: points scaled into `width` x `height` with `pad` px around (so the end dot is not clipped). */
export function sparklineGeometry(
  values: readonly number[],
  width: number,
  height: number,
  pad = 3,
  min?: number,
  max?: number,
  options: SparklineOptions = {},
): SparklineGeometry {
  const finite = values.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return { line: '', area: '', end: null };
  const lo = min ?? Math.min(...finite);
  const hi = max ?? Math.max(...finite);
  const span = hi - lo;
  const slots = Math.max(options.capacity !== undefined && Number.isFinite(options.capacity) ? options.capacity : values.length, values.length);
  const step = slots > 1 ? (width - 2 * pad) / (slots - 1) : 0;
  const x0 = pad + (slots - values.length) * step;
  const level = (v: number): number => (options.clamp ? Math.min(Math.max(v, lo), hi) : v);
  const pts = values.map((v, i) => (Number.isFinite(v) ? { x: x0 + i * step, y: span <= 0 ? height / 2 : pad + (1 - (level(v) - lo) / span) * (height - 2 * pad) } : null));
  // Gaps are honest: a missing value breaks the line.
  let line = '';
  let pen = false;
  for (const p of pts) {
    if (!p) {
      pen = false;
      continue;
    }
    line += `${pen ? 'L' : 'M'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`;
    pen = true;
  }
  const valid = pts.filter((p): p is { x: number; y: number } => p !== null);
  const first = valid[0] as { x: number; y: number };
  const last = valid[valid.length - 1] as { x: number; y: number };
  // One point has no width to fill.
  const area = valid.length < 2 ? '' : `${line}L${last.x.toFixed(1)} ${height}L${first.x.toFixed(1)} ${height}Z`;
  return { line, area, end: pts[pts.length - 1] ?? null };
}

/**
 * Tiny inline-SVG trend: 1.5 px line, optional area at 14% alpha, 4 px end dot. 24-32 px high, no axes.
 *
 * Options that are off by default (a plain sparkline is unchanged): a `values2` second line (dashed), `capacity` (a short series is
 * right-aligned, so it grows in from the right), a `scale` of `zero` or `zoom`, `pad`, `dot` and `color`/`color2`.
 */
@Component({
  selector: 'pd-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[style.color]': 'color()' },
  template: `
    <svg [attr.viewBox]="'0 0 ' + viewWidth() + ' ' + viewHeight()" [attr.height]="viewHeight()" preserveAspectRatio="none" role="img" [attr.aria-label]="summary()">
      @if (area() && geometry().area) {
        <path [attr.d]="geometry().area" class="pd-area" />
      }
      <path [attr.d]="geometry().line" class="pd-line" fill="none" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      @if (second().line) {
        <path [attr.d]="second().line" class="pd-line pd-line2" [style.stroke]="color2()" fill="none" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      }
    </svg>
    @if (dot() && geometry().end; as e) {
      <span class="pd-dot" [style.left.%]="(e.x / viewWidth()) * 100" [style.top.px]="e.y"></span>
    }
  `,
  styles: `
    :host { position: relative; display: block; width: 100%; color: var(--pd-accent); }
    svg { display: block; width: 100%; overflow: visible; }
    .pd-line { stroke: currentColor; }
    .pd-line2 { stroke-dasharray: 3 2; }
    .pd-area { fill: currentColor; opacity: 0.14; }
    /* A 4 px dot as an element: the SVG stretches to the tile width, which would squash a circle. */
    .pd-dot { position: absolute; width: 4px; height: 4px; margin: -2px 0 0 -2px; border-radius: 50%; background: currentColor; }
  `,
})
export class SuSparkline {
  readonly values = input.required<readonly number[]>();
  /** A second series on the same scale, drawn as a dashed line. */
  readonly values2 = input<readonly number[] | null>(null);
  /** Accessible name, e.g. "CPU, last hour". The summary (latest and peak) is appended. */
  readonly label = input<string>($localize`:@@pd.ui.sparkline:Trend`);
  /** The name of the second series in the summary. */
  readonly label2 = input<string>($localize`:@@pd.ui.sparkline.second:Second line`);
  /** Formats the latest and peak values in the accessible name (a unit, a rounding); omitted: a rounded number. */
  readonly formatValue = input<(v: number) => string>(formatNumber);
  readonly height = input<number>(32);
  readonly area = input<boolean>(false);
  /**
   * `fit` (default): the data's own range (`max` is ignored). `zero`: from 0 to `max` (or the peak plus 15% headroom), shared by
   * rows of sparklines. `zoom`: the first series' range inside 0..`max` (100 when omitted), for levels that barely move. `zero` and
   * `zoom` clamp values into the scale.
   */
  readonly scale = input<SparklineScaleMode>('fit');
  readonly max = input<number | undefined>(undefined);
  /** How many samples the box holds; fewer values are drawn right-aligned. Omitted: the series fills the box. */
  readonly capacity = input<number | undefined>(undefined);
  /** Px around the drawing, so the end dot is not clipped; 0 for a line that runs edge to edge. */
  readonly pad = input<number>(3);
  /** The 4 px dot on the latest value. */
  readonly dot = input<boolean>(true);
  /** Line colour (any CSS colour or `var(--...)`); omitted: the accent. */
  readonly color = input<string | undefined>(undefined);
  /** Colour of the second line; omitted: the same as the first. */
  readonly color2 = input<string | undefined>(undefined);
  protected readonly width = input<number>(120);

  private readonly h = computed(() => positive(this.height(), 32));
  private readonly w = computed(() => positive(this.width(), 120));
  private readonly padding = computed(() => {
    const p = this.pad();
    return Number.isFinite(p) && p >= 0 && 2 * p < this.w() ? p : 3;
  });
  private readonly ceiling = computed(() => finiteOrUndefined(this.max()));
  private readonly range = computed(() => sparklineScale(this.values(), this.values2(), this.scale(), this.ceiling()));
  private readonly options = computed<SparklineOptions>(() => ({ capacity: this.cap(), clamp: this.scale() !== 'fit' }));
  private readonly cap = computed(() => {
    const c = this.capacity();
    return c !== undefined && Number.isFinite(c) && c >= 1 ? c : undefined;
  });

  protected readonly viewWidth = this.w;
  protected readonly viewHeight = this.h;
  protected readonly geometry = computed(() => this.draw(this.values()));
  protected readonly second = computed(() => this.draw(this.values2() ?? []));
  protected readonly summary = computed(() => {
    const first = this.describe(this.label(), this.values());
    const v2 = this.values2();
    return v2 && v2.some((x) => Number.isFinite(x)) ? `${first}; ${this.describe(this.label2(), v2)}` : first;
  });

  private describe(label: string, values: readonly number[]): string {
    const v = values.filter((x) => Number.isFinite(x));
    if (v.length === 0) return `${label}: no data`;
    const f = this.formatValue();
    return `${label}: latest ${f(v[v.length - 1] as number)}, peak ${f(Math.max(...v))}`;
  }

  private draw(values: readonly number[]): SparklineGeometry {
    const { min, max } = this.range();
    return sparklineGeometry(values, this.w(), this.h(), this.padding(), min, max, this.options());
  }
}

function positive(n: number, fallback: number): number {
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

function finiteOrUndefined(n: number | undefined): number | undefined {
  return n !== undefined && Number.isFinite(n) ? n : undefined;
}
