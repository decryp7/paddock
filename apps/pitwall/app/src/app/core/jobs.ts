import { computed, DestroyRef, inject, Injectable, signal } from '@angular/core';
import type { JobEvent, JobUnit } from './api.types';
import { BACKEND } from './backend';

/** No event for this long while a job runs: the panel says "Still working" (the job is never abandoned, ER-19). */
export const STILL_WORKING_MS = 10_000;

/** One running job as the panel shows it. */
export interface JobRow {
  id: number;
  title: string;
  /** The phase now running, in words ("Starting" until the first progress event). */
  phase: string;
  done: number;
  total: number | null;
  unit: JobUnit;
  elapsedSecs: number;
  cancellable: boolean;
  notCancellableReason: string | null;
  canStopWaiting: boolean;
  /** Cancel was asked for and has not finished yet. */
  cancelling: boolean;
  /** When the last event arrived (the panel's clock), for "Still working". */
  lastEventAt: number;
}

/**
 * The running jobs after one more event. Pure: `started` adds a row, `progress` updates it (a count never goes backwards
 * within a phase), and a terminal event removes it (whoever awaited the job reports the result).
 */
export function applyJobEvent(rows: readonly JobRow[], event: JobEvent, now: number): JobRow[] {
  const others = rows.filter((r) => r.id !== event.jobId);
  const current = rows.find((r) => r.id === event.jobId);
  switch (event.kind) {
    case 'started': {
      const row: JobRow = {
        id: event.jobId,
        title: event.title,
        phase: 'Starting',
        done: 0,
        total: null,
        unit: 'items',
        elapsedSecs: 0,
        cancellable: true,
        notCancellableReason: null,
        canStopWaiting: false,
        cancelling: false,
        lastEventAt: now,
      };
      return [...others, row].sort((x, y) => x.id - y.id);
    }
    case 'progress': {
      const base = current ?? { id: event.jobId, title: 'Working', cancelling: false };
      const samePhase = current !== undefined && current.phase === event.phase && current.unit === event.unit;
      return [
        ...others,
        {
          ...base,
          phase: event.phase,
          done: samePhase ? Math.max(current.done, event.done) : event.done,
          total: event.total,
          unit: event.unit,
          elapsedSecs: Math.max(current?.elapsedSecs ?? 0, event.elapsedSecs),
          cancellable: event.cancellable,
          notCancellableReason: event.notCancellableReason,
          canStopWaiting: event.canStopWaiting,
          lastEventAt: now,
        },
      ].sort((a, b) => a.id - b.id);
    }
    case 'done':
    case 'failed':
    case 'cancelled':
      return others;
  }
}

/** The running background jobs (model loads, benchmarks, service install and repair, app and model update checks). */
@Injectable({ providedIn: 'root' })
export class JobsService {
  private readonly backend = inject(BACKEND);
  private readonly rows = signal<JobRow[]>([]);
  private readonly clock = signal(Date.now());
  /** The running jobs, oldest first. */
  readonly jobs = this.rows.asReadonly();
  readonly any = computed(() => this.rows().length > 0);

  constructor() {
    this.backend.jobEvents((event) => this.rows.update((r) => applyJobEvent(r, event, Date.now())));
    const tick = setInterval(() => this.clock.set(Date.now()), 1000);
    inject(DestroyRef).onDestroy(() => clearInterval(tick));
  }

  /** Whether `job` has been quiet long enough that the panel says "Still working". */
  stalled(job: JobRow): boolean {
    return this.clock() - job.lastEventAt >= STILL_WORKING_MS;
  }

  /** Asks to cancel; the job ends with its own `cancelled` event. A refusal (a step that cannot be interrupted) leaves it running. */
  async cancel(id: number): Promise<void> {
    this.rows.update((r) => r.map((j) => (j.id === id ? { ...j, cancelling: true } : j)));
    const accepted = await this.backend.jobCancel(id).catch(() => false);
    if (!accepted) this.rows.update((r) => r.map((j) => (j.id === id ? { ...j, cancelling: false } : j)));
  }

  /** The user stops waiting for a job whose Cancel did not take effect. */
  async stopWaiting(id: number): Promise<void> {
    await this.backend.jobStopWaiting(id).catch(() => false);
  }
}
