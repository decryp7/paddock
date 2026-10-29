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

/** How often the state is read again while the repair works. */
export const REPAIR_POLL_MS = 3000;

/** How often the state is read again while the server is down on macOS, and how many times. */
export const DOWN_POLL_MS = 5000;
export const MAX_DOWN_CHECKS = 24;

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
    /** The Repair button works (macOS, not switched off in Login Items, the server is not up). */
    canRepair: computed(() => {
      const s = service();
      return s !== null && s.platform === 'macos' && !s.disabledInLoginItems && !s.running && s.repair?.state !== 'working';
    }),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    let poll: ReturnType<typeof setTimeout> | null = null;
    let downChecks = 0;
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
        // A cancel the user asked for is an outcome, not a failure: no stored error.
        if (error.code !== 'cancelled') patchState(store, { error });
        toasts.error(error);
        return null;
      } finally {
        setBusy(key, false);
      }
    }

    /** Reads the service, start-at-log-in and update state; each part on its own so one failure hides nothing else. */
    async function refresh(): Promise<void> {
      if (poll !== null) clearTimeout(poll);
      poll = null;
      const before = store.service()?.repair?.state ?? null;
      const [service, autostart, updateStatus] = await Promise.allSettled([backend.serviceState(), backend.autostart(), backend.appUpdateStatus()]);
      patchState(store, {
        service: service.status === 'fulfilled' ? service.value : store.service(),
        autostart: autostart.status === 'fulfilled' ? autostart.value : store.autostart(),
        updateStatus: updateStatus.status === 'fulfilled' ? updateStatus.value : store.updateStatus(),
        error: service.status === 'rejected' ? toCommandError(service.reason) : null,
      });
      const repair = store.service()?.repair ?? null;
      if (repair?.state === 'working') {
        // The repair runs in the background and can take a minute: look again until it ends.
        poll = setTimeout(() => void refresh(), REPAIR_POLL_MS);
      } else if (store.service()?.platform === 'macos' && store.service()?.running === false && !store.service()?.disabledInLoginItems && downChecks < MAX_DOWN_CHECKS) {
        // Down on macOS: the app's own repair may start any moment (version change, launchd refusing the job), so look again.
        downChecks += 1;
        poll = setTimeout(() => void refresh(), DOWN_POLL_MS);
      } else if (before === 'working' && repair) {
        if (repair.state === 'succeeded') toasts.ok(repair.message);
        else toasts.show(`The background service could not be re-registered. ${repair.message}`, 'bad', 10000);
      }
    }

    return {
      refresh,
      /** Opens System Settings > General > Login Items; the user switches the item (Pitwall never does). */
      async openLoginItems(): Promise<void> {
        await act('loginItems', () => backend.openLoginItems());
      },

      /** Repairs the background service (macOS): the app re-registers it with macOS in the background; progress shows in the state. Needs no server. */
      async repair(): Promise<void> {
        const s = await act('repair', () => backend.repairService());
        if (s) {
          patchState(store, { service: s });
          downChecks = 0;
          void refresh();
        }
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
