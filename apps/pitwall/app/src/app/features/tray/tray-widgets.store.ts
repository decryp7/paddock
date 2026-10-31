import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import type { LoadingRows, PopoverHeader } from '../../core/generated/widgets';
import type { PopoverEdit, PopoverValues, PopoverView } from '../../core/widgets.types';

interface TrayWidgetsState {
  /** How many rows each kind draws, from the engine: a loading twin is as tall as a ready widget. */
  shape: LoadingRows[];
  /** The status pill and the footer menu flags; null before the first answer. */
  header: PopoverHeader | null;
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
 * The tray popup's widgets (family ADR 0035): the layout, the values and the header, all the app's (computed in Rust). The popup
 * and the Settings page's chooser and preview read the same store; the app pushes a new snapshot after every change of the
 * server's state or of the layout, so neither window polls. An edit is sent to the app and its answer replaces the layout: the UI
 * holds no layout logic.
 */
export const TrayWidgetsStore = signalStore(
  { providedIn: 'root' },
  withState<TrayWidgetsState>({ shape: [], header: null, view: null, values: null, error: null, loading: false }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    let started = false;
    /** How many pushes arrived: a read that began before a push must not overwrite it with older data. */
    let pushes = 0;

    const load = async (): Promise<void> => {
      patchState(store, { loading: true });
      const seen = pushes;
      try {
        const [view, values, shape, header] = await Promise.all([backend.popoverView(), backend.popoverValues(), backend.popoverShape(), backend.popoverHeader()]);
        // A push that came while the read ran is newer than what the read answered: keep it, take only the shape (which a push lacks).
        if (pushes !== seen) patchState(store, { shape, error: null, loading: false });
        else patchState(store, { view, values, shape, header, error: null, loading: false });
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
        backend.onPopover((push) => {
          pushes++;
          patchState(store, { view: push.view, values: push.values, header: push.header, error: null });
        });
      },
      /** Sends one edit to the app; the answer is the new layout. Returns whether it was accepted. */
      async edit(edit: PopoverEdit): Promise<boolean> {
        try {
          patchState(store, { view: await backend.editPopover(edit), error: null });
          // The values follow the layout; the push also brings them, this keeps the preview right without waiting for it.
          const seen = pushes;
          const values = await backend.popoverValues();
          if (pushes === seen) patchState(store, { values });
          return true;
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
          return false;
        }
      },
    };
  }),
);
