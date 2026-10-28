import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

export interface Bar {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Bar geometry for `values` on a zero baseline in a `width` x `height` box (zero values get a 1 px stub). Pure. */
export function trendBars(values: number[], width: number, height: number, gap = 2): Bar[] {
  const n = values.length;
  if (n === 0) return [];
  const w = Math.max(1, (width - gap * (n - 1)) / n);
  const max = Math.max(1, ...values);
  return values.map((v, i) => {
    const h = v > 0 ? Math.max(2, (v / max) * height) : 1;
    return { x: i * (w + gap), y: height - h, w, h };
  });
}

/** A tiny neutral bar chart on a 0 baseline; the number in the neighbouring cell carries the value. */
@Component({
  selector: 'pc-trend-bars',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <!-- conformance: svg-ok — bar chart --><svg [attr.viewBox]="'0 0 ' + width + ' ' + height" role="img" [attr.aria-label]="label()" focusable="false">
      @for (b of bars(); track $index) {
        <rect [attr.x]="b.x" [attr.y]="b.y" [attr.width]="b.w" [attr.height]="b.h" rx="1" />
      }
    </svg>
  `,
  styles: `
    :host {
      display: inline-block;
      color: var(--text-3);
      vertical-align: middle;
    }
    svg {
      display: block;
      width: 64px;
      height: 20px;
    }
    rect {
      fill: currentColor;
    }
  `,
})
export class TrendBars {
  readonly values = input.required<number[]>();
  readonly label = input('Jobs per day, last 7 days');
  protected readonly width = 64;
  protected readonly height = 20;
  protected readonly bars = computed(() => trendBars(this.values(), this.width, this.height));
}
