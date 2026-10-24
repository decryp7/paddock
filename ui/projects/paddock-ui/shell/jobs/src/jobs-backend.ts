import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import type { JobsCommand } from '../generated/jobs-commands';
import { JOBS_EVENT, parseJobSnapshot, parseJobSnapshots } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The jobs port (ADR 0012 §4): the Tauri commands of `paddock_jobs::tauri` in the desktop app, a fake in the browser build
 * and in tests. Components and stores never call `invoke`; they get this by DI. Every method returns at once: none waits
 * for a job, and none has a timeout (rule 12).
 */
export abstract class JobsBackend {
  /** The latest snapshot of every known job, ordered by id (`jobs_snapshot`). */
  abstract snapshot(): Promise<readonly SuJobSnapshot[]>;
  /** Asks job `id` to stop (`job_cancel`); false when it is unknown or already ended. */
  abstract cancel(id: number): Promise<boolean>;
  /** Calls `handler` with every snapshot the engine emits (`paddock://jobs`) until the returned function is called. */
  abstract listen(handler: (job: SuJobSnapshot) => void): Promise<Unlisten>;
}

/**
 * The app's IPC as the jobs backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke`
 * and an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface JobsIpc {
  invoke<T>(command: JobsCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/** The jobs backend over Tauri. Payloads are parsed at this edge; one that is not a snapshot is dropped. */
export class TauriJobsBackend extends JobsBackend {
  constructor(private readonly ipc: JobsIpc) {
    super();
  }

  async snapshot(): Promise<readonly SuJobSnapshot[]> {
    return parseJobSnapshots(await this.ipc.invoke<unknown>('jobs_snapshot'));
  }

  async cancel(id: number): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('job_cancel', { id })) === true;
  }

  listen(handler: (job: SuJobSnapshot) => void): Promise<Unlisten> {
    return this.ipc.listen(JOBS_EVENT, (payload) => {
      const job = parseJobSnapshot(payload);
      if (job) handler(job);
    });
  }
}

/**
 * A backend held in memory, for the browser build, demos and tests: `emit` plays the engine's part (a snapshot reaches
 * every listener and the list), `onCancel` decides what a cancel does (by default nothing beyond answering true for an
 * active job: the simulated job reports its own end, as a real one does).
 */
export class MemoryJobsBackend extends JobsBackend {
  private readonly jobs = new Map<number, SuJobSnapshot>();
  private readonly listeners = new Set<(job: SuJobSnapshot) => void>();

  constructor(private readonly onCancel: (id: number, job: SuJobSnapshot) => void = () => undefined) {
    super();
  }

  /** Publishes a snapshot as the engine would (the newest `seq` per id is kept). */
  emit(job: SuJobSnapshot): void {
    const known = this.jobs.get(job.id);
    if (!known || known.seq < job.seq) this.jobs.set(job.id, job);
    for (const l of [...this.listeners]) l(job);
  }

  /** The latest snapshot of job `id`, if known. */
  get(id: number): SuJobSnapshot | undefined {
    return this.jobs.get(id);
  }

  snapshot(): Promise<readonly SuJobSnapshot[]> {
    return Promise.resolve([...this.jobs.values()].sort((a, b) => a.id - b.id));
  }

  cancel(id: number): Promise<boolean> {
    const job = this.jobs.get(id);
    if (!job || (job.state !== 'queued' && job.state !== 'running')) return Promise.resolve(false);
    this.onCancel(id, job);
    return Promise.resolve(true);
  }

  listen(handler: (job: SuJobSnapshot) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
