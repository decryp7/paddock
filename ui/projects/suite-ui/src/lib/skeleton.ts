import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

export type SuSkeletonShape = 'text' | 'block' | 'circle';

/** Placeholder with the final geometry (no layout shift). Shimmer is 1.6 s and static under reduced motion. */
@Component({
  selector: 'su-skeleton',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    'aria-hidden': 'true',
    '[class]': '"su-skeleton su-skeleton--" + shape()',
    '[style.width]': 'cssWidth()',
    '[style.height]': 'cssHeight()',
  },
  template: ``,
  styles: `
    :host {
      display: block;
      background: linear-gradient(90deg, var(--su-skeleton) 0%, var(--su-skeleton-shine) 50%, var(--su-skeleton) 100%);
      background-size: 200% 100%;
      animation: su-shimmer 1.6s linear infinite;
      border-radius: var(--su-radius-tag);
    }
    :host(.su-skeleton--block) { border-radius: var(--su-radius-card); }
    :host(.su-skeleton--circle) { border-radius: 50%; }
    @keyframes su-shimmer { from { background-position: 100% 0; } to { background-position: -100% 0; } }
    @media (prefers-reduced-motion: reduce) { :host { animation: none; background: var(--su-skeleton); } }
  `,
})
export class SuSkeleton {
  readonly shape = input<SuSkeletonShape>('text');
  /** CSS length or a number of px. */
  readonly width = input<string | number>('100%');
  readonly height = input<string | number | undefined>(undefined);

  protected readonly cssWidth = computed(() => toCss(this.width()));
  protected readonly cssHeight = computed(() => toCss(this.height() ?? (this.shape() === 'text' ? 12 : 32)));
}

function toCss(v: string | number): string {
  return typeof v === 'number' ? `${v}px` : v;
}
