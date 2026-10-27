import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { WatchDetail } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

interface DetailState {
  id: string | null;
  detail: Loadable<WatchDetail>;
}

/** One watch's detail page: loaded by id, refreshed whenever the watch list is pushed. Provided per page. */
export const WatchDetailStore = signalStore(
  withState<DetailState>({ id: null, detail: idle<WatchDetail>() }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    return {
      async load(id: string): Promise<void> {
        const keep = store.id() === id ? store.detail() : idle<WatchDetail>();
        patchState(store, { id, detail: pending(keep) });
        const next = await settle(keep, () => backend.getWatch(id));
        // A slow answer for a watch the user already left is dropped.
        if (store.id() === id) patchState(store, { detail: next });
      },
      reload(): Promise<void> {
        const id = store.id();
        return id ? this.load(id) : Promise.resolve();
      },
    };
  }),
);
