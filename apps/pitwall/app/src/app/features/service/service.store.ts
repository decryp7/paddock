import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { AppUpdateStatus, CommandError, ServiceInstall, UpdateInfo } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

interface ServiceState {
  /** The server's OS registration; null until the first answer (or when it never came). */
  service: ServiceInstall | null;
  /** Whether Pitwall starts at log-in; null while unknown. */
  autostart: boolean | null;
  updateStatus: AppUpdateStatus | null;
  /** The update the last check found. */
  update: UpdateInfo | null;
  /** A check ran and found nothing newer. */
  upToDate: boolean;
  /** Actions in flight: `install`, `uninstall`, `autostart`, `check`, `apply`. */
  busy: Record<string, true>;
  error: CommandError | null;
}

const initial: ServiceState = { service: null, autostart: null, updateStatus: null, update: null, upToDate: false, busy: {}, error: null };

/**
 * The server as an OS service, start at log-in and app updates. Everything here is optional for the app to work, so
 * nothing blocks on it: `refresh` starts in the background and a failure only leaves the cards saying so.
 */
export const ServiceStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withComputed(({ service, busy }) => ({
    /** Show the first-run "run Pitwall's service?" question. */
    askConsent: computed(() => service()?.askConsent === true),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    /** Runs one action: busy flag while it runs, the error recorded and toasted when it fails. Returns the result or null. */
    async function act<T>(key: string, run: () => Promise<T>): Promise<T | null> {
      if (key in store.busy()) return null;
      setBusy(key, true);
      patchState(store, { error: null });
      try {
        return await run();
      } catch (e) {
        const error = toCommandError(e);
        patchState(store, { error });
        toasts.error(error);
        return null;
      } finally {
        setBusy(key, false);
      }
    }

    return {
      /** Reads the service, start-at-log-in and update state; each part on its own so one failure hides nothing else. */
      async refresh(): Promise<void> {
        const [service, autostart, updateStatus] = await Promise.allSettled([backend.serviceState(), backend.autostart(), backend.appUpdateStatus()]);
        patchState(store, {
          service: service.status === 'fulfilled' ? service.value : store.service(),
          autostart: autostart.status === 'fulfilled' ? autostart.value : store.autostart(),
          updateStatus: updateStatus.status === 'fulfilled' ? updateStatus.value : store.updateStatus(),
          error: service.status === 'rejected' ? toCommandError(service.reason) : null,
        });
      },

      /** The user said yes to the service. */
      async install(): Promise<boolean> {
        const s = await act('install', () => backend.installService());
        if (s) {
          patchState(store, { service: s });
          toasts.ok(s.running ? 'Pitwall now runs as a service.' : 'The service is installed; it is starting.');
        }
        return s !== null;
      },

      async uninstall(): Promise<boolean> {
        const s = await act('uninstall', () => backend.uninstallService());
        if (s) {
          patchState(store, { service: s });
          toasts.ok('The service was removed.');
        }
        return s !== null;
      },

      /** The user said no: remember it and stop asking. */
      async decline(): Promise<void> {
        const done = await act('decline', () => backend.declineService().then(() => true));
        const s = store.service();
        if (done && s) patchState(store, { service: { ...s, askConsent: false } });
      },

      /** Hides the question for now without remembering an answer (asked again next time). */
      later(): void {
        const s = store.service();
        if (s) patchState(store, { service: { ...s, askConsent: false } });
      },

      async setAutostart(enabled: boolean): Promise<void> {
        const now = await act('autostart', () => backend.setAutostart(enabled));
        if (now !== null) patchState(store, { autostart: now });
      },

      async checkUpdate(): Promise<void> {
        const found = await act('check', async () => ({ info: await backend.checkAppUpdate() }));
        if (found) patchState(store, { update: found.info, upToDate: found.info === null });
      },

      /** Installs the update the check found; the app restarts when it works. */
      async applyUpdate(): Promise<void> {
        if (!store.update()) return;
        await act('apply', () => backend.installAppUpdate());
      },
    };
  }),
);

export type ServiceStore = InstanceType<typeof ServiceStore>;
