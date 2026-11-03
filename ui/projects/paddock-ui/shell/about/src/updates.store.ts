import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage, injectTrackLoad, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { Preferences, UpdateErrorDto, UpdateInfo, UpdateStatus } from '../generated/about';
import { UpdatesBackend, type Unlisten } from './backend';
import { toUpdateError } from './wire';

/** The notification kind of "a new version is available" (registered by the About contributions). */
export const UPDATE_AVAILABLE_KIND = 'pd.updates.available';

/** How a window's updates store behaves. */
export interface UpdatesConfig {
  /** Announce a newly found update as a notice (the main window; a dialog window does not). */
  readonly announce: boolean;
}

export const UPDATES_CONFIG = new InjectionToken<UpdatesConfig>('PADDOCK_UPDATES_CONFIG', {
  providedIn: 'root',
  factory: () => ({ announce: false }),
});

interface UpdatesState {
  /** The engine's status, newest first (events win over a command's reply that crossed them). */
  status: UpdateStatus | null;
  /** Why the last command was refused (the engine's error, shown with its next step). */
  actionError: UpdateErrorDto | null;
  /** The version last announced in this window. */
  announced: string | null;
  /** Events heard; a reply is applied only if none arrived while its command ran. */
  heard: number;
}

/**
 * The updater as one window sees it: the engine's `UpdateStatus` (the engine is the single source of truth, rule 34), the
 * request status of the first read, and the error of a refused step. Every step returns at once; the result arrives as
 * an event, and a long download is shown by its job, never timed out (rule 12).
 */
export const UpdatesStore = signalStore(
  withState<UpdatesState>({ status: null, actionError: null, announced: null, heard: 0 }),
  withRequestStatus(),
  withComputed(({ status, actionError }) => {
    const phase = computed(() => status()?.phase ?? null);
    return {
      phase,
      kind: computed(() => phase()?.kind ?? null),
      /** The update the phase is about. */
      info: computed<UpdateInfo | null>(() => {
        const p = phase();
        return p && 'info' in p ? p.info : null;
      }),
      /** The refused step's error, else the phase's failure. */
      error: computed<UpdateErrorDto | null>(() => {
        const p = phase();
        return actionError() ?? (p?.kind === 'failed' ? p.error : null);
      }),
      /** The job of the check or download in flight (its progress is in the Jobs list). */
      job: computed<number | null>(() => {
        const p = phase();
        return p?.kind === 'checking' || p?.kind === 'downloading' ? p.job : null;
      }),
      canCheck: computed(() => ['idle', 'upToDate', 'available', 'failed'].includes(phase()?.kind ?? '')),
      canDownload: computed(() => {
        const p = phase();
        return p?.kind === 'available' || (p?.kind === 'failed' && p.info !== null && p.error.kind !== 'signatureInvalid' && p.error.kind !== 'tooLarge');
      }),
    };
  }),
  withMethods((store, backend = inject(UpdatesBackend), ports = inject(SHELL_PORTS), config = inject(UPDATES_CONFIG)) => {
    const apply = (status: UpdateStatus): void => {
      patchState(store, { status });
      const p = status.phase;
      if (config.announce && p.kind === 'available' && store.announced() !== p.info.version) {
        patchState(store, { announced: p.info.version });
        const message = $localize`:@@pd.updates.notice.available:Version ${p.info.version}:version: is available (you have ${p.info.currentVersion}:current:).`;
        ports.notify({ level: 'info', kind: UPDATE_AVAILABLE_KIND, message });
      }
    };

    /** Runs one step; its reply is applied unless an event arrived meanwhile (that event is newer). */
    const step = async (call: () => Promise<UpdateStatus>): Promise<boolean> => {
      const heard = store.heard();
      patchState(store, { actionError: null });
      try {
        const status = await call();
        if (store.heard() === heard) apply(status);
        return true;
      } catch (e) {
        patchState(store, { actionError: toUpdateError(e) });
        return false;
      }
    };

    return {
      /** Reads the status (also the Retry of a failed read). */
      async load(): Promise<void> {
        const heard = store.heard();
        patchState(store, setLoading());
        try {
          const status = await backend.status();
          if (store.heard() === heard) apply(status);
          patchState(store, setLoaded());
        } catch (e) {
          patchState(store, setError(errorMessage(e, $localize`:@@pd.updates.no-answer:The updater did not answer.`)));
        }
      },
      check: () => step(() => backend.check()),
      download: () => step(() => backend.download()),
      cancel: () => step(() => backend.cancel()),
      installOnQuit: (enabled: boolean) => step(() => backend.installOnQuit(enabled)),
      restart: () => step(() => backend.restart()),
      setPreferences: (preferences: Preferences) => step(() => backend.setPreferences(preferences)),
      /** A status from the engine's event. */
      _receive(status: UpdateStatus): void {
        patchState(store, { heard: store.heard() + 1 });
        apply(status);
      },
    };
  }),
  withHooks((store, backend = inject(UpdatesBackend), trackLoad = injectTrackLoad()) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then read: an event during the read wins over the read's reply.
        // A pending task: the updates window is settled (sized to the status) only after the read ends.
        void trackLoad(() =>
          Promise.all([
            backend
              .listen((s) => store._receive(s))
              .then((u) => (destroyed ? u() : (off = u)))
              .catch((e: unknown) => console.warn(`[paddock-updates] cannot listen for update events: ${errorMessage(e, 'unknown error')}`)),
            store.load(),
          ]),
        );
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type UpdatesStoreType = InstanceType<typeof UpdatesStore>;
