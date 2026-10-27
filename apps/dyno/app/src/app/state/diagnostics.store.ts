import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { BundlePreview, CommandError, SavedDiagnostics } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { formatBytes, plural } from '../shared/format';
import { Notifier } from '../shared/notifier';

interface DiagnosticsState {
  preview: BundlePreview | null;
  previewError: CommandError | null;
  saving: boolean;
  saved: SavedDiagnostics | null;
  saveError: CommandError | null;
}

/** The last path segment of a saved file, whichever separator the OS uses. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** The diagnostics bundle: what it would contain (nothing is written) and the result of saving it where the person chose. */
export const DiagnosticsStore = signalStore(
  { providedIn: 'root' },
  withState<DiagnosticsState>({ preview: null, previewError: null, saving: false, saved: null, saveError: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
    async loadPreview(): Promise<void> {
      try {
        patchState(store, { preview: await backend.diagnosticsPreview(), previewError: null });
      } catch (e) {
        patchState(store, { previewError: toCommandError(e) });
      }
    },
    /** Shows the OS save dialog through the shell; a cancelled dialog changes nothing. */
    async save(): Promise<void> {
      if (store.saving()) return;
      patchState(store, { saving: true, saveError: null });
      try {
        const saved = await backend.saveDiagnostics();
        patchState(store, { saving: false });
        if (saved) {
          patchState(store, { saved });
          notifier.show(`Saved ${fileName(saved.path)} (${plural(saved.files, 'file')}, ${formatBytes(saved.bytes)})`);
        }
      } catch (e) {
        patchState(store, { saving: false, saveError: toCommandError(e) });
      }
    },
  })),
);
