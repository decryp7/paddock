import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { Config, RunnerConfig, SettingsReply } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { idle, pending, settle, type Loadable } from './loadable';
import { StatusStore } from './status.store';
import { ToastStore } from './toast.store';

interface SettingsState {
  settings: Loadable<SettingsReply>;
  saving: boolean;
}

/** `pitcrew.toml` as the service has it, and saving it (the service validates and may refuse). */
export const SettingsStore = signalStore(
  { providedIn: 'root' },
  withState<SettingsState>({ settings: idle<SettingsReply>(), saving: false }),
  withComputed(({ settings }) => ({
    config: computed(() => settings().value?.config ?? null),
    readOnly: computed(() => settings().value?.readOnly ?? false),
    problems: computed(() => settings().value?.problems ?? []),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(ToastStore), status = inject(StatusStore)) => {
    const load = async () => {
      patchState(store, { settings: pending(store.settings()) });
      patchState(store, { settings: await settle(store.settings(), () => backend.settings()) });
    };
    const save = async (config: Config, what = 'Settings saved'): Promise<boolean> => {
      patchState(store, { saving: true });
      try {
        await backend.saveSettings(config);
        toasts.show({ tone: 'success', icon: 'check-circle', title: what, body: null });
        await Promise.all([load(), status.load()]);
        return true;
      } catch (e) {
        toasts.show({ tone: 'danger', icon: 'alert', title: 'The settings were not saved', body: toCommandError(e).message });
        return false;
      } finally {
        patchState(store, { saving: false });
      }
    };
    return {
      load,
      save,
      /** Saves one runner's entry changed by `change` (e.g. its update policy override). */
      async saveRunner(name: string, change: (r: RunnerConfig) => RunnerConfig, what: string): Promise<boolean> {
        const config = store.config();
        if (!config) return false;
        const next: Config = { ...config, runners: config.runners.map((r) => (r.name === name ? change(structuredClone(r)) : r)) };
        return save(next, what);
      },
      runnerConfig(name: string): RunnerConfig | null {
        return store.config()?.runners.find((r) => r.name === name) ?? null;
      },
    };
  }),
);
