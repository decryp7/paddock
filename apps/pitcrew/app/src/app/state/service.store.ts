import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { ServiceInstall } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { idle, pending, settle, type Loadable } from './loadable';
import { StatusStore } from './status.store';
import { ToastStore } from './toast.store';

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
    const load = async () => {
      patchState(store, { registration: pending(store.registration()) });
      patchState(store, { registration: await settle(store.registration(), () => backend.serviceInstall()) });
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
      install: () => change('install'),
      uninstall: () => change('uninstall'),
    };
  }),
);
