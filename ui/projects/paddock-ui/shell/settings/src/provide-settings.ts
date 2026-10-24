import { EnvironmentProviders, provideEnvironmentInitializer, Provider } from '@angular/core';
import type { SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { followAppearance } from './appearance';
import { SettingsBackend } from './settings-backend';
import { SHELL_SETTINGS_PAGES } from './settings.contributions';
import { SettingsDrafts } from './settings-drafts';
import { SETTINGS_RESTART, SETTINGS_WINDOW_PAGES } from './settings-window';
import { SettingsStore } from './settings.store';

export interface SettingsWindowOptions {
  /** Builds the window's backend (in an injection context): `new TauriSettingsBackend(ipc)` in the app, a fake in the browser. */
  readonly backend: () => SettingsBackend;
  /** The app's settings pages (its `settingsPages` contributions); the shell's own are added. */
  readonly pages: readonly SettingsPageDef[];
  /** Restarts the app keeping its session, for the restart bar's "Restart now"; without it the bar has no button. */
  readonly restart?: () => () => void | Promise<void>;
  /** Applies the Appearance settings to this window (default true). */
  readonly appearance?: boolean;
}

/**
 * The providers of the Settings window (the `providers` of the app's `pd.settings` dialog): the backend, the store, the
 * unsaved edits and the pages. Nothing else of the main window is needed there (rule 34).
 */
export function settingsWindowProviders(options: SettingsWindowOptions): (Provider | EnvironmentProviders)[] {
  const own = new Set(options.pages.map((p) => p.id));
  const pages = [...SHELL_SETTINGS_PAGES.filter((p) => !own.has(p.id)), ...options.pages];
  return [
    { provide: SettingsBackend, useFactory: options.backend },
    { provide: SETTINGS_WINDOW_PAGES, useValue: Object.freeze(pages) },
    ...(options.restart ? [{ provide: SETTINGS_RESTART, useFactory: options.restart }] : []),
    SettingsStore,
    SettingsDrafts,
    ...(options.appearance === false ? [] : [provideEnvironmentInitializer(() => followAppearance())]),
  ];
}
