import { Injector, computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, FileHashInfo, HashSource, SearchSeed } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';

interface LookupState {
  seed: SearchSeed | null;
  hash: FileHashInfo | null;
  /** A request is in flight (reading the seed or the file, or opening the browser). */
  busy: boolean;
  error: CommandError | null;
  /** Reading the file runs as a job (a program can be large): its phase, its bytes so far, and whether it has stood still for a while. */
  hashJobId: number | null;
}

const initial: LookupState = { seed: null, hash: null, busy: false, error: null, hashJobId: null };

/**
 * The state of the two "look it up" dialogs: the redacted search text, and a file's SHA-256. The engine builds both and the
 * browser addresses; the shell opens the browser on the click. One dialog is open at a time, so one store serves both.
 */
export const LookupStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withComputed(({ hashJobId }, injector = inject(Injector)) => ({
    /** The running read of the file: its phase, its bytes so far, and whether it has been quiet long enough to say "Still working". */
    // The shell's jobs store is only asked for once a file is being read: the web search dialog has none and needs none.
    hashJob: computed(() => {
      const id = hashJobId();
      return id === null ? null : progressOf(injector.get(JobsStore), id);
    }),
  })),
  withMethods((store, backend = inject(BACKEND), starter = inject(JobStarter)) => {
    let hashing: JobRun | null = null;
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
        // A file still being read is dropped with the dialog: the engine stops reading it.
        const reading = hashing;
        hashing = null;
        void reading?.cancel().catch(() => undefined);
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
      /** Computes the SHA-256 of the file (nothing is uploaded) as a job; a cancelled read leaves no hash and no error. */
      async loadHash(source: HashSource): Promise<void> {
        patchState(store, { hash: null, hashJobId: null });
        const hash = await run(async () => {
          const job = (hashing = starter.run({ fileHash: { source } }, (hashJobId) => patchState(store, { hashJobId })));
          try {
            const out = await job.result;
            // The dialog was closed or reopened meanwhile: this answer belongs to nobody.
            if (hashing !== job) return null;
            if (out === null) throw { code: 'internal', message: 'Reading the file was stopped. Open VirusTotal reads it again.' } satisfies CommandError;
            return 'fileHash' in out ? out.fileHash : null;
          } finally {
            if (hashing === job) {
              hashing = null;
              patchState(store, { hashJobId: null });
            }
          }
        });
        if (hash) patchState(store, { hash });
      },
      /** Stops reading the file. */
      async cancelHash(): Promise<void> {
        try {
          await hashing?.cancel();
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      /** Opens the VirusTotal page of the hash in the browser; true when it opened. */
      async openVirusTotal(sha256: string): Promise<boolean> {
        return (await run(() => backend.openVirusTotal(sha256).then(() => true))) === true;
      },
    };
  }),
);
