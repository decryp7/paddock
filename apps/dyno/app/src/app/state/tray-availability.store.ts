import { DestroyRef, computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withState } from '@ngrx/signals';
import type { TrayAvailability } from '../core/api.types';
import { BACKEND } from '../core/backend';

interface TrayAvailabilityState {
  /** What the engine last said; null until it has answered. */
  availability: TrayAvailability | null;
}

/** The engine's note when the desktop shows no tray icons, else null. The UI shows it as it is. */
export function noHostNote(availability: TrayAvailability | null): string | null {
  return availability !== null && availability !== 'shown' ? availability.noHost.hint : null;
}

/**
 * Whether the desktop shows Dyno's tray icons, as the engine decided (Linux without a StatusNotifier host shows none). It reads the
 * state once and follows the engine's pushes, so the note appears when a host goes and disappears when one starts.
 */
export const TrayAvailabilityStore = signalStore(
  { providedIn: 'root' },
  withState<TrayAvailabilityState>({ availability: null }),
  withComputed(({ availability }) => ({
    /** The engine has answered. */
    loaded: computed(() => availability() !== null),
    /** What to tell the person, or null when the icons are shown (or not known yet). */
    note: computed(() => noHostNote(availability())),
  })),
  withHooks({
    onInit(store, backend = inject(BACKEND), destroyRef = inject(DestroyRef)) {
      // A push that arrives before the first answer is newer than the answer: it wins.
      let pushed = false;
      const stop = backend.onTrayAvailability((a) => {
        pushed = true;
        patchState(store, { availability: a });
      });
      destroyRef.onDestroy(stop);
      backend
        .trayAvailability()
        .then((a) => {
          if (!pushed) patchState(store, { availability: a });
        })
        .catch((e: unknown) => {
          // The icons' state is a note, not a feature: when it cannot be read there is nothing to show.
          console.error('tray_availability failed', e);
          if (!pushed) patchState(store, { availability: 'shown' });
        });
    },
  }),
);
