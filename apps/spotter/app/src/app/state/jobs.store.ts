import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { JobEvent, JobKind, JobProgress } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { ToastStore } from './toast.store';

/** A long operation as the UI shows it. */
export interface JobView {
  id: string;
  kind: JobKind;
  /** What it is, for the operations panel ("Downloading Lumen CLI 2.4.1"). */
  title: string;
  startedAt: number;
  /** When the engine last reported; "still working" is shown when this gets old. */
  lastEventAt: number;
  /** The phase it is in on its own scale; null until the engine has reported. */
  progress: JobProgress | null;
  cancelling: boolean;
}

interface JobsState {
  jobs: JobView[];
}

let counter = 0;

/** A fresh id the shell accepts (letters, digits, dashes). */
function newId(kind: JobKind): string {
  counter += 1;
  return `${kind}-${Date.now().toString(36)}-${counter}`;
}

/**
 * The running long operations (release downloads, the browser install, checks, the wizard's test). They are shown from
 * the moment they begin and follow the engine's `JobEvent`s; the UI never gives up on one on its own: the engine ends
 * it, or the user cancels it.
 */
export const JobsStore = signalStore(
  { providedIn: 'root' },
  withState<JobsState>({ jobs: [] }),
  withComputed((s) => ({
    running: computed(() => s.jobs().length),
  })),
  withMethods((store) => {
    const backend = inject(BACKEND);
    const toast = inject(ToastStore);
    let subscribed: Promise<void> | null = null;

    const forget = (id: string): void => patchState(store, { jobs: store.jobs().filter((j) => j.id !== id) });

    const onEvent = (e: JobEvent): void => {
      const known = store.jobs().some((j) => j.id === e.jobId);
      if (!known) return;
      if (e.state === 'running') {
        patchState(store, { jobs: store.jobs().map((j) => (j.id === e.jobId ? { ...j, progress: e.progress, lastEventAt: Date.now() } : j)) });
        return;
      }
      forget(e.jobId);
      if (e.kind === 'checkAll' && e.state === 'cancelled') toast.show({ tone: 'info', icon: 'info', title: 'Check cancelled', body: null });
    };

    /** Subscribes once; a failed subscription only means no progress is shown, the commands still work. */
    const ready = (): Promise<void> => {
      subscribed ??= backend.onJob(onEvent).then(
        () => undefined,
        () => undefined,
      );
      return subscribed;
    };

    const begin = (kind: JobKind, title: string): string => {
      const id = newId(kind);
      const now = Date.now();
      patchState(store, { jobs: [...store.jobs(), { id, kind, title, startedAt: now, lastEventAt: now, progress: null, cancelling: false }] });
      return id;
    };

    return {
      /** Runs a command that is itself the operation (it resolves when the operation ends). */
      async run<T>(kind: JobKind, title: string, work: (jobId: string) => Promise<T>): Promise<T> {
        await ready();
        const id = begin(kind, title);
        try {
          return await work(id);
        } finally {
          forget(id);
        }
      },
      /** Starts a command that returns at once while the operation goes on; the engine's end event removes it. */
      async start(kind: JobKind, title: string, work: (jobId: string) => Promise<void>): Promise<string> {
        await ready();
        const id = begin(kind, title);
        try {
          await work(id);
        } catch (e) {
          forget(id);
          throw e;
        }
        return id;
      },
      /** Asks the engine to stop a job. */
      async cancel(id: string): Promise<void> {
        patchState(store, { jobs: store.jobs().map((j) => (j.id === id ? { ...j, cancelling: true } : j)) });
        await backend.cancelJob(id).catch(() => undefined);
      },
    };
  }),
);
