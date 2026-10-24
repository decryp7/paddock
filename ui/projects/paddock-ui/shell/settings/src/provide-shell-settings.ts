import { EnvironmentProviders, inject, makeEnvironmentProviders, type Signal } from '@angular/core';
import { APPEARANCE_SOURCE } from '@decryp7/paddock-ui/shell';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { SettingsBackend } from './settings-backend';
import { settingsAppearanceBackend } from './settings-appearance';
import { SETTINGS_DIALOG_ID } from './settings.contributions';

export interface ShellSettingsOptions {
  /** Builds the window's backend (in an injection context). */
  readonly backend: () => SettingsBackend;
  /**
   * The window's look lives in the Appearance settings (default true): `provideAppearance` then takes its backend from
   * here (`APPEARANCE_SOURCE`). With `gate` (in an injection context: `() => inject(PAGES_WINDOW_READY).asReadonly()`)
   * nothing is asked of the engine before the window is ready. `false` leaves the look to the app.
   */
  readonly appearance?: boolean | { readonly gate: () => Signal<boolean> };
}

/**
 * Settings in a main window: the backend, "Open Settings" for notices and repairs (`OPEN_SETTINGS`, opening the
 * `pd.settings` dialog at a page), and the source of the window's appearance (rule 40: the appearance store is the one
 * writer of the look; the engine's Appearance settings are its one source). Register `SETTINGS_CONTRIBUTIONS` too (the
 * command, its key and menu item, the shell's pages) and `provideAppearance({ accent })`.
 */
export function provideShellSettings(options: ShellSettingsOptions): EnvironmentProviders {
  const appearance = options.appearance ?? true;
  return makeEnvironmentProviders([
    { provide: SettingsBackend, useFactory: options.backend },
    {
      provide: OPEN_SETTINGS,
      useFactory: () => {
        const dialogs = inject(DialogService);
        return (page: string) => void dialogs.open(SETTINGS_DIALOG_ID, { inputs: page ? { page } : {} });
      },
    },
    ...(appearance === false ? [] : [{ provide: APPEARANCE_SOURCE, useValue: settingsAppearanceBackend(appearance === true ? {} : { gate: appearance.gate }) }]),
  ]);
}
