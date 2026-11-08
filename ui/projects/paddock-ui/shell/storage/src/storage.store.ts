import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { toCommandError, type CommandError } from '@decryp7/paddock-ui/ipc';
import { errorMessage, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { StorageClearResult, StorageEvent, StorageTree, StorageUsage } from '../generated/storage';
import { StorageBackend, type StorageProgress, type Unlisten } from './backend';
import { clearRequest } from './wire';

/** What is running: a scan, or a clear. */
export type StorageJob = 'scan' | 'clear' | 'tree';

interface StorageState {
  /** What the engine last measured; null until the first answer. */
  usage: StorageUsage | null;
  /** The job asked for and not yet ended by an event (the commands return at once). */
  working: StorageJob | null;
  /** Where the job is, while it runs. */
  progress: StorageProgress | null;
  /** What the last clear did (also when it stopped part way), until dismissed. */
  result: StorageClearResult | null;
  /** Why the last request did not work (a refusal or a failed job), until the next request or dismissal. */
  failure: CommandError | null;
  /** The size trees the engine sent, by category; cleared whenever the figures are measured again. */
  trees: Readonly<Record<string, StorageTree>>;
  /** Cancel was pressed and the engine has not ended the job yet. */
  cancelling: boolean;
}

/**
 * What the app keeps on disk as one window sees it (rule 34: the engine is the single source of truth). It holds the usage the
 * engine measured, starts a scan or a clear (both return at once), follows the job's phase and its outcome event, and measures
 * again after a clear so the figures are never stale. Nothing here times out a running job (rule 12): it shows "working" and
 * the phase until the engine says it is over. A clear is only ever started with the confirmations the user gave (the engine
 * refuses data without one), and never without a scan in front of it.
 */
export const StorageStore = signalStore(
  withState<StorageState>({ usage: null, working: null, progress: null, result: null, failure: null, trees: {}, cancelling: false }),
  // A failed read retries itself; a snapshot event ends the error at once (ADR 0058).
  withRequestStatus({ retry: true }),
  withComputed(({ usage }) => ({
    /** The rows of the table. */
    categories: computed(() => usage()?.categories ?? []),
    /** The cache categories that clearing would free something in, and the bytes it frees. */
    cache: computed(() => {
      const rows = (usage()?.categories ?? []).filter((c) => c.kind === 'cache' && c.clearable && c.clearableItems > 0);
      return { ids: rows.map((c) => c.id), bytes: rows.reduce((n, c) => n + c.clearableBytes, 0) };
    }),
  })),
  withMethods((store, backend = inject(StorageBackend)) => {
    const start = async (job: StorageJob, action: () => Promise<void>): Promise<boolean> => {
      if (store.working()) return false;
      patchState(store, { working: job, failure: null, result: job === 'clear' ? null : store.result(), progress: null, cancelling: false });
      try {
        await action();
        return true;
      } catch (e) {
        patchState(store, { working: null, failure: toCommandError(e) });
        return false;
      }
    };
    const reads = store._reads.define({
      read: () => backend.last(),
      // Without a measurement yet, measure now (load waits until the scan started).
      apply: (usage: StorageUsage | null) => {
        if (usage) patchState(store, { usage });
        else return start('scan', () => backend.scan());
        return undefined;
      },
      message: (e) => errorMessage(e, $localize`:@@pd.storage.no-usage:What the app keeps could not be read.`),
      quiet: () => store.usage() !== null,
    });
    return {
      /** Shows the last measurement if the engine has one, else measures. Also the Retry of a failed ask. */
      async load(): Promise<void> {
        await reads.load();
      },
      /** Measures again. */
      scan(): Promise<boolean> {
        return start('scan', () => backend.scan());
      },
      /** Clears the chosen categories (the page asked the user first; `confirmed` is that answer for the user's data). */
      clear(choices: readonly { id: string; confirmed: boolean }[]): Promise<boolean> {
        return start('clear', () => backend.clear(clearRequest(choices)));
      },
      /** Walks one category's folder into a size tree (the outcome arrives as an event). */
      tree(id: string): Promise<boolean> {
        return start('tree', () => backend.tree(id));
      },
      /** Cancels the job in progress. What a clear already did stays done and is reported. */
      async cancel(): Promise<void> {
        if (!store.working()) return;
        patchState(store, { cancelling: true });
        try {
          await backend.cancel();
        } catch (e) {
          patchState(store, { cancelling: false, failure: toCommandError(e) });
        }
      },
      /** Shows a category's folder in the file manager. */
      async reveal(id: string): Promise<void> {
        patchState(store, { failure: null });
        try {
          await backend.reveal(id);
        } catch (e) {
          patchState(store, { failure: toCommandError(e) });
        }
      },
      /** Hides the failure. */
      dismissFailure(): void {
        patchState(store, { failure: null });
      },
      /** Hides what the last clear did. */
      dismissResult(): void {
        patchState(store, { result: null });
      },
      /** An outcome from the engine's event. */
      _event(event: StorageEvent): void {
        switch (event.state) {
          case 'usage':
            patchState(store, { usage: event.usage, working: null, progress: null, cancelling: false, trees: {} });
            reads.received();
            break;
          case 'cleared':
            patchState(store, { result: event.result, working: null, progress: null, cancelling: false, usage: null, trees: {} });
            // The figures are old now: measure again, so the table is right and the next clear has its preview.
            void start('scan', () => backend.scan());
            break;
          case 'tree':
            patchState(store, { trees: { ...store.trees(), [event.tree.id]: event.tree }, working: null, progress: null, cancelling: false });
            break;
          case 'cancelled':
            patchState(store, { working: null, progress: null, cancelling: false });
            break;
          case 'failed':
            patchState(store, { working: null, progress: null, cancelling: false, failure: { code: 'failed', message: event.message, retryable: true } });
            break;
        }
      },
      _progress(progress: StorageProgress | null): void {
        // Another window's job is this window's too: the table is busy until the engine says it is over.
        patchState(store, progress ? { progress, working: store.working() ?? 'scan' } : { progress: null });
      },
    };
  }),
  withHooks((store, backend = inject(StorageBackend)) => {
    const offs: Unlisten[] = [];
    let destroyed = false;
    const keep = (p: Promise<Unlisten>): void => {
      void p
        .then((off) => (destroyed ? off() : offs.push(off)))
        .catch((e: unknown) => console.warn(`[paddock-storage] cannot listen for storage events: ${errorMessage(e, 'unknown error')}`));
    };
    return {
      onInit() {
        // Listen first, then ask: an event can only follow something the figures already show.
        keep(backend.listen((e) => store._event(e)));
        keep(backend.listenProgress((p) => store._progress(p)));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        for (const off of offs) off();
      },
    };
  }),
);

export type StorageStoreType = InstanceType<typeof StorageStore>;
