import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, GeoSummary } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** The map asks again this often while it is open. */
export const MAP_POLL_MS = 3000;

interface MapState {
  summary: GeoSummary | null;
  error: CommandError | null;
}

/** The connections map's data, polled only while the page is open. */
export const MapStore = signalStore(
  { providedIn: 'root' },
  withState<MapState>({ summary: null, error: null }),
  withMethods((store, backend = inject(BACKEND), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let open = false;
    const stop = (): void => {
      open = false;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const load = async (): Promise<void> => {
      try {
        patchState(store, { summary: await backend.geoSummary(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
      if (open) timer = setTimeout(() => void load(), MAP_POLL_MS);
    };
    destroyRef?.onDestroy(stop);
    return {
      load,
      /** Starts following the connections (the first answer may start the location download). */
      start(): void {
        stop();
        open = true;
        void load();
      },
      stop,
    };
  }),
);
