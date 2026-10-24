import { Injectable, OnDestroy } from '@angular/core';
import type { SuJobPhase, SuJobSnapshot } from '@decryp7/paddock-ui';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';

/** One step of a simulated job: a phase and how long it runs, or a step that cannot be interrupted. */
interface Step {
  readonly phase: Omit<SuJobPhase, 'done'>;
  /** Ticks (100 ms each) the step takes; `null`: until cancelled, without a single event (a quiet step). */
  readonly ticks: number | null;
  /** Total found at the end of a discovery step (its total is unknown while it runs). */
  readonly found?: number;
  readonly uncancellable?: string;
}

interface Running {
  readonly id: number;
  readonly title: string;
  readonly subject: string | null;
  readonly steps: readonly Step[];
  step: number;
  tick: number;
  seq: number;
  started: number;
  cancelRequested: boolean;
}

const TICK_MS = 100;

/**
 * The demo's stand-in for the engine (the browser build has no Rust): jobs with phases on their own scales, a step that
 * cannot be cancelled, and a quiet job that sends no events until cancelled. It plays `paddock-jobs`' part through
 * `MemoryJobsBackend`; the shell's jobs code is the same as in a real app.
 */
@Injectable({ providedIn: 'root' })
export class DemoJobsEngine implements OnDestroy {
  readonly backend = new MemoryJobsBackend((id) => this.requestCancel(id));
  private readonly jobs = new Map<number, Running>();
  private next = 0;
  private timer: ReturnType<typeof setInterval> | null = null;

  /** An import: finding files (growing count), reading them (determinate), writing the index (not cancellable). */
  startImport(): number {
    return this.start('Importing logs', '3 folders', [
      { phase: { id: 'find', label: 'Finding files', unit: 'items', total: null }, ticks: 20, found: 140 },
      { phase: { id: 'read', label: 'Reading', unit: 'bytes', total: 8 * 1024 * 1024 }, ticks: 50 },
      { phase: { id: 'index', label: 'Writing the index', unit: 'items', total: 1 }, ticks: 10, uncancellable: 'Writing the index' },
    ]);
  }

  /** A job whose step reports nothing for a long time: the UI shows "Still working" and never gives up on it. */
  startQuiet(): number {
    return this.start('Update runner', 'build-mac-2', [{ phase: { id: 'wait', label: 'Waiting for the device', unit: 'items', total: null }, ticks: null }]);
  }

  ngOnDestroy(): void {
    this.stopTimer();
  }

  private start(title: string, subject: string | null, steps: readonly Step[]): number {
    const job: Running = { id: ++this.next, title, subject, steps, step: -1, tick: 0, seq: 0, started: Date.now(), cancelRequested: false };
    this.jobs.set(job.id, job);
    this.emit(job, { state: 'queued' });
    this.enter(job, 0);
    this.timer ??= setInterval(() => this.advance(), TICK_MS);
    return job.id;
  }

  private requestCancel(id: number): void {
    const job = this.jobs.get(id);
    if (job) job.cancelRequested = true;
  }

  private advance(): void {
    for (const job of [...this.jobs.values()]) {
      const step = job.steps[job.step] as Step;
      if (job.cancelRequested && !step.uncancellable) {
        this.end(job, { state: 'cancelled', reason: 'user' });
        continue;
      }
      if (step.ticks === null) continue;
      job.tick++;
      if (job.tick < step.ticks) {
        this.emit(job, this.running(job));
      } else if (job.cancelRequested) {
        this.end(job, { state: 'cancelled', reason: 'user' });
      } else if (job.step + 1 < job.steps.length) {
        this.enter(job, job.step + 1);
      } else {
        this.end(job, { state: 'done' });
      }
    }
    if (this.jobs.size === 0) this.stopTimer();
  }

  private enter(job: Running, step: number): void {
    job.step = step;
    job.tick = 0;
    this.emit(job, this.running(job));
  }

  private running(job: Running): Partial<SuJobSnapshot> {
    const step = job.steps[job.step] as Step;
    const total = step.phase.total ?? null;
    const scale = total ?? step.found ?? 0;
    const done = step.ticks ? Math.round((scale * job.tick) / step.ticks) : 0;
    return { state: 'running', phase: { ...step.phase, total, done } };
  }

  private end(job: Running, state: Partial<SuJobSnapshot>): void {
    this.jobs.delete(job.id);
    this.emit(job, state);
  }

  private emit(job: Running, state: Partial<SuJobSnapshot>): void {
    const step = job.steps[job.step];
    const active = state.state === 'queued' || state.state === 'running';
    const reason = active ? (step?.uncancellable ?? null) : null;
    this.backend.emit({
      id: job.id,
      kind: 'demo',
      title: job.title,
      subject: job.subject,
      seq: ++job.seq,
      elapsedMs: Date.now() - job.started,
      etaMs: null,
      cancellable: active && reason === null,
      notCancellableReason: reason,
      ...state,
    } as SuJobSnapshot);
  }

  private stopTimer(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
  }
}
