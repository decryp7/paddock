import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { ServiceCommandError, ServiceStatus, ServiceUpdate } from '../generated/service';
import { ServiceBackend, type Unlisten } from './backend';
import { asServiceError, serviceMessage } from './wire';

/** What the user can ask the service to do; the one that failed is what Retry runs again. */
export type ServiceAction = 'repair' | 'install' | 'openLoginItems';

/** Why the last request did not work, and which request it was. */
export interface ServiceFailure {
  readonly action: ServiceAction;
  readonly error: ServiceCommandError;
}

interface ServiceState {
  /** What the engine last said; null until the first answer. */
  status: ServiceStatus | null;
  /** The repair or install asked for and not yet answered by an update (the commands return at once). */
  requested: 'repair' | 'install' | null;
  failure: ServiceFailure | null;
}

/**
 * The background service as one window sees it (rule 34: the engine is the single source of truth). The store holds the
 * status the engine reported, folds the updates that arrive on `paddock://service` into it, and starts repair and install,
 * which return at once. Nothing here times out a running repair (rule 12): it shows "working" and its phase until the
 * engine says it is over. The page never depends on the service (rule 19): a failed status request is an inline error with
 * Retry, and the rest of the Settings window works.
 */
export const ServiceStore = signalStore(
  withState<ServiceState>({ status: null, requested: null, failure: null }),
  withRequestStatus(),
  withComputed(({ status, requested, failure }) => {
    const working = computed(() => requested() !== null || status()?.repair?.state === 'working' || status()?.condition === 'repairing');
    return {
      /** A repair or install is under way, here or in another window. */
      working,
      /** The phase the engine reports while it works, or null. */
      step: computed(() => {
        const repair = status()?.repair;
        return repair?.state === 'working' ? repair.step : null;
      }),
      /** The words of the repair notice (progress, outcome), or null. */
      notice: computed(() => status()?.repair ?? null),
      canRepair: computed(() => status()?.canRepair === true),
      canInstall: computed(() => status()?.canInstall === true),
      canOpenLoginItems: computed(() => status()?.canOpenLoginItems === true),
      /** The failure Retry can run again (a refusal that says trying again can help). */
      retryable: computed(() => (failure()?.error.retryable === true && failure()?.action !== 'openLoginItems' ? failure()!.action : null)),
    };
  }),
  withMethods((store, backend = inject(ServiceBackend)) => {
    const load = async (): Promise<void> => {
      // A refresh keeps the status on screen; only the first ask shows as loading.
      if (!store.status()) patchState(store, setLoading());
      try {
        const status = await backend.status();
        patchState(store, { status }, setLoaded());
      } catch (e) {
        patchState(store, setError(serviceMessage(e, $localize`:@@pd.service.no-status:The service could not be asked how it is doing.`)));
      }
    };
    const start = async (action: 'repair' | 'install'): Promise<void> => {
      if (store.working()) return;
      patchState(store, { requested: action, failure: null });
      try {
        await (action === 'repair' ? backend.repair() : backend.install());
      } catch (e) {
        const error = asServiceError(e) ?? { code: 'failed' as const, message: serviceMessage(e, $localize`:@@pd.service.refused:The request was not accepted.`), retryable: true };
        patchState(store, { requested: null, failure: { action, error } });
        // Busy means one runs already (another window): show where it is.
        if (error.code === 'busy') await load();
      }
    };
    return {
      /** Asks the engine how the service is doing (also the Retry of a failed ask). */
      load(): Promise<void> {
        return load();
      },
      /** Starts a repair; the outcome and its phases follow as updates. */
      repair(): Promise<void> {
        return start('repair');
      },
      /** Starts the install; the outcome follows as updates. */
      install(): Promise<void> {
        return start('install');
      },
      /** Opens the OS page of the login switch; the app never flips the switch itself. */
      async openLoginItems(): Promise<void> {
        patchState(store, { failure: null });
        try {
          if (!(await backend.openLoginItems())) {
            const message = $localize`:@@pd.service.no-login-items:This system has no Login Items page to open. Look for the app in its startup settings.`;
            patchState(store, { failure: { action: 'openLoginItems', error: { code: 'unavailable', message, retryable: false } } });
          }
        } catch (e) {
          const error = asServiceError(e) ?? { code: 'failed' as const, message: serviceMessage(e, $localize`:@@pd.service.login-items-failed:Login Items could not be opened.`), retryable: false };
          patchState(store, { failure: { action: 'openLoginItems', error } });
        }
      },
      /** Runs the failed request again. */
      async retry(): Promise<void> {
        const action = store.retryable();
        if (action === 'repair') await start('repair');
        else if (action === 'install') await start('install');
      },
      /** Hides the failure. */
      dismissFailure(): void {
        patchState(store, { failure: null });
      },
      /** An update from the engine's event: the status now, and why the request that ended here did not work. */
      _update(update: ServiceUpdate): void {
        // Another window may have asked: Retry then runs what fits the status.
        const action = store.requested() ?? (update.status.canInstall ? 'install' : 'repair');
        patchState(store, { status: update.status, requested: null, failure: update.error ? { action, error: update.error } : null }, setLoaded());
      },
    };
  }),
  withHooks((store, backend = inject(ServiceBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: an update can only follow something the status already shows.
        void backend
          .listen((u) => store._update(u))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-service] cannot listen for service updates: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type ServiceStoreType = InstanceType<typeof ServiceStore>;
