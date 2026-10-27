import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { ActivityEntry, CommandError, TuningSuggestion } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { HealthStore } from './health.store';

interface TuningState {
  items: TuningSuggestion[];
  loaded: boolean;
  error: CommandError | null;
  /** Ids with a change in flight. */
  busy: string[];
}

/** The tuning catalogue: what Dyno suggests, and the changes it applied and can revert. Nothing changes until `apply` or `revert` is called from a click. */
export const TuningStore = signalStore(
  { providedIn: 'root' },
  withState<TuningState>({ items: [], loaded: false, error: null, busy: [] }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), activity = inject(ActivityStore), health = inject(HealthStore)) => {
    const busy = (id: string, on: boolean) => patchState(store, (s) => ({ busy: on ? [...s.busy, id] : s.busy.filter((x) => x !== id) }));
    const load = async (): Promise<void> => {
      patchState(store, { error: null });
      try {
        patchState(store, { items: await backend.tuning(), loaded: true });
      } catch (e) {
        const err = toCommandError(e);
        // An engine that does not offer tuning yet is an empty list, not a failure with a Retry that can never work.
        if (err.code === 'unsupported') patchState(store, { items: [], loaded: true });
        else patchState(store, { error: err });
      }
    };
    /** Runs one change, logs it, offers Undo, then refreshes the list and the health score it affects. */
    const change = async (id: string, run: () => Promise<ActivityEntry>): Promise<boolean> => {
      busy(id, true);
      try {
        const entry = await run();
        activity.receive(entry);
        notifier.show(entry.title, 'ok', entry.undo === 'available' ? { label: 'Undo', run: () => void activity.undo(entry.id).then(() => load()) } : null);
        await Promise.all([load(), health.loadReport()]);
        return true;
      } catch (e) {
        notifier.error(toCommandError(e));
        return false;
      } finally {
        busy(id, false);
      }
    };
    return {
      load,
      /** Applies a suggestion (the caller has shown its preview). */
      apply: (id: string) => change(id, () => backend.applyTuning(id)),
      /** Reverts a change Dyno applied; `force` overwrites a setting something else changed since. */
      revert: (id: string, force = false) => change(id, () => backend.revertTuning(id, force)),
    };
  }),
);
