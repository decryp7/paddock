import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AddWatchedFolder, CommandError, WatchChange, WatchOverview, WatchPreview } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

interface WatchedState {
  overview: WatchOverview | null;
  error: CommandError | null;
  /** The id of the folder a request is running for (or `add`). */
  busy: string | null;
  /** The preview being shown, and an error from fetching it. */
  preview: WatchPreview | null;
  previewError: CommandError | null;
}

const initial: WatchedState = { overview: null, error: null, busy: null, preview: null, previewError: null };

/**
 * Watched folders: the list the engine keeps, and the few things a person can do with it. Adding opens the OS folder dialog (in
 * the shell), turning a folder on needs its preview first, and the engine enforces both. Removal never touches files.
 */
export const WatchedStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => {
    const apply = async (id: string, body: () => Promise<WatchOverview>, done?: string): Promise<boolean> => {
      if (store.busy()) return false;
      patchState(store, { busy: id });
      try {
        patchState(store, { overview: await body(), error: null });
        if (done) notifier.show(done);
        return true;
      } catch (e) {
        notifier.error(toCommandError(e));
        return false;
      } finally {
        patchState(store, { busy: null });
      }
    };
    return {
      async load(): Promise<void> {
        try {
          patchState(store, { overview: await backend.watchedFolders(), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      /** Opens the folder dialog and adds the choice, switched off. False when they cancelled or it failed. */
      async add(request: AddWatchedFolder): Promise<boolean> {
        if (store.busy()) return false;
        patchState(store, { busy: 'add' });
        try {
          const o = await backend.addWatchedFolder(request);
          if (o) patchState(store, { overview: o, error: null });
          return o !== null;
        } catch (e) {
          notifier.error(toCommandError(e));
          return false;
        } finally {
          patchState(store, { busy: null });
        }
      },
      change(id: string, change: WatchChange, done?: string): Promise<boolean> {
        return apply(id, () => backend.setWatchedFolder(id, change), done);
      },
      remove(id: string): Promise<boolean> {
        return apply(id, () => backend.removeWatchedFolder(id), 'Stopped watching the folder. Its files were not touched');
      },
      /** Moves what is due now to the quarantine (the caller has shown the preview). */
      sweep(id: string): Promise<boolean> {
        return apply(id, () => backend.sweepWatchedFolder(id), 'Swept the folder');
      },
      /** The dry run for the dialog; the engine remembers that it was shown. */
      async loadPreview(id: string): Promise<void> {
        patchState(store, { preview: null, previewError: null });
        try {
          patchState(store, { preview: await backend.previewWatchedFolder(id) });
          // The folder is now "previewed": refresh so the list can offer Turn on.
          await this.load();
        } catch (e) {
          patchState(store, { previewError: toCommandError(e) });
        }
      },
      closePreview(): void {
        patchState(store, { preview: null, previewError: null });
      },
    };
  }),
);
