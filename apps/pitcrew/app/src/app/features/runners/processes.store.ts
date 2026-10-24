import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withProps, withState } from '@ngrx/signals';
import type { KeepFor, ProcessEntry, ProcessList } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ClockService } from '../../core/clock.service';
import { toCommandError } from '../../core/command-error';
import { idle, pending, settle, type Loadable } from '../../state/loadable';
import { StatusStore } from '../../state/status.store';
import { Notify } from '../../state/notify';

/** How often the Processes tab asks for a fresh list while it is visible. */
export const PROCESS_POLL_MS = 5_000;

interface ProcessesState {
  runner: string | null;
  list: Loadable<ProcessList>;
  /** The pid an action is running for. */
  busy: number | null;
  /** The engine's text when the last action was refused, shown inline next to the table. */
  actionError: string | null;
}

/** One runner's process tree and the leftovers of finished jobs, refreshed every few seconds; stop and keep actions. */
export const ProcessesStore = signalStore(
  withState<ProcessesState>({ runner: null, list: idle<ProcessList>(), busy: null, actionError: null }),
  withProps(() => ({ _poll: { id: null as ReturnType<typeof setInterval> | null } })),
  withMethods((store, backend = inject(BACKEND), clock = inject(ClockService), toasts = inject(Notify), status = inject(StatusStore)) => {
    const load = async (runner: string, silent = false): Promise<void> => {
      const same = store.runner() === runner;
      if (!silent || !same) patchState(store, { runner, list: pending(same ? store.list() : undefined), ...(same ? {} : { actionError: null }) });
      const list = await settle(same ? store.list() : undefined, () => backend.runnerProcesses(runner));
      if (store.runner() === runner) patchState(store, { list });
    };

    const stopPolling = () => {
      if (store._poll.id) clearInterval(store._poll.id);
      store._poll.id = null;
    };

    return {
      load,
      stopPolling,
      /** Loads the list and keeps it fresh while the window is visible. */
      watch(runner: string): void {
        stopPolling();
        void load(runner);
        store._poll.id = setInterval(() => {
          if (clock.visible()) void load(runner, true);
        }, PROCESS_POLL_MS);
      },
      retry(): Promise<void> {
        const r = store.runner();
        return r ? load(r) : Promise.resolve();
      },
      dismissError(): void {
        patchState(store, { actionError: null });
      },
      /** Asks the engine to stop a process of the runner's tree; `allowRunner` only for the runner process itself. */
      async stop(entry: ProcessEntry, allowRunner = false): Promise<boolean> {
        const runner = store.runner();
        if (!runner) return false;
        patchState(store, { busy: entry.pid, actionError: null });
        try {
          await backend.stopProcess(runner, entry.pid, entry.startedAt, allowRunner);
          toasts.show({ tone: 'success', icon: 'check-circle', title: `Stopped ${entry.name}`, body: `pid ${entry.pid}` });
          return true;
        } catch (e) {
          const message = toCommandError(e).message;
          patchState(store, { actionError: message });
          toasts.show({ tone: 'danger', icon: 'alert', title: `${entry.name} was not stopped`, body: message });
          return false;
        } finally {
          patchState(store, { busy: null });
          await Promise.all([load(runner, true), status.load()]);
        }
      },
      /** Adds time before the automatic cleanup stops a leftover. */
      async keep(entry: ProcessEntry, extra: KeepFor): Promise<boolean> {
        const runner = store.runner();
        if (!runner || entry.startedAt === null) return false;
        patchState(store, { busy: entry.pid, actionError: null });
        try {
          await backend.keepProcess(runner, entry.pid, entry.startedAt, extra);
          return true;
        } catch (e) {
          const message = toCommandError(e).message;
          patchState(store, { actionError: message });
          toasts.show({ tone: 'danger', icon: 'alert', title: `${entry.name} could not be kept`, body: message });
          return false;
        } finally {
          patchState(store, { busy: null });
          await load(runner, true);
        }
      },
    };
  }),
  withHooks({
    onDestroy(store) {
      if (store._poll.id) clearInterval(store._poll.id);
    },
  }),
);
