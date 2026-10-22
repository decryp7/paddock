import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/** Shimmering placeholder lines while data loads (static when reduced motion is preferred). */
@Component({
  selector: 'pw-skeleton',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `
    @for (w of widths(); track $index) {
      <span class="line" [style.width]="w" [style.height]="lineHeight()"></span>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-2);
      padding: var(--sp-1) 0;
    }
    .line {
      display: block;
      border-radius: var(--r-sm);
      background: linear-gradient(90deg, var(--skeleton) 0%, var(--skeleton-shine) 50%, var(--skeleton) 100%);
      background-size: 200% 100%;
      animation: shimmer 1.4s ease-in-out infinite;
    }
    @keyframes shimmer {
      from {
        background-position: 100% 0;
      }
      to {
        background-position: -100% 0;
      }
    }
  `,
})
export class Skeleton {
  readonly lines = input(3);
  readonly lineHeight = input('12px');
  protected readonly widths = computed(() => Array.from({ length: this.lines() }, (_, i) => `${[92, 76, 84, 58, 70][i % 5]}%`));
}
