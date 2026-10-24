import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

export interface SparklineGeometry {
  line: string;
  area: string;
  end: { x: number; y: number } | null;
}

/** Pure geometry of a sparkline: points scaled into `width` x `height` with `pad` px around (so the end dot is not clipped). */
export function sparklineGeometry(values: readonly number[], width: number, height: number, pad = 3, min?: number, max?: number): SparklineGeometry {
  const finite = values.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return { line: '', area: '', end: null };
  const lo = min ?? Math.min(...finite);
  const hi = max ?? Math.max(...finite);
  const span = hi - lo;
  const step = values.length > 1 ? (width - 2 * pad) / (values.length - 1) : 0;
  const pts = values.map((v, i) => (Number.isFinite(v) ? { x: pad + i * step, y: span === 0 ? height / 2 : pad + (1 - (v - lo) / span) * (height - 2 * pad) } : null));
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
  const area = `${line}L${last.x.toFixed(1)} ${height}L${first.x.toFixed(1)} ${height}Z`;
  return { line, area, end: pts[pts.length - 1] ?? null };
}

/** Tiny inline-SVG trend: 1.5 px line, optional area at 14% alpha, 4 px end dot. 24-32 px high, no axes. */
@Component({
  selector: 'pd-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg [attr.viewBox]="'0 0 ' + width() + ' ' + height()" [attr.height]="height()" preserveAspectRatio="none" role="img" [attr.aria-label]="summary()">
      @if (area() && geometry().line) {
        <path [attr.d]="geometry().area" class="pd-area" />
      }
      <path [attr.d]="geometry().line" class="pd-line" fill="none" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
    </svg>
    @if (geometry().end; as e) {
      <span class="pd-dot" [style.left.%]="(e.x / width()) * 100" [style.top.px]="e.y"></span>
    }
  `,
  styles: `
    :host { position: relative; display: block; width: 100%; color: var(--pd-accent); }
    svg { display: block; width: 100%; overflow: visible; }
    .pd-line { stroke: currentColor; }
    .pd-area { fill: currentColor; opacity: 0.14; }
    /* A 4 px dot as an element: the SVG stretches to the tile width, which would squash a circle. */
    .pd-dot { position: absolute; width: 4px; height: 4px; margin: -2px 0 0 -2px; border-radius: 50%; background: currentColor; }
  `,
})
export class SuSparkline {
  readonly values = input.required<readonly number[]>();
  /** Accessible name, e.g. "CPU, last hour". The summary (latest and peak) is appended. */
  readonly label = input<string>($localize`:@@pd.ui.sparkline:Trend`);
  readonly height = input<number>(32);
  readonly area = input<boolean>(false);
  /** Fixed scale shared by sparklines in one row; omitted: fitted to the data. */
  readonly min = input<number | undefined>(undefined);
  readonly max = input<number | undefined>(undefined);
  protected readonly width = input<number>(120);

  protected readonly geometry = computed(() => sparklineGeometry(this.values(), this.width(), this.height(), 3, this.min(), this.max()));
  protected readonly summary = computed(() => {
    const v = this.values().filter((x) => Number.isFinite(x));
    if (v.length === 0) return `${this.label()}: no data`;
    return `${this.label()}: latest ${v[v.length - 1]}, peak ${Math.max(...v)}`;
  });
}
