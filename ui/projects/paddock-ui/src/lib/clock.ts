import { DestroyRef, inject, Injectable, InjectionToken, signal } from '@angular/core';
import { DOCUMENT } from '@angular/common';

/** Where the clock reads the time (tests give a manual one). */
export const PD_CLOCK_SOURCE = new InjectionToken<() => number>('PD_CLOCK_SOURCE', { providedIn: 'root', factory: () => () => Date.now() });

/** How often the clock ticks while the window is visible (ms). */
export const PD_CLOCK_INTERVAL = new InjectionToken<number>('PD_CLOCK_INTERVAL', { providedIn: 'root', factory: () => 1000 });

/**
 * The wall clock for elapsed times and "5 min ago": `now` (ms since the epoch) is a signal that ticks every second while
 * the window is visible and stops while it is hidden (no timer, no change detection), jumping to the real time when the
 * window shows again. `visible` tells refreshes to pause as well. `tick()` reads the time now (tests, or right after an
 * action).
 */
@Injectable({ providedIn: 'root' })
export class PdClock {
  private readonly doc = inject(DOCUMENT);
  private readonly source = inject(PD_CLOCK_SOURCE);
  private readonly interval = inject(PD_CLOCK_INTERVAL);
  private readonly nowSignal = signal(this.source());
  private readonly visibleSignal = signal(this.doc.visibilityState !== 'hidden');
  private timer: ReturnType<typeof setInterval> | null = null;

  /** Milliseconds since the epoch, refreshed every second while visible. */
  readonly now = this.nowSignal.asReadonly();
  /** Whether the window is visible. */
  readonly visible = this.visibleSignal.asReadonly();

  constructor() {
    const onVisibility = () => {
      const visible = this.doc.visibilityState !== 'hidden';
      this.visibleSignal.set(visible);
      if (visible) this.start();
      else this.stop();
    };
    this.doc.addEventListener('visibilitychange', onVisibility);
    inject(DestroyRef).onDestroy(() => {
      this.doc.removeEventListener('visibilitychange', onVisibility);
      this.stop();
    });
    if (this.visibleSignal()) this.start();
  }

  /** Reads the time now. */
  tick(): void {
    this.nowSignal.set(this.source());
  }

  private start(): void {
    this.tick();
    this.timer ??= setInterval(() => this.tick(), this.interval);
  }

  private stop(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
  }
}
