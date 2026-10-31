import type { LoadingRows } from '../core/generated/widgets';
import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import type { PopoverEdit, PopoverValues, PopoverView } from '../core/widgets.types';

interface TrayWidgetsState {
  /** How many rows each kind draws, from the engine: a loading twin is as tall as a ready widget. */
  shape: LoadingRows[];
  /** The layout the chooser draws; null before the first answer. */
  view: PopoverView | null;
  /** What the widgets show; null before the first answer (every widget is loading). */
  values: PopoverValues | null;
  /** The last read or edit that failed; cleared by the next success. */
  error: CommandError | null;
  /** A read is running. */
  loading: boolean;
}

/**
 * The tray popup's widgets (family ADR 0035): the layout and the values, both the engine's. The popup and the Settings
 * page's chooser and preview read the same store; the engine pushes a new layout and new values after every change of the
 * watches or the settings, so neither window polls. An edit is sent to the engine and its answer replaces the layout:
 * the UI holds no layout logic.
 */
export const TrayWidgetsStore = signalStore(
  { providedIn: 'root' },
  withState<TrayWidgetsState>({ shape: [], view: null, values: null, error: null, loading: false }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    let started = false;

    const load = async (): Promise<void> => {
      patchState(store, { loading: true });
      try {
        const [view, values, shape] = await Promise.all([backend.popoverView(), backend.popoverValues(), backend.popoverShape()]);
        patchState(store, { view, values, shape, error: null, loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    };

    return {
      load,
      /** Loads and subscribes in the background; never awaited before the first paint. */
      start(): void {
        if (started) return;
        started = true;
        void load();
        backend.onState((push) => patchState(store, { view: push.popover, values: push.popoverValues, error: null })).catch(() => undefined);
      },
      /** Sends one edit to the engine; the answer is the new layout. Returns whether it was accepted. */
      async edit(edit: PopoverEdit): Promise<boolean> {
        try {
          patchState(store, { view: await backend.editPopover(edit), error: null });
          // The values follow the layout; the push also brings them, this keeps the preview right without waiting for it.
          patchState(store, { values: await backend.popoverValues() });
          return true;
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
          return false;
        }
      },
    };
  }),
);
