import { effect, EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer, type Signal } from '@angular/core';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { appearanceFollower } from './appearance';
import { SettingsBackend } from './settings-backend';
import { SETTINGS_DIALOG_ID } from './settings.contributions';

export interface ShellSettingsOptions {
  /** Builds the window's backend (in an injection context). */
  readonly backend: () => SettingsBackend;
  /**
   * Keeps this window's look on the Appearance settings (default true): at once, or once `gate` holds (in an injection
   * context: `windowReady` from `shell/startup`, so nothing is asked of the engine before the window is ready).
   */
  readonly appearance?: boolean | { readonly gate: () => Signal<boolean> };
}

/** Starts following the Appearance settings, at once or when `gate` first holds. */
function startAppearance(gate: (() => Signal<boolean>) | null): void {
  const start = appearanceFollower();
  if (!gate) return start();
  const open = gate();
  // A side effect outside the signal graph (IPC): start following once the window may talk to the engine.
  effect(() => {
    if (open()) start();
  });
}

/**
 * Settings in a main window: the backend, "Open Settings" for notices and repairs (`OPEN_SETTINGS`, opening the
 * `pd.settings` dialog at a page), and the window's look kept on the Appearance settings. Register
 * `SETTINGS_CONTRIBUTIONS` too (the command, its key and menu item, the shell's pages).
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
    ...(appearance === false ? [] : [provideEnvironmentInitializer(() => startAppearance(appearance === true ? null : appearance.gate))]),
  ]);
}
