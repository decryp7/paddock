import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { ServiceInstall } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { idle, pending, settle, type Loadable } from './loadable';
import { StatusStore } from './status.store';
import { Notify } from './notify';

/** How often the state is read again while the repair works. */
export const REPAIR_POLL_MS = 3000;
/** How often the state is read again while the service is down on macOS, and how many times (the app may repair it by itself). */
export const DOWN_POLL_MS = 5000;
export const MAX_DOWN_CHECKS = 24;

interface ServiceState {
  registration: Loadable<ServiceInstall>;
  /** Install or uninstall in progress (waits for the OS prompt). */
  working: 'install' | 'uninstall' | 'repair' | null;
}

/** The service's registration with the OS, and installing or removing it (the OS asks for permission). */
export const ServiceStore = signalStore(
  { providedIn: 'root' },
  withState<ServiceState>({ registration: idle<ServiceInstall>(), working: null }),
  withComputed(({ registration }) => ({
    /** The Repair button works (macOS, not switched off in Login Items, the service is not up, no repair under way). */
    canRepair: computed(() => {
      const s = registration().value;
      return s !== null && s.platform === 'macos' && !s.disabledInLoginItems && !s.running && s.repair?.state !== 'working';
    }),
  })),
  withMethods((store, backend = inject(BACKEND), status = inject(StatusStore), toasts = inject(Notify)) => {
    let poll: ReturnType<typeof setTimeout> | null = null;
    let downChecks = 0;
    const isDownOnMac = (): boolean => {
      const s = store.registration().value;
      return s !== null && s.platform === 'macos' && !s.running && !s.disabledInLoginItems && downChecks < MAX_DOWN_CHECKS;
    };
    const load = async (): Promise<void> => {
      if (poll !== null) clearTimeout(poll);
      poll = null;
      const before = store.registration().value?.repair?.state ?? null;
      patchState(store, { registration: pending(store.registration()) });
      patchState(store, { registration: await settle(store.registration(), () => backend.serviceInstall()) });
      const repair = store.registration().value?.repair ?? null;
      if (before === 'working' && repair && repair.state !== 'working') {
        toasts.show(
          repair.state === 'succeeded'
            ? { tone: 'success', icon: 'check-circle', title: repair.message, body: null }
            : { tone: 'danger', icon: 'alert', title: 'The background service could not be re-registered', body: repair.message },
        );
      }
      if (repair?.state === 'working') {
        // The repair runs in the background and can take a minute: look again until it ends.
        poll = setTimeout(() => void load(), REPAIR_POLL_MS);
      } else if (isDownOnMac()) {
        // Down on macOS: the app's own repair may start any moment (version change, launchd refusing the job), so look again.
        downChecks += 1;
        poll = setTimeout(() => void load(), DOWN_POLL_MS);
      } else if (repair?.state === 'succeeded' && store.registration().value?.running && status.offline()) {
        // The service is back: the connection banner and the panels read the status again.
        await status.load();
      }
    };
    const change = async (what: 'install' | 'uninstall') => {
      patchState(store, { working: what });
      try {
        await (what === 'install' ? backend.installService() : backend.uninstallService());
        toasts.show({ tone: 'success', icon: 'check-circle', title: what === 'install' ? 'The Pitcrew service is installed and running' : 'The Pitcrew service was removed', body: null });
        await status.load();
      } catch (e) {
        const err = toCommandError(e);
        toasts.show({ tone: err.code === 'cancelled' ? 'neutral' : 'danger', icon: 'alert', title: err.code === 'cancelled' ? 'Cancelled' : 'That did not work', body: err.message });
      } finally {
        patchState(store, { working: null });
        await load();
      }
    };
    return {
      load,
      /** Opens System Settings ▸ General ▸ Login Items; the user switches the item (Pitcrew never does). */
      async openLoginItems(): Promise<void> {
        try {
          await backend.openLoginItems();
        } catch (e) {
          toasts.show({ tone: 'danger', icon: 'alert', title: 'Could not open System Settings', body: toCommandError(e).message });
        }
      },
      /** Repairs the background service (macOS): the app re-registers it in the background; the progress shows in the state. Needs no service. */
      async repair(): Promise<void> {
        patchState(store, { working: 'repair' });
        try {
          await backend.repairService();
          downChecks = 0;
        } catch (e) {
          toasts.show({ tone: 'danger', icon: 'alert', title: 'Repair did not start', body: toCommandError(e).message });
        } finally {
          patchState(store, { working: null });
        }
        await load();
      },
      install: () => change('install'),
      uninstall: () => change('uninstall'),
    };
  }),
);
