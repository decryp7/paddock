import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage, injectTrackLoad, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
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
  // A failed read retries itself; a status event (the whole status) ends the error at once (ADR 0058).
  withRequestStatus({ retry: true }),
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
      /** Whether this build installs what it downloads; one that only notifies offers no Download, Restart or install on quit. */
      canInstall: computed(() => status()?.canInstall ?? false),
      canDownload: computed(() => {
        const p = phase();
        if (!status()?.canInstall) return false;
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

    /** The status read; its reply is applied unless an event arrived while it ran (that event is newer). */
    const reads = store._reads.define({
      read: () => {
        const heard = store.heard();
        return backend.status().then((status) => ({ status, heard }));
      },
      apply: ({ status, heard }: { status: UpdateStatus; heard: number }) => {
        if (store.heard() === heard) apply(status);
      },
      message: (e) => errorMessage(e, $localize`:@@pd.updates.no-answer:The updater did not answer.`),
    });

    /** Runs one step; its reply is applied unless an event arrived meanwhile (that event is newer). */
    const step = async (call: () => Promise<UpdateStatus>): Promise<boolean> => {
      const heard = store.heard();
      patchState(store, { actionError: null });
      try {
        const status = await call();
        if (store.heard() === heard) apply(status);
        // The reply is the whole status: the updater answers, so a failed read's error ends.
        reads.received();
        return true;
      } catch (e) {
        patchState(store, { actionError: toUpdateError(e) });
        return false;
      }
    };

    return {
      /** Reads the status (also the Retry of a failed read). */
      async load(): Promise<void> {
        await reads.load();
      },
      check: () => step(() => backend.check()),
      download: () => step(() => backend.download()),
      /** After a failed per-machine hand-off: looks for the update again and, when it is there, downloads it (the verified
       * download is not kept across the restart); the Install button comes back when it is ready. */
      async installAgain(): Promise<boolean> {
        if (!(await step(() => backend.check()))) return false;
        return store.phase()?.kind === 'available' ? step(() => backend.download()) : false;
      },
      cancel: () => step(() => backend.cancel()),
      installOnQuit: (enabled: boolean) => step(() => backend.installOnQuit(enabled)),
      restart: () => step(() => backend.restart()),
      setPreferences: (preferences: Preferences) => step(() => backend.setPreferences(preferences)),
      /** A status from the engine's event. */
      _receive(status: UpdateStatus): void {
        patchState(store, { heard: store.heard() + 1 });
        apply(status);
        reads.received();
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
