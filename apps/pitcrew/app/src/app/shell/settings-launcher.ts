import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';

/** The Settings window's dialog id (the shell's `pd.settings`, registered in the contributions). */
export const SETTINGS_DIALOG_ID = 'pd.settings';

/** Opens Settings at one of Pitcrew's pages: Settings is a window of the shell, not a route of the main window. */
@Injectable({ providedIn: 'root' })
export class SettingsLauncher {
  private readonly ports = inject(SHELL_PORTS);

  /** Opens the Settings window at `page` (`pc.tokens`). */
  open(page: string): void {
    void this.ports.openDialog(SETTINGS_DIALOG_ID, { page });
  }
}
