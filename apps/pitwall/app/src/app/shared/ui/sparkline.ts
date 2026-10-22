import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/** SVG paths of a sparkline through `values`, scaled into `w × h` (pure; empty for no data). */
export function sparkPath(values: readonly number[], w: number, h: number): { line: string; area: string } {
  if (values.length === 0) return { line: '', area: '' };
  const max = Math.max(...values, 1);
  const step = values.length > 1 ? w / (values.length - 1) : 0;
  const pts = values.map((v, i) => [i * step, h - 1 - (Math.max(0, v) / max) * (h - 2)] as const);
  const line = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  const area = `${line} L${pts[pts.length - 1]![0].toFixed(1)},${h} L0,${h} Z`;
  return { line, area };
}

/** A tiny trend line, cheap enough for every row of a list (no chart library). */
@Component({
  selector: 'pw-sparkline',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `
    <svg [attr.viewBox]="'0 0 ' + width() + ' ' + height()" [attr.width]="width()" [attr.height]="height()" preserveAspectRatio="none">
      <path [attr.d]="path().area" [attr.fill]="color()" fill-opacity="0.12" />
      <path [attr.d]="path().line" fill="none" [attr.stroke]="color()" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" />
    </svg>
  `,
  styles: `
    :host {
      display: inline-block;
      line-height: 0;
    }
    svg {
      overflow: visible;
    }
  `,
})
export class Sparkline {
  readonly values = input.required<readonly number[]>();
  readonly color = input('currentColor');
  readonly width = input(96);
  readonly height = input(24);
  protected readonly path = computed(() => sparkPath(this.values(), this.width(), this.height()));
}
