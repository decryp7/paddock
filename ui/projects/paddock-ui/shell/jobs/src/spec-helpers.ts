// Test helpers of the jobs specs (not exported from the entry point).
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { SuJobPhase, SuJobSnapshot } from '@decryp7/paddock-ui';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { MemoryJobsBackend, type JobsBackend } from './jobs-backend';
import { JOBS_CLOCK, type JobsClock } from './jobs-config';
import { provideShellJobs } from './provide-jobs';

/** A clock moved by hand; `advance` fires every tick that is due (once per call, enough for these specs). */
export class FakeClock implements JobsClock {
  time = 1_000;
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

/** A snapshot with plausible defaults. */
export function snap(id: number, seq: number, state: SuJobSnapshot['state'] = 'running', extra: Record<string, unknown> = {}): SuJobSnapshot {
  const base = { id, kind: 'scan', title: `Job ${id}`, subject: null, seq, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null };
  const byState: Record<SuJobSnapshot['state'], object> = {
    queued: {},
    running: { phase: null },
    done: {},
    failed: { message: 'it broke' },
    cancelled: { reason: 'user' },
  };
  return { ...base, state, ...byState[state], ...extra } as SuJobSnapshot;
}

export function phase(label: string, done: number, total: number | null = null): SuJobPhase {
  return { id: label.toLowerCase(), label, unit: 'items', total, done };
}

/** Lets promises and effects settle (zoneless). */
export async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  TestBed.tick();
}

export interface JobsBed {
  backend: MemoryJobsBackend;
  clock: FakeClock;
  notices: ShellNotice[];
}

/** TestBed with the window's jobs over a memory backend, a fake clock and captured notices. */
export function jobsBed(options: { backend?: JobsBackend; stillWorkingAfterMs?: number; extra?: unknown[] } = {}): JobsBed {
  const backend = new MemoryJobsBackend();
  const clock = new FakeClock();
  const notices: ShellNotice[] = [];
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideShellJobs({ backend: () => options.backend ?? backend, stillWorkingAfterMs: options.stillWorkingAfterMs ?? 10_000 }),
      { provide: JOBS_CLOCK, useValue: clock },
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: (n: ShellNotice) => void notices.push(n) } },
      ...((options.extra ?? []) as never[]),
    ],
  });
  return { backend, clock, notices };
}
