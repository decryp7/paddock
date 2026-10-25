import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, FileHashInfo, HashSource, SearchSeed } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface LookupState {
  seed: SearchSeed | null;
  hash: FileHashInfo | null;
  /** A request is in flight (reading the seed or the file, or opening the browser). */
  busy: boolean;
  error: CommandError | null;
}

const initial: LookupState = { seed: null, hash: null, busy: false, error: null };

/**
 * The state of the two "look it up" dialogs: the redacted search text, and a file's SHA-256. The engine builds both and the
 * browser addresses; the shell opens the browser on the click. One dialog is open at a time, so one store serves both.
 */
export const LookupStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withMethods((store, backend = inject(BACKEND)) => {
    const run = async <T>(body: () => Promise<T>): Promise<T | null> => {
      patchState(store, { busy: true, error: null });
      try {
        return await body();
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
        return null;
      } finally {
        patchState(store, { busy: false });
      }
    };
    return {
      reset(): void {
        patchState(store, initial);
      },
      /** Reads the redacted starting text for a search about `text`. */
      async loadSeed(text: string): Promise<void> {
        patchState(store, { seed: null });
        const seed = await run(() => backend.searchSeed(text));
        if (seed) patchState(store, { seed });
      },
      /** Opens a search for exactly `query` in the browser; true when it opened. */
      async search(query: string): Promise<boolean> {
        return (await run(() => backend.webSearch(query).then(() => true))) === true;
      },
      /** Computes the SHA-256 of the file (nothing is uploaded). */
      async loadHash(source: HashSource): Promise<void> {
        patchState(store, { hash: null });
        const hash = await run(() => backend.fileHash(source));
        if (hash) patchState(store, { hash });
      },
      /** Opens the VirusTotal page of the hash in the browser; true when it opened. */
      async openVirusTotal(sha256: string): Promise<boolean> {
        return (await run(() => backend.openVirusTotal(sha256).then(() => true))) === true;
      },
    };
  }),
);
