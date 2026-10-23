import type { CommandError, JobEvent, JobKind, JobProgress } from '../api.types';

/** Reports one step of a phase; rejects with `cancelled` when the user cancelled the job. */
export type Tick = (phase: JobProgress['phase'], label: string, unit: JobProgress['unit'], done: number, total: number | null) => Promise<void>;

/**
 * The mock's long operations: they emit the same `JobEvent`s as the shell (a phase, its own scale, an end) over
 * time, so the browser build and the tests can show and cancel progress. `stepMs` is the pause between steps
 * (0 in unit tests: the whole job runs without waiting).
 */
export class MockJobs {
  private readonly handlers = new Set<(e: JobEvent) => void>();
  private readonly cancelled = new Set<string>();

  constructor(private readonly stepMs: number) {}

  on(handler: (e: JobEvent) => void): () => void {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }

  cancel(jobId: string): void {
    this.cancelled.add(jobId);
  }

  private emit(e: JobEvent): void {
    this.handlers.forEach((h) => h(e));
  }

  /** Runs `work`, announcing how it ends like the shell does (finished, cancelled or failed). */
  async run<T>(jobId: string, kind: JobKind, work: (tick: Tick) => Promise<T>): Promise<T> {
    const tick: Tick = async (phase, label, unit, done, total) => {
      if (this.cancelled.has(jobId)) throw cancelledError();
      this.emit({ jobId, kind, state: 'running', progress: { phase, label, unit, done, total } });
      if (this.stepMs > 0) await new Promise((r) => setTimeout(r, this.stepMs));
      if (this.cancelled.has(jobId)) throw cancelledError();
    };
    try {
      const out = await work(tick);
      this.emit({ jobId, kind, state: 'finished', progress: null });
      return out;
    } catch (e) {
      const cancelled = (e as Partial<CommandError>).code === 'cancelled';
      this.emit({ jobId, kind, state: cancelled ? 'cancelled' : 'failed', progress: null });
      throw e;
    } finally {
      this.cancelled.delete(jobId);
    }
  }
}

function cancelledError(): CommandError {
  return { code: 'cancelled', message: 'Cancelled.', problems: [] };
}
