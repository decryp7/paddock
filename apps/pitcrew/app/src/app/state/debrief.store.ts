import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { DebriefStatus } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { ToastStore } from './toast.store';

interface DebriefState {
  status: DebriefStatus | null;
  busy: boolean;
}

/**
 * The hand-off of a job's log to Debrief, the owner's log-analysis app (ADR 0018). Without Debrief the engine still
 * writes the extract, and the toast says where it is and what to do; nothing here is an error screen.
 */
export const DebriefStore = signalStore(
  { providedIn: 'root' },
  withState<DebriefState>({ status: null, busy: false }),
  withMethods((store, backend = inject(BACKEND), toasts = inject(ToastStore)) => ({
    /** Looks for Debrief once (cheap; call from the places that show the button). */
    async load(): Promise<void> {
      if (store.status() !== null) return;
      try {
        patchState(store, { status: await backend.debriefStatus() });
      } catch {
        patchState(store, { status: { installed: false, program: null, needsUpdate: false, cliName: 'debrief' } });
      }
    },
    /** Opens the runner's log (the whole file, or one job's part when both times are given) in Debrief. */
    async open(runner: string, label: string, startedAt: number | null = null, finishedAt: number | null = null): Promise<void> {
      patchState(store, { busy: true });
      try {
        const r = await backend.openInDebrief(runner, label, startedAt, finishedAt);
        if (r.result === 'opened') toasts.show({ tone: 'success', icon: 'check-circle', title: 'Sent to Debrief', body: r.file });
        else if (r.result === 'notInstalled') toasts.show({ tone: 'warning', icon: 'info', title: store.status()?.needsUpdate ? 'Update Debrief to use Open in Debrief' : 'Debrief is not installed', body: `${r.message} The log is saved at ${r.file}.` });
        else toasts.show({ tone: 'warning', icon: 'info', title: 'Nothing to open', body: r.message });
      } catch (e) {
        toasts.show({ tone: 'danger', icon: 'alert', title: 'Could not open Debrief', body: toCommandError(e).message });
      } finally {
        patchState(store, { busy: false });
      }
    },
  })),
);
