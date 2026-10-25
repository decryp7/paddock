import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, input, signal, untracked } from '@angular/core';
import { prefersReducedMotion } from '../../shared/chart';

/** How many decimals a number string has (`23.90` → 2). */
function decimals(s: string): number {
  const i = s.indexOf('.');
  return i < 0 ? 0 : s.length - i - 1;
}

/**
 * A number that glides to its new value (about 300 ms, ease-out) instead of jumping, so live values read smoothly.
 * Text that is not a number (`Normal`, `—`) and reduced motion set it at once. The full value is always in the DOM
 * as text, so screen readers and copy get the real number.
 */
@Component({
  selector: 'dy-animated-number',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `{{ shown() }}`,
  styles: `:host { font-variant-numeric: tabular-nums; }`,
})
export class AnimatedNumber {
  /** The value as text, e.g. `23.9`. */
  readonly value = input.required<string>();
  protected readonly shown = signal('');
  private current: number | null = null;
  private frame = 0;

  constructor() {
    const destroyRef = inject(DestroyRef);
    destroyRef.onDestroy(() => this.cancel());
    effect(() => {
      const text = this.value();
      untracked(() => this.go(text));
    });
  }

  private cancel(): void {
    if (this.frame && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(this.frame);
    this.frame = 0;
  }

  private go(text: string): void {
    this.cancel();
    const target = Number(text.replace(/,/g, ''));
    const from = this.current;
    this.current = Number.isFinite(target) ? target : null;
    if (!Number.isFinite(target) || from === null || from === target || prefersReducedMotion() || typeof requestAnimationFrame !== 'function') {
      this.shown.set(text);
      return;
    }
    const d = decimals(text);
    const start = performance.now();
    const step = (now: number): void => {
      const t = Math.min(1, (now - start) / 300);
      const eased = 1 - (1 - t) ** 3;
      this.shown.set(t >= 1 ? text : (from + (target - from) * eased).toFixed(d));
      this.frame = t >= 1 ? 0 : requestAnimationFrame(step);
    };
    this.frame = requestAnimationFrame(step);
  }
}
