import { DestroyRef, inject, signal, type Signal } from '@angular/core';

/**
 * The current time as a signal that ticks every `ms` (for countdowns and "5 min ago" text only; data is pushed, never
 * polled). Must be called in an injection context; the timer stops when that context is destroyed.
 */
export function injectNow(ms = 1000): Signal<number> {
  const now = signal(Date.now());
  const id = setInterval(() => now.set(Date.now()), ms);
  inject(DestroyRef).onDestroy(() => clearInterval(id));
  return now.asReadonly();
}
