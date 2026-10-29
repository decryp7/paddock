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
  /** Cancel was asked for and the server has not answered yet (a short local flag; the next progress event clears it). */
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
          // The server answered (progress after a Cancel is its answer): the local "cancelling" flag has done its job.
          cancelling: false,
          lastEventAt: now,
        },
      ].sort((a, b) => a.id - b.id);
    }
    case 'done':
    case 'failed':
    case 'cancelled':
      return others;
    default:
      // An event kind this version does not know (a newer app): keep the rows as they are.
      return [...rows];
  }
}

/** How long the id of a finished job is remembered, so a late event of it does not bring the row back. */
export const FINISHED_MEMORY_MS = 60_000;

/** The running background jobs (model loads, benchmarks, service install and repair, app and model update checks). */
@Injectable({ providedIn: 'root' })
export class JobsService {
  private readonly backend = inject(BACKEND);
  private readonly rows = signal<JobRow[]>([]);
  private readonly clock = signal(Date.now());
  private readonly finished = new Map<number, number>();
  private timer: ReturnType<typeof setInterval> | null = null;
  /** The running jobs, oldest first. */
  readonly jobs = this.rows.asReadonly();
  readonly any = computed(() => this.rows().length > 0);

  constructor() {
    this.backend.jobEvents((event) => this.on(event));
    inject(DestroyRef).onDestroy(() => this.stopClock());
  }

  private on(event: JobEvent): void {
    const now = Date.now();
    for (const [id, at] of this.finished) if (now - at > FINISHED_MEMORY_MS) this.finished.delete(id);
    if (this.finished.has(event.jobId)) return;
    if (event.kind === 'done' || event.kind === 'failed' || event.kind === 'cancelled') this.finished.set(event.jobId, now);
    this.update((r) => applyJobEvent(r, event, now));
  }

  /** Applies a change to the rows; the one-second clock for "Still working" runs only while a job does. */
  private update(change: (rows: JobRow[]) => JobRow[]): void {
    this.rows.update(change);
    if (this.rows().length > 0) {
      this.timer ??= setInterval(() => this.clock.set(Date.now()), 1000);
      this.clock.set(Date.now());
    } else this.stopClock();
  }

  private stopClock(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
  }

  /** Whether `job` has been quiet long enough that the panel says "Still working". */
  stalled(job: JobRow): boolean {
    return this.clock() - job.lastEventAt >= STILL_WORKING_MS;
  }

  /**
   * Asks to cancel the background job. The job ends with its own `cancelled` event; if Cancel does not take effect the next
   * progress event says so (`canStopWaiting`) and Cancel may be pressed again. When the backend refuses a job that claims to be
   * cancellable it does not know it (it finished, or the app restarted): the orphaned row goes.
   */
  async cancel(id: number): Promise<void> {
    this.update((r) => r.map((j) => (j.id === id ? { ...j, cancelling: true } : j)));
    const accepted = await this.backend.jobCancel(id).catch(() => false);
    if (accepted) return;
    this.update((r) =>
      r.flatMap((j) => (j.id !== id ? [j] : j.cancellable ? [] : [{ ...j, cancelling: false }])),
    );
  }

  /** The user stops waiting for a job whose Cancel did not take effect. */
  async stopWaiting(id: number): Promise<void> {
    await this.backend.jobStopWaiting(id).catch(() => false);
  }
}
