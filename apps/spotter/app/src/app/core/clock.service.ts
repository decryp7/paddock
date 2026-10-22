import { Injectable, OnDestroy, signal } from '@angular/core';

/** How often "x minutes ago" text is refreshed. */
export const CLOCK_TICK_MS = 30_000;

/** The current time as a signal, ticking every 30 s, so relative times stay fresh without each view keeping a timer. */
@Injectable({ providedIn: 'root' })
export class ClockService implements OnDestroy {
  private readonly current = signal(Date.now());
  private readonly timer = setInterval(() => this.current.set(Date.now()), CLOCK_TICK_MS);

  readonly now = this.current.asReadonly();

  /** Tests and the mock pin the time. */
  set(ms: number): void {
    this.current.set(ms);
  }

  ngOnDestroy(): void {
    clearInterval(this.timer);
  }
}
