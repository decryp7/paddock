import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

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

/** The scale for a set of series from zero: the fixed `max` when given, else the highest value with 15% headroom (1 when all are zero). */
export function headroomMax(series: readonly (readonly number[])[], max?: number): number {
  if (max !== undefined) return max;
  const top = Math.max(0, ...series.flat().filter((v) => Number.isFinite(v)));
  return top > 0 ? top * 1.15 : 1;
}

/**
 * The scale a sparkline draws with. `zoom` follows the first series' own range (`max` is then the ceiling, 100 when omitted);
 * `fromZero` starts at 0 and ends at `max` or the peak of both series plus headroom; otherwise `min`/`max` are used as given
 * (`undefined` = fitted to the data).
 */
export function sparklineScale(
  values: readonly number[],
  values2: readonly number[] | null,
  o: { min?: number; max?: number; zoom?: boolean; fromZero?: boolean },
): SparklineScale {
  if (o.zoom) return zoomRange(values, 8, o.max ?? 100);
  if (o.fromZero) return { min: 0, max: headroomMax([values, values2 ?? []], o.max) };
  if (values2 === null || (o.min !== undefined && o.max !== undefined)) return { min: o.min, max: o.max };
  // Two series share one scale, so their lines can be compared.
  const all = [...values, ...values2].filter((v) => Number.isFinite(v));
  if (all.length === 0) return { min: o.min, max: o.max };
  return { min: o.min ?? Math.min(...all), max: o.max ?? Math.max(...all) };
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
  const slots = Math.max(options.capacity ?? values.length, values.length);
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
 * right-aligned, so it grows in from the right), `fromZero` and `zoom` scales, `pad`, `dot` and `color`/`color2`.
 */
@Component({
  selector: 'pd-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[style.color]': 'color()' },
  template: `
    <svg [attr.viewBox]="'0 0 ' + width() + ' ' + height()" [attr.height]="height()" preserveAspectRatio="none" role="img" [attr.aria-label]="summary()">
      @if (area() && geometry().area) {
        <path [attr.d]="geometry().area" class="pd-area" />
      }
      <path [attr.d]="geometry().line" class="pd-line" fill="none" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      @if (second().line) {
        <path [attr.d]="second().line" class="pd-line pd-line2" [style.stroke]="color2()" fill="none" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      }
    </svg>
    @if (dot() && geometry().end; as e) {
      <span class="pd-dot" [style.left.%]="(e.x / width()) * 100" [style.top.px]="e.y"></span>
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
  readonly height = input<number>(32);
  readonly area = input<boolean>(false);
  /** Fixed scale shared by sparklines in one row; omitted: fitted to the data. */
  readonly min = input<number | undefined>(undefined);
  /** The top of the fixed scale; with `zoom`, the ceiling of the zoomed range (100 when omitted). */
  readonly max = input<number | undefined>(undefined);
  /** Scale from 0 up to `max`, or up to the peak of both series plus 15% headroom; values are clamped into the scale. */
  readonly fromZero = input<boolean>(false);
  /** Zoom the scale to the first series' own range (for levels that barely move), inside 0..`max`; values are clamped into it. */
  readonly zoom = input<boolean>(false);
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

  private readonly scale = computed(() =>
    sparklineScale(this.values(), this.values2(), { min: this.min(), max: this.max(), zoom: this.zoom(), fromZero: this.fromZero() }),
  );
  private readonly options = computed<SparklineOptions>(() => ({ capacity: this.capacity(), clamp: this.zoom() || this.fromZero() }));

  protected readonly geometry = computed(() => this.draw(this.values()));
  protected readonly second = computed(() => this.draw(this.values2() ?? []));
  protected readonly summary = computed(() => {
    const v = this.values().filter((x) => Number.isFinite(x));
    if (v.length === 0) return `${this.label()}: no data`;
    return `${this.label()}: latest ${v[v.length - 1]}, peak ${Math.max(...v)}`;
  });

  private draw(values: readonly number[]): SparklineGeometry {
    const { min, max } = this.scale();
    return sparklineGeometry(values, this.width(), this.height(), this.pad(), min, max, this.options());
  }
}
