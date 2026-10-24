import { EnvironmentProviders, Provider } from '@angular/core';
import type { SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { SettingsBackend, SettingsStore, SHELL_SETTINGS_PAGES } from '@decryp7/paddock-ui/shell/settings';
import { SettingsDrafts } from './settings-drafts';
import { SETTINGS_RESTART, SETTINGS_WINDOW_PAGES } from './settings-window';

export interface SettingsWindowOptions {
  /** Builds the window's backend (in an injection context): `new TauriSettingsBackend(ipc)` in the app, a fake in the browser. */
  readonly backend: () => SettingsBackend;
  /** The app's settings pages (its `settingsPages` contributions); the shell's own are added. */
  readonly pages: readonly SettingsPageDef[];
  /** Restarts the app keeping its session, for the restart bar's "Restart now"; without it the bar has no button. */
  readonly restart?: () => () => void | Promise<void>;
}

/**
 * The providers of the Settings window (the `providers` of the app's `pd.settings` dialog): the backend, the store, the
 * unsaved edits and the pages. Nothing else of the main window is needed there (rule 34). The window's look is its
 * appearance store's (the dialog window's base providers: `provideAppearance` with `provideSettingsAppearance`), never
 * written here.
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
  ];
}
