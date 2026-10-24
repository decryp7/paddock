import { inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { computed } from '@angular/core';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { KindView, PacksView } from '../generated/packs';
import { PacksBackend, type Unlisten } from './backend';
import { packsMessage } from './wire';

interface PacksState {
  /** What the engine last said; null until the first answer. */
  view: PacksView | null;
  /** A reload was asked for and not yet answered. */
  reloading: boolean;
  /** Why the last reload did not work, or null. */
  reloadError: string | null;
}

/**
 * The knowledge packs as one window sees them (rule 34: the engine is the single source of truth). The store holds the view
 * the engine reported and folds the changes that arrive on `paddock://packs` into it (a pack edited while the app runs shows
 * at once). Reload is the repair of a broken pack and never depends on one (rule 19): a failed ask is an inline error with
 * Retry. Nothing here times out a running reload (rule 12).
 */
export const PacksStore = signalStore(
  withState<PacksState>({ view: null, reloading: false, reloadError: null }),
  withRequestStatus(),
  withComputed(({ view }) => ({
    /** The kinds the app registered. */
    kinds: computed<readonly KindView[]>(() => view()?.kinds ?? []),
    /** The number of refused packs across the kinds. */
    problemCount: computed(() => (view()?.kinds ?? []).reduce((n, k) => n + k.problems.length, 0)),
  })),
  withMethods((store, backend = inject(PacksBackend)) => ({
    /** Asks the engine what is in use (also the Retry of a failed ask). */
    async load(): Promise<void> {
      if (!store.view()) patchState(store, setLoading());
      try {
        patchState(store, { view: await backend.status() }, setLoaded());
      } catch (e) {
        patchState(store, setError(packsMessage(e, $localize`:@@pd.packs.no-status:The packs could not be listed.`)));
      }
    },
    /** Reads the pack folders again. */
    async reload(): Promise<void> {
      if (store.reloading()) return;
      patchState(store, { reloading: true, reloadError: null });
      try {
        patchState(store, { view: await backend.reload(), reloading: false }, setLoaded());
      } catch (e) {
        patchState(store, { reloading: false, reloadError: packsMessage(e, $localize`:@@pd.packs.reload-failed:The packs could not be reloaded.`) });
      }
    },
    /** An update from the engine's event. */
    _update(view: PacksView): void {
      patchState(store, { view }, setLoaded());
    },
  })),
  withHooks((store, backend = inject(PacksBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: a change can only follow something the status already shows.
        void backend
          .listen((v) => store._update(v))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-packs] cannot listen for pack changes: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type PacksStoreType = InstanceType<typeof PacksStore>;
