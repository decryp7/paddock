// Test helpers of the undo specs (not exported from the entry point).
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { UNDO_CLOCK, type UndoClock } from './undo-config';

/** A clock moved by hand: `advance` moves the time and fires the one-shot timers that are due. */
export class ManualUndoClock implements UndoClock {
  time = 1_000_000;
  private readonly timers = new Set<{ at: number; fn: () => void }>();
  now(): number {
    return this.time;
  }
  after(ms: number, fn: () => void): () => void {
    const t = { at: this.time + ms, fn };
    this.timers.add(t);
    return () => void this.timers.delete(t);
  }
  advance(ms: number): void {
    this.time += ms;
    for (const t of [...this.timers]) {
      if (t.at <= this.time) {
        this.timers.delete(t);
        t.fn();
      }
    }
  }
  get pending(): number {
    return this.timers.size;
  }
}

export interface Bed {
  clock: ManualUndoClock;
  notices: ShellNotice[];
}

/** TestBed with a manual clock and captured notices; `providers` adds the stores under test. */
export function bed(providers: unknown[]): Bed {
  const clock = new ManualUndoClock();
  const notices: ShellNotice[] = [];
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: UNDO_CLOCK, useValue: clock },
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: (n: ShellNotice) => void notices.push(n) } },
      ...(providers as never[]),
    ],
  });
  return { clock, notices };
}

/** Lets promises and effects settle (zoneless). */
export async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  TestBed.tick();
}
