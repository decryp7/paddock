import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { toCommandError, type CommandError } from '@decryp7/paddock-ui/ipc';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { AppFolder, PickRequest, RecentLists } from '../generated/files';
import { FilesBackend, type Unlisten } from './backend';
import { entriesOf } from './wire';

interface FilesState {
  /** The recent lists the engine last sent; null until the first answer. */
  lists: RecentLists | null;
  /** Why the last action did not work (a refusal in words for the user), until the next action or dismissal. */
  failure: CommandError | null;
  /** The paths dropped on this window (already granted by the engine), newest drop last; the page takes them with `takeDropped`. */
  dropped: readonly string[];
}

/**
 * The user's files as one window sees them (rule 34: the engine is the single source of truth). The store holds the recent
 * lists the engine reported and keeps them current from its change event, starts the pickers and the reveal, open and
 * re-open actions, and keeps the last refusal for an inline message (never a dialog, never a thrown error). A path reaches
 * the page only from the engine (`pick`, a drop, a recent entry); it is never built in TypeScript.
 */
export const FilesStore = signalStore(
  withState<FilesState>({ lists: null, failure: null, dropped: [] }),
  withRequestStatus(),
  withComputed(({ lists }) => ({
    /** The recent files and folders, newest first. */
    files: computed(() => entriesOf(lists(), 'files')),
    /** The recent projects, newest first. */
    projects: computed(() => entriesOf(lists(), 'projects')),
  })),
  withMethods((store, backend = inject(FilesBackend)) => {
    /** Runs one action; a refusal becomes the inline failure and the result is `fallback`. */
    const attempt = async <T>(action: () => Promise<T>, fallback: T): Promise<T> => {
      patchState(store, { failure: null });
      try {
        return await action();
      } catch (e) {
        patchState(store, { failure: toCommandError(e) });
        return fallback;
      }
    };
    return {
      /** Entries of any list the app declared. */
      entries(list: string) {
        return entriesOf(store.lists(), list);
      },
      /** Asks the engine for the lists (also the Retry of a failed ask). */
      async load(): Promise<void> {
        if (!store.lists()) patchState(store, setLoading());
        try {
          patchState(store, { lists: await backend.recent() }, setLoaded());
        } catch (e) {
          patchState(store, setError(errorMessage(e, $localize`:@@pd.files.no-recent:The recent files could not be read.`)));
        }
      },
      /** Shows the native dialog; the chosen paths, or none when it was closed or refused. */
      pick(request: PickRequest): Promise<string[]> {
        return attempt(() => backend.pick(request), [] as string[]);
      },
      /** Grants a recent entry to this window again and returns its path, or null when it cannot be used any more. */
      reopen(list: string, path: string): Promise<string | null> {
        return attempt<string | null>(() => backend.regrant(list, path), null);
      },
      /** Takes one entry out of its list. */
      async remove(list: string, path: string): Promise<void> {
        await attempt(() => backend.removeRecent(list, path), undefined);
      },
      /** Empties a list. */
      async clear(list: string): Promise<void> {
        await attempt(() => backend.clearRecent(list), undefined);
      },
      /** Shows a path in the file manager; whether it worked. */
      reveal(path: string): Promise<boolean> {
        return attempt(() => backend.reveal(path).then(() => true), false);
      },
      /** Opens a path with the OS's program for it; whether it worked. */
      open(path: string): Promise<boolean> {
        return attempt(() => backend.open(path).then(() => true), false);
      },
      /** Shows one of the app's own folders; whether it worked. */
      openAppFolder(folder: AppFolder): Promise<boolean> {
        return attempt(() => backend.openAppFolder(folder).then(() => true), false);
      },
      /** Opens a web address in the browser; whether it worked. */
      openUrl(url: string): Promise<boolean> {
        return attempt(() => backend.openUrl(url).then(() => true), false);
      },
      /** Hides the failure. */
      dismissFailure(): void {
        patchState(store, { failure: null });
      },
      /** The paths dropped since the last call (the page opens them), and forgets them. */
      takeDropped(): readonly string[] {
        const taken = store.dropped();
        patchState(store, { dropped: [] });
        return taken;
      },
      _lists(lists: RecentLists): void {
        patchState(store, { lists }, setLoaded());
      },
      _dropped(paths: readonly string[]): void {
        patchState(store, { dropped: [...store.dropped(), ...paths] });
      },
    };
  }),
  withHooks((store, backend = inject(FilesBackend)) => {
    const offs: Unlisten[] = [];
    let destroyed = false;
    const keep = (p: Promise<Unlisten>): void => {
      void p
        .then((off) => (destroyed ? off() : offs.push(off)))
        .catch((e: unknown) => console.warn(`[paddock-files] cannot listen for file events: ${errorMessage(e, 'unknown error')}`));
    };
    return {
      onInit() {
        // Listen first, then ask: a change can only follow something the lists already show.
        keep(backend.listenRecent((lists) => store._lists(lists)));
        keep(backend.listenDropped((d) => store._dropped(d.paths)));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        for (const off of offs) off();
      },
    };
  }),
);

export type FilesStoreType = InstanceType<typeof FilesStore>;
