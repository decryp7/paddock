import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, PluginInfo, Settings } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

interface SettingsState {
  settings: Settings | null;
  error: CommandError | null;
  saving: boolean;
  /** Why the last save failed (null after a good save); the page can show it next to the button. */
  saveError: CommandError | null;
  plugins: PluginInfo[];
  pluginsLoaded: boolean;
  pluginsError: CommandError | null;
}

/** Settings (the appearance is the shell's, not part of these) and the plugins list. */
export const SettingsStore = signalStore(
  { providedIn: 'root' },
  withState<SettingsState>({ settings: null, error: null, saving: false, saveError: null, plugins: [], pluginsLoaded: false, pluginsError: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
    async load(): Promise<void> {
      patchState(store, { error: null });
      try {
        const settings = await backend.settings();
        patchState(store, { settings });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    async loadPlugins(): Promise<void> {
      patchState(store, { pluginsError: null });
      try {
        patchState(store, { plugins: await backend.plugins(), pluginsLoaded: true });
      } catch (e) {
        patchState(store, { pluginsError: toCommandError(e) });
      }
    },
    /** Saves the whole settings object; the engine validates it and returns what it stored. */
    async save(settings: Settings, message: string | null = 'Settings saved'): Promise<boolean> {
      patchState(store, { saving: true, saveError: null });
      try {
        const saved = await backend.saveSettings(settings);
        patchState(store, { settings: saved, saving: false, saveError: null });
        if (message) notifier.show(message);
        return true;
      } catch (e) {
        const saveError = toCommandError(e);
        patchState(store, { saving: false, saveError });
        notifier.error(saveError);
        return false;
      }
    },
  })),
);
