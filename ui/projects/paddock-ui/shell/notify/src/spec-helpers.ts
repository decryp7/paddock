// Test helpers of the notify specs (not exported from the entry point).
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemoryNotificationCentre, MemoryServiceHealth, type MemoryRepairOutcome, type NotificationsBackend, type ServiceHealthBackend } from './backends';
import { NOTIFY_CLOCK, type NotifyClock } from './notify-config';
import { provideShellNotify } from './provide-notify';
import type { Cause } from './wire.types';

/** A clock moved by hand; `advance` fires every tick (once per call). */
export class FakeClock implements NotifyClock {
  time = 1_000_000;
  readonly timers = new Set<{ ms: number; tick: () => void }>();
  now(): number {
    return this.time;
  }
  every(ms: number, tick: () => void): () => void {
    const t = { ms, tick };
    this.timers.add(t);
    return () => void this.timers.delete(t);
  }
  advance(ms: number): void {
    this.time += ms;
    for (const t of [...this.timers]) t.tick();
  }
}

/** Lets promises and effects settle (zoneless). */
export async function settle(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  TestBed.tick();
  for (let i = 0; i < 4; i++) await Promise.resolve();
  TestBed.tick();
}

export const cause = (message: string, extra: Partial<Cause> = {}): Cause => ({ message, hint: null, detail: null, code: null, raiseCap: null, settings: null, ...extra });

export interface NotifyBed {
  centre: MemoryNotificationCentre;
  health: MemoryServiceHealth;
  clock: FakeClock;
}

/** TestBed with a window's notifications over memory backends and a fake clock (no host mounted). */
export function notifyBed(
  options: { health?: ServiceHealthBackend; centre?: NotificationsBackend; repairOutcome?: (s: string) => MemoryRepairOutcome; extra?: unknown[]; mountHost?: boolean } = {},
): NotifyBed {
  const clock = new FakeClock();
  const centre = new MemoryNotificationCentre(() => clock.now());
  const health = new MemoryServiceHealth(
    [
      { id: 'store', label: 'the log store' },
      { id: 'ai', label: 'the AI worker' },
    ],
    { centre, repairOutcome: options.repairOutcome },
  );
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideShellNotify({ health: () => options.health ?? health, notifications: () => options.centre ?? centre, mountHost: options.mountHost ?? false }),
      { provide: NOTIFY_CLOCK, useValue: clock },
      ...((options.extra ?? []) as never[]),
    ],
  });
  return { centre, health, clock };
}

/** The elements under `root` matching `selector`. */
export function all(root: ParentNode, selector: string): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(selector)];
}

export function one(root: ParentNode, selector: string): HTMLElement {
  const el = root.querySelector<HTMLElement>(selector);
  if (!el) throw new Error(`no ${selector}`);
  return el;
}
