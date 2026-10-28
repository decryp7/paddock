import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { provideIcons } from '../shared/icons';
import { provideSpotterAppearance } from './appearance';
import { DIALOGS } from './contributions';
import { dialogBaseProviders } from './dialog-base';
import { settingsBackend } from './engines';
import { tauriBridge } from './ipc';

/** A dialog window of Spotter: the dialog shell, the small base set and the dialog's own providers (rule 34). */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // In the desktop app the dialog talks to its window through the shell plugin; the browser build opens the route by URL.
  // The dialog window's look follows the Appearance settings' change events (no settings command in its contract).
  const bridge = tauriBridge();
  const appearance = [provideSettingsAppearance({ settings: settingsBackend, read: 'events' }), provideSpotterAppearance()];
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      dialogs: DIALOGS,
      // The settings page's stores are app-wide singletons of this window and read the backend: it is provided here.
      providers: [provideIcons(), { provide: BACKEND, useFactory: createBackend }, ...dialogBaseProviders(), ...appearance],
      route,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
