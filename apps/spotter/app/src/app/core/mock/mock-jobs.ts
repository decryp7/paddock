import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import type { CommandError } from '../api.types';

export type Phase = 'fetchingChecksum' | 'checkingExisting' | 'downloading' | 'copying' | 'unpacking' | 'selfTest' | 'finding' | 'settling' | 'checking';
export type Unit = 'bytes' | 'count';

/** Reports one step of a phase; rejects with `cancelled` when the user cancelled the job. */
export type Tick = (phase: Phase, label: string, unit: Unit, done: number, total: number | null) => Promise<void>;

/** Where snapshots go: the shell's Jobs list (a `MemoryJobsBackend.emit` in the browser build, a recorder in tests). */
export type SnapshotSink = (job: SuJobSnapshot) => void;

/**
 * The mock's long operations: they emit the same job snapshots as the shell's `paddock-jobs` (a phase on its own scale,
 * an end) over time, so the browser build and the tests can show and cancel progress. `stepMs` is the pause between
 * steps (0 in unit tests: the whole job runs without waiting).
 */
export class MockJobs {
  private readonly cancelled = new Set<number>();
  private next = 1;
  private sink: SnapshotSink = () => undefined;

  constructor(private readonly stepMs: number) {}

  /** Connects the shell's Jobs list. */
  connect(sink: SnapshotSink): void {
    this.sink = sink;
  }

  /** The Jobs list's Cancel. */
  cancel(id: number): void {
    this.cancelled.add(id);
  }

  /** Runs `work` as a job, announcing how it ends like the shell does (done, cancelled or failed). */
  async run<T>(kind: string, title: string, work: (tick: Tick) => Promise<T>): Promise<T> {
    const id = this.next++;
    const started = Date.now();
    let seq = 0;
    const emit = (state: SuJobSnapshot extends infer S ? Omit<S, never> : never): void => this.sink({ ...(state as object), id, kind, title, seq: ++seq, elapsedMs: Date.now() - started, cancellable: true } as SuJobSnapshot);
    emit({ state: 'queued' } as never);
    const tick: Tick = async (phase, label, unit, done, total) => {
      if (this.cancelled.has(id)) throw cancelledError();
      emit({ state: 'running', phase: { id: phase, label, unit: unit === 'bytes' ? 'bytes' : 'items', total, done } } as never);
      if (this.stepMs > 0) await new Promise((r) => setTimeout(r, this.stepMs));
      if (this.cancelled.has(id)) throw cancelledError();
    };
    try {
      const out = await work(tick);
      emit({ state: 'done' } as never);
      return out;
    } catch (e) {
      const cancelled = (e as Partial<CommandError>).code === 'cancelled';
      emit((cancelled ? { state: 'cancelled', reason: 'user' } : { state: 'failed', message: (e as Partial<CommandError>).message ?? 'Failed.' }) as never);
      throw e;
    } finally {
      this.cancelled.delete(id);
    }
  }
}

function cancelledError(): CommandError {
  return { code: 'cancelled', message: 'Cancelled.', problems: [] };
}
