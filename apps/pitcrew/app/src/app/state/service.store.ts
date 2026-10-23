import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { ServiceInstall } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { idle, pending, settle, type Loadable } from './loadable';
import { StatusStore } from './status.store';
import { ToastStore } from './toast.store';

/** How often the state is read again while the repair works. */
export const REPAIR_POLL_MS = 3000;

interface ServiceState {
  registration: Loadable<ServiceInstall>;
  /** Install or uninstall in progress (waits for the OS prompt). */
  working: 'install' | 'uninstall' | null;
}

/** The service's registration with the OS, and installing or removing it (the OS asks for permission). */
export const ServiceStore = signalStore(
  { providedIn: 'root' },
  withState<ServiceState>({ registration: idle<ServiceInstall>(), working: null }),
  withMethods((store, backend = inject(BACKEND), status = inject(StatusStore), toasts = inject(ToastStore)) => {
    let poll: ReturnType<typeof setTimeout> | null = null;
    const load = async (): Promise<void> => {
      if (poll !== null) clearTimeout(poll);
      poll = null;
      const before = store.registration().value?.repair?.state ?? null;
      patchState(store, { registration: pending(store.registration()) });
      patchState(store, { registration: await settle(store.registration(), () => backend.serviceInstall()) });
      const repair = store.registration().value?.repair ?? null;
      if (repair?.state === 'working') {
        // The repair runs in the background and can take a minute: look again until it ends.
        poll = setTimeout(() => void load(), REPAIR_POLL_MS);
      } else if (before === 'working' && repair) {
        toasts.show(
          repair.state === 'succeeded'
            ? { tone: 'success', icon: 'check-circle', title: repair.message, body: null }
            : { tone: 'danger', icon: 'alert', title: 'The background service could not be re-registered', body: repair.message },
        );
        if (repair.state === 'succeeded') await status.load();
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
      install: () => change('install'),
      uninstall: () => change('uninstall'),
    };
  }),
);
