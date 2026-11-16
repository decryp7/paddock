import { Injectable, InjectionToken, OnDestroy, inject, signal } from '@angular/core';

/** How often "x minutes ago" text is refreshed. */
export const CLOCK_TICK_MS = 30_000;

/** The interval timers of the clock: the page's by default; a unit test provides ones it can hold (`holdableTimers` of `@decryp7/paddock-ui/testing`). */
export interface ClockTimers {
  setInterval(fn: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
}

export const CLOCK_TIMERS = new InjectionToken<ClockTimers>('CLOCK_TIMERS', {
  providedIn: 'root',
  factory: () => ({ setInterval: (fn, ms) => setInterval(fn, ms), clearInterval: (h) => clearInterval(h as ReturnType<typeof setInterval>) }),
});

/** The current time as a signal, ticking every 30 s, so relative times stay fresh without each view keeping a timer. */
@Injectable({ providedIn: 'root' })
export class ClockService implements OnDestroy {
  private readonly current = signal(Date.now());
  private readonly timers = inject(CLOCK_TIMERS);
  private readonly timer = this.timers.setInterval(() => this.current.set(Date.now()), CLOCK_TICK_MS);

  readonly now = this.current.asReadonly();

  /** Tests and the mock pin the time. */
  set(ms: number): void {
    this.current.set(ms);
  }

  ngOnDestroy(): void {
    this.timers.clearInterval(this.timer);
  }
}
