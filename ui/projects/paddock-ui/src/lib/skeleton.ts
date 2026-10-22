import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

export type SuSkeletonShape = 'text' | 'block' | 'circle';

/** Placeholder with the final geometry (no layout shift). Shimmer is 1.6 s and static under reduced motion. */
@Component({
  selector: 'pd-skeleton',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    'aria-hidden': 'true',
    '[class]': '"pd-skeleton pd-skeleton--" + shape()',
    '[style.width]': 'cssWidth()',
    '[style.height]': 'cssHeight()',
  },
  template: ``,
  styles: `
    :host {
      display: block;
      background: linear-gradient(90deg, var(--pd-skeleton) 0%, var(--pd-skeleton-shine) 50%, var(--pd-skeleton) 100%);
      background-size: 200% 100%;
      animation: pd-shimmer 1.6s linear infinite;
      border-radius: var(--pd-radius-tag);
    }
    :host(.pd-skeleton--block) { border-radius: var(--pd-radius-card); }
    :host(.pd-skeleton--circle) { border-radius: 50%; }
    @keyframes pd-shimmer { from { background-position: 100% 0; } to { background-position: -100% 0; } }
    @media (prefers-reduced-motion: reduce) { :host { animation: none; background: var(--pd-skeleton); } }
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
