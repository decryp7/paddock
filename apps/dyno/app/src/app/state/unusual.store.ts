import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, UnusualAction, UnusualItem, UnusualOverview } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

interface UnusualState {
  overview: UnusualOverview | null;
  includeIgnored: boolean;
  error: CommandError | null;
  /** The key of the program an action is running for. */
  busy: string | null;
}

/** New and unusual programs: the list the engine builds from its baseline, and the three things a person can do with an item. */
export const UnusualStore = signalStore(
  { providedIn: 'root' },
  withState<UnusualState>({ overview: null, includeIgnored: false, error: null, busy: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
    async load(): Promise<void> {
      try {
        patchState(store, { overview: await backend.unusual(store.includeIgnored()), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    async setIncludeIgnored(on: boolean): Promise<void> {
      patchState(store, { includeIgnored: on });
      await this.load();
    },
    /** Ignores, protects or restores `item`. The engine answers the new list. */
    async act(item: UnusualItem, action: UnusualAction): Promise<void> {
      if (store.busy()) return;
      patchState(store, { busy: item.key });
      try {
        // The answer hides ignored items; ask again when the person wants to see them.
        await backend.unusualAct(item.key, action);
        patchState(store, { busy: null });
        await this.load();
        notifier.show(
          action === 'protect' ? `${item.name} is protected: rules never touch it` : action === 'ignore' ? `${item.name} is hidden from this list` : `${item.name} is back on the list`,
        );
      } catch (e) {
        patchState(store, { busy: null });
        notifier.error(toCommandError(e));
      }
    },
    /** Forgets the baseline; the caller has asked the person first. */
    async forget(): Promise<void> {
      try {
        patchState(store, { overview: await backend.forgetBaseline(), error: null });
        notifier.show('Dyno forgot the programs it knew and starts learning again');
      } catch (e) {
        notifier.error(toCommandError(e));
      }
    },
  })),
);
