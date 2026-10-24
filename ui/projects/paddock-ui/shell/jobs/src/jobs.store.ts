import { computed, effect, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { JobsBackend, type Unlisten } from './jobs-backend';
import { JOBS_CLOCK, JOBS_CONFIG } from './jobs-config';
import { setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { isActiveJob } from './wire';

/** The notification kinds the store posts (registered by `JOBS_CONTRIBUTIONS`). */
export const JOB_NOTICE_KINDS = { finished: 'pd.jobs.finished', failed: 'pd.jobs.failed' } as const;

interface JobsState {
  /** The newest snapshot of every known job (highest `seq` per id), ordered by id. */
  jobs: readonly SuJobSnapshot[];
  /** When the window last heard of each job (its last event, or the list that brought it). */
  heardAt: Readonly<Record<number, number>>;
  /** The clock's time at the last tick or event; "Still working" is measured against it. */
  now: number;
}

/** A job someone waits for: resolved with its terminal snapshot. */
interface Waiter {
  readonly id: number;
  readonly resolve: (job: SuJobSnapshot) => void;
}

/** What `follow` hands back: the end of the job, and a way to stop waiting (the job goes on). */
export interface JobFollowing {
  /** Resolves with the job's terminal snapshot. It never rejects and never times out. */
  readonly finished: Promise<SuJobSnapshot>;
  /** Stops waiting: the job's end is then announced as a notice again. */
  stop(): void;
}

function message(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === 'object' && typeof (e as { message?: unknown }).message === 'string') return (e as { message: string }).message;
  return $localize`:@@pd.jobs.no-answer:The jobs service did not answer.`;
}

/** The notice for a job that just ended, or null (a job the user cancelled needs none). */
export function finishedNotice(job: SuJobSnapshot): ShellNotice | null {
  switch (job.state) {
    case 'done':
      return { level: 'success', kind: JOB_NOTICE_KINDS.finished, message: $localize`:@@pd.jobs.notice.done:${job.title}:title:: done` };
    case 'failed':
      return { level: 'error', kind: JOB_NOTICE_KINDS.failed, message: $localize`:@@pd.jobs.notice.failed:${job.title}:title: failed: ${job.message}:cause:` };
    case 'cancelled':
      return job.reason === 'deadline'
        ? { level: 'warning', kind: JOB_NOTICE_KINDS.failed, message: $localize`:@@pd.jobs.notice.deadline:${job.title}:title: stopped: the time limit was reached` }
        : null;
    default:
      return null;
  }
}

/** Active jobs first (oldest first, the order they run), then finished ones (newest first). */
function listOrder(a: SuJobSnapshot, b: SuJobSnapshot): number {
  const active = Number(isActiveJob(b)) - Number(isActiveJob(a));
  if (active !== 0) return active;
  return isActiveJob(a) ? a.id - b.id : b.id - a.id;
}

/**
 * The window's jobs as the engine reports them (`paddock://jobs`, `jobs_snapshot`): the engine owns every job, its
 * phases and its end; this keeps the newest snapshot per job, which jobs went quiet ("Still working", never a timeout),
 * the request status of the list, and who waits for which job. Provided per window by `provideShellJobs`.
 */
export const JobsStore = signalStore(
  withState<JobsState>({ jobs: [], heardAt: {}, now: 0 }),
  withRequestStatus(),
  withComputed((store, config = inject(JOBS_CONFIG)) => ({
    /** The jobs as the list shows them: active first, then finished. */
    list: computed(() => [...store.jobs()].sort(listOrder)),
    active: computed(() => store.jobs().filter(isActiveJob)),
    activeCount: computed(() => store.jobs().filter(isActiveJob).length),
    /** Running jobs without an event for `stillWorkingAfterMs`. */
    stalledIds: computed(() => {
      const now = store.now();
      const heard = store.heardAt();
      const after = config.stillWorkingAfterMs;
      return new Set(store.jobs().filter((j) => j.state === 'running' && now - (heard[j.id] ?? now) >= after).map((j) => j.id));
    }),
    /** The first running job (the status item names its phase). */
    lead: computed(() => store.jobs().find((j) => j.state === 'running') ?? null),
    _hasRunning: computed(() => store.jobs().some((j) => j.state === 'running')),
  })),
  withMethods((store, backend = inject(JobsBackend), ports = inject(SHELL_PORTS), clock = inject(JOBS_CLOCK)) => {
    const waiters = new Set<Waiter>();

    /** Merges snapshots under the highest-`seq` rule; returns the ones that changed the store. */
    const merge = (incoming: readonly SuJobSnapshot[]): SuJobSnapshot[] => {
      const byId = new Map(store.jobs().map((j) => [j.id, j]));
      const changed: SuJobSnapshot[] = [];
      for (const job of incoming) {
        const known = byId.get(job.id);
        if (known && known.seq >= job.seq) continue;
        byId.set(job.id, job);
        changed.push(job);
      }
      if (changed.length) {
        const now = clock.now();
        const heardAt = { ...store.heardAt() };
        for (const j of changed) heardAt[j.id] = now;
        patchState(store, { jobs: [...byId.values()].sort((a, b) => a.id - b.id), heardAt, now });
      }
      return changed;
    };

    /** Resolves the waiters of `job` if it ended; true when someone was waiting. */
    const settle = (job: SuJobSnapshot): boolean => {
      if (isActiveJob(job)) return false;
      let waited = false;
      for (const w of [...waiters]) {
        if (w.id !== job.id) continue;
        waiters.delete(w);
        w.resolve(job);
        waited = true;
      }
      return waited;
    };

    const applyList = (jobs: readonly SuJobSnapshot[]): void => {
      for (const job of merge(jobs)) settle(job);
      patchState(store, setLoaded());
    };

    return {
      /** Reads the list (also the Retry of a failed load). Never notifies: these jobs did not just end. */
      async load(): Promise<void> {
        patchState(store, setLoading());
        try {
          const jobs = await backend.snapshot();
          // Merged in a sync step of its own: the state is read fresh there, not across the await (rule 29).
          applyList(jobs);
        } catch (e) {
          patchState(store, setError(message(e)));
        }
      },

      /** Asks job `id` to stop; its events tell when it did. A failed request is a notice, not a thrown error. */
      async cancel(id: number): Promise<boolean> {
        try {
          return await backend.cancel(id);
        } catch (e) {
          ports.notify({ level: 'error', kind: JOB_NOTICE_KINDS.failed, message: $localize`:@@pd.jobs.cancel-failed:The job could not be cancelled: ${message(e)}:cause:` });
          return false;
        }
      },

      /**
       * Waits for job `id` to end (a dialog that started it). While someone waits, its end is not also a notice: the
       * waiter shows the outcome. No timeout: a long job is shown as "Still working", never abandoned.
       */
      follow(id: number): JobFollowing {
        let waiter: Waiter | undefined;
        const finished = new Promise<SuJobSnapshot>((resolve) => {
          waiter = { id, resolve };
        });
        const w = waiter as Waiter;
        const known = store.jobs().find((j) => j.id === id);
        if (known && !isActiveJob(known)) w.resolve(known);
        else waiters.add(w);
        return { finished, stop: () => void waiters.delete(w) };
      },

      /** A snapshot from the engine's event. A job that just ended is announced unless someone waits for it. */
      _receive(job: SuJobSnapshot): void {
        const before = store.jobs().find((j) => j.id === job.id);
        if (!merge([job]).length) return;
        const waited = settle(job);
        const justEnded = !isActiveJob(job) && (!before || isActiveJob(before));
        const notice = justEnded && !waited ? finishedNotice(job) : null;
        if (notice) ports.notify(notice);
      },

      _tick(): void {
        patchState(store, { now: clock.now() });
      },
    };
  }),
  withHooks((store, backend = inject(JobsBackend), clock = inject(JOBS_CLOCK), config = inject(JOBS_CONFIG)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then read the list: an event during the read is kept (highest `seq` wins), none is missed.
        void backend
          .listen((job) => store._receive(job))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-jobs] cannot listen for job events: ${message(e)}`));
        void store.load();
        // The clock (outside the signal graph) ticks only while a job runs; `_tick` re-reads the time for "Still working".
        effect((onCleanup) => {
          if (!store._hasRunning()) return;
          const every = Math.max(250, Math.min(1000, config.stillWorkingAfterMs));
          onCleanup(clock.every(every, () => store._tick()));
        });
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type JobsStoreType = InstanceType<typeof JobsStore>;
