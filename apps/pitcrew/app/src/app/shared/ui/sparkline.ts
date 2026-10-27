import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { downsample, sparkPaths } from '../geometry';

/** A small trend line (decorative: the number next to it carries the value). */
@Component({
  selector: 'pc-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg [attr.viewBox]="'0 0 ' + width() + ' ' + height()" preserveAspectRatio="none" aria-hidden="true" focusable="false">
      <path class="area" [attr.d]="paths().area" />
      <path class="line" [attr.d]="paths().line" vector-effect="non-scaling-stroke" />
    </svg>
  `,
  styles: `
    :host {
      display: block;
      color: var(--chart-1);
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
      stroke-linecap: round;
    }
    .area {
      fill: currentColor;
      opacity: 0.1;
    }
  `,
})
export class Sparkline {
  readonly values = input.required<number[]>();
  readonly max = input<number | undefined>(undefined);
  readonly width = input(120);
  readonly height = input(32);
  readonly points = input(60);

  protected readonly paths = computed(() => sparkPaths(downsample(this.values(), this.points()), this.width(), this.height(), this.max()));
}
