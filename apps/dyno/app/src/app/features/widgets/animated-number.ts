import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, input, signal, untracked } from '@angular/core';
import { prefersReducedMotion } from '../../shared/chart';

/** How many decimals a number string has (`23.90` → 2). */
function decimals(s: string): number {
  const i = s.indexOf('.');
  return i < 0 ? 0 : s.length - i - 1;
}

/** How long a glide takes. */
export const GLIDE_MS = 300;
/** How many values a glide shows, the last being the target: about 20 per second instead of one per display frame. */
export const GLIDE_STEPS = 6;

/**
 * The texts a glide from `from` to `target` shows, one per step, eased out and printed with `decimals` decimals; the last
 * is `text` itself. Pure, so the steps are tested without a clock.
 */
export function glideSteps(from: number, target: number, text: string, decimals: number, steps = GLIDE_STEPS): string[] {
  const out: string[] = [];
  for (let i = 1; i < steps; i++) {
    const eased = 1 - (1 - i / steps) ** 3;
    out.push((from + (target - from) * eased).toFixed(decimals));
  }
  out.push(text);
  return out;
}

/**
 * A number that glides to its new value (about 300 ms, ease-out) instead of jumping, so live values read smoothly.
 * Text that is not a number (`Normal`, `—`) and reduced motion set it at once. The full value is always in the DOM
 * as text, so screen readers and copy get the real number.
 *
 * The glide takes {@link GLIDE_STEPS} steps on a timer, not one per animation frame: each step repaints the page, and a
 * dashboard of live numbers that glided every second kept the web view painting about 18 frames a second
 * (docs/benchmarks.md, 2026-11-03).
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
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    const destroyRef = inject(DestroyRef);
    destroyRef.onDestroy(() => this.cancel());
    effect(() => {
      const text = this.value();
      untracked(() => this.go(text));
    });
  }

  private cancel(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private go(text: string): void {
    this.cancel();
    const target = Number(text.replace(/,/g, ''));
    const from = this.current;
    this.current = Number.isFinite(target) ? target : null;
    if (!Number.isFinite(target) || from === null || from === target || prefersReducedMotion()) {
      this.shown.set(text);
      return;
    }
    const steps = glideSteps(from, target, text, decimals(text));
    const next = (i: number): void => {
      this.shown.set(steps[i] ?? text);
      this.timer = i + 1 < steps.length ? setTimeout(() => next(i + 1), GLIDE_MS / GLIDE_STEPS) : null;
    };
    this.timer = setTimeout(() => next(0), GLIDE_MS / GLIDE_STEPS);
  }
}
