import { DestroyRef, type Signal, inject, signal } from '@angular/core';

/** The current time as a signal that ticks (default every 30 s) until the component is destroyed, for "3 h ago" and "ends in 2 h" texts. */
export function injectNow(everyMs = 30_000): Signal<number> {
  const now = signal(Date.now());
  const timer = setInterval(() => now.set(Date.now()), everyMs);
  inject(DestroyRef).onDestroy(() => clearInterval(timer));
  return now.asReadonly();
}
