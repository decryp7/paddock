import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { downsample, sparkPaths } from '../../shared/geometry';

const PAD = 2;

/**
 * An honest usage chart: an area from a fixed zero baseline up to `max`, a dashed line at the peak labelled
 * `peakLabel` ("peak 52%") and the scale's top label ("100%"). Decorative for assistive tech (the hero number carries the value).
 */
@Component({
  selector: 'pc-usage-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap" [style.height.px]="height()">
      <!-- conformance: svg-ok — usage chart --><svg [attr.viewBox]="'0 0 ' + width() + ' ' + height()" preserveAspectRatio="none" aria-hidden="true" focusable="false">
        <path class="area" [attr.d]="paths().area" />
        <path class="line" [attr.d]="paths().line" vector-effect="non-scaling-stroke" />
      </svg>
      @if (peakTop(); as top) {
        <span class="peak-line" [style.top.%]="top"></span>
        <span class="peak-label" [style.top.%]="top">{{ peakLabel() }}</span>
      }
      <span class="axis axis-top">{{ topLabel() }}</span>
      <span class="axis axis-bottom">0</span>
    </div>
  `,
  styles: `
    :host {
      display: block;
      color: var(--chart-1);
    }
    .wrap {
      position: relative;
      border-bottom: 1px solid var(--border);
    }
    svg {
      display: block;
      width: 100%;
      height: 100%;
      overflow: visible;
    }
    .line {
      fill: none;
      stroke: currentColor;
      stroke-width: 1.5;
      stroke-linejoin: round;
    }
    .area {
      fill: currentColor;
      opacity: 0.12;
    }
    .peak-line {
      position: absolute;
      left: 0;
      right: 0;
      border-top: 1px dashed var(--text-3);
      pointer-events: none;
    }
    .peak-label,
    .axis {
      position: absolute;
      font-size: var(--text-xs);
      font-variant-numeric: tabular-nums;
      white-space: nowrap;
      color: var(--text-3);
    }
    .peak-label {
      right: 0;
      transform: translateY(-100%);
    }
    .axis {
      left: 0;
    }
    .axis-top {
      top: 0;
    }
    .axis-bottom {
      bottom: 2px;
    }
  `,
})
export class UsageChart {
  readonly values = input.required<number[]>();
  /** The value at the top of the chart; the baseline is always zero. */
  readonly max = input.required<number>();
  /** The highest value reached, drawn as a dashed line. */
  readonly peak = input<number | null>(null);
  readonly peakLabel = input('');
  readonly topLabel = input('');
  readonly width = input(320);
  readonly height = input(72);

  protected readonly paths = computed(() => sparkPaths(downsample(this.values(), 60), this.width(), this.height(), this.max()));
  /** The peak line's offset from the top, in percent of the chart height (matches the geometry's 2 px padding). */
  protected readonly peakTop = computed(() => {
    const p = this.peak();
    if (p === null || !Number.isFinite(p) || this.max() <= 0) return null;
    const h = this.height();
    const y = PAD + (h - 2 * PAD) * (1 - Math.min(1, Math.max(0, p / this.max())));
    return (y / h) * 100;
  });
}
