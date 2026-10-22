import { DOCUMENT } from '@angular/common';
import { DestroyRef, Injectable, inject, signal } from '@angular/core';

/**
 * The wall clock for elapsed times and "5 min ago", ticking once a second while the window is visible, and whether the
 * window is visible (refreshes pause while it is hidden).
 */
@Injectable({ providedIn: 'root' })
export class ClockService {
  private readonly doc = inject(DOCUMENT);
  private readonly nowSignal = signal(Date.now());
  private readonly visibleSignal = signal(this.doc.visibilityState !== 'hidden');
  private timer: ReturnType<typeof setInterval> | null = null;

  /** Milliseconds since the epoch, updated every second while visible. */
  readonly now = this.nowSignal.asReadonly();
  /** Whether the window is visible. */
  readonly visible = this.visibleSignal.asReadonly();

  constructor() {
    const onVisibility = () => {
      const visible = this.doc.visibilityState !== 'hidden';
      this.visibleSignal.set(visible);
      if (visible) this.start();
      else this.stopTimer();
    };
    this.doc.addEventListener('visibilitychange', onVisibility);
    inject(DestroyRef).onDestroy(() => {
      this.doc.removeEventListener('visibilitychange', onVisibility);
      this.stopTimer();
    });
    if (this.visibleSignal()) this.start();
  }

  private start(): void {
    this.nowSignal.set(Date.now());
    this.timer ??= setInterval(() => this.nowSignal.set(Date.now()), 1000);
  }

  private stopTimer(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }
}
