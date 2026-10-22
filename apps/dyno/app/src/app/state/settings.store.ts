import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppInfo, CommandError, PluginInfo, Settings, Theme } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { ThemeService } from '../core/theme';
import { Notifier } from '../shared/notifier';

interface SettingsState {
  settings: Settings | null;
  error: CommandError | null;
  saving: boolean;
  appInfo: AppInfo | null;
  plugins: PluginInfo[];
  pluginsLoaded: boolean;
  pluginsError: CommandError | null;
}

/** Settings (applies the theme), app information and the plugins list. */
export const SettingsStore = signalStore(
  { providedIn: 'root' },
  withState<SettingsState>({ settings: null, error: null, saving: false, appInfo: null, plugins: [], pluginsLoaded: false, pluginsError: null }),
  withMethods((store, backend = inject(BACKEND), theme = inject(ThemeService), notifier = inject(Notifier)) => ({
    async load(): Promise<void> {
      patchState(store, { error: null });
      try {
        const settings = await backend.settings();
        theme.set(settings.theme);
        patchState(store, { settings });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
      backend
        .appInfo()
        .then((appInfo) => patchState(store, { appInfo }))
        .catch(() => undefined);
    },
    /** Previews a theme choice before it is saved. */
    applyTheme(t: Theme): void {
      theme.set(t);
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
      patchState(store, { saving: true });
      try {
        const saved = await backend.saveSettings(settings);
        theme.set(saved.theme);
        patchState(store, { settings: saved, saving: false });
        if (message) notifier.show(message);
        return true;
      } catch (e) {
        patchState(store, { saving: false });
        notifier.error(toCommandError(e));
        return false;
      }
    },
  })),
);
