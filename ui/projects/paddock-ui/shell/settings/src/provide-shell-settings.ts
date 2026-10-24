import { EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { followAppearance } from './appearance';
import { SettingsBackend } from './settings-backend';
import { SETTINGS_DIALOG_ID } from './settings.contributions';

export interface ShellSettingsOptions {
  /** Builds the window's backend (in an injection context). */
  readonly backend: () => SettingsBackend;
  /** Applies the Appearance settings to this window (default true). */
  readonly appearance?: boolean;
}

/**
 * Settings in a main window: the backend, "Open Settings" for notices and repairs (`OPEN_SETTINGS`, opening the
 * `pd.settings` dialog at a page), and the window's look kept on the Appearance settings. Register
 * `SETTINGS_CONTRIBUTIONS` too (the command, its key and menu item, the shell's pages).
 */
export function provideShellSettings(options: ShellSettingsOptions): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: SettingsBackend, useFactory: options.backend },
    {
      provide: OPEN_SETTINGS,
      useFactory: () => {
        const dialogs = inject(DialogService);
        return (page: string) => void dialogs.open(SETTINGS_DIALOG_ID, { inputs: page ? { page } : {} });
      },
    },
    ...(options.appearance === false ? [] : [provideEnvironmentInitializer(() => followAppearance())]),
  ]);
}
