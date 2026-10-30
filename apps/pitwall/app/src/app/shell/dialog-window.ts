import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { NOTIFICATION_SOURCES } from '@decryp7/paddock-ui/shell/notify';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { Toasts } from '../core/toasts';
import { OWN_TOAST_HOST } from '../features/settings/settings-page';
import { provideIcons } from '../shared/icons';
import { providePitwallAppearance } from './appearance';
import { DIALOGS } from './contributions';
import { dialogBaseProviders } from './dialog-base';
import { settingsBackend } from './engines';
import { tauriBridge } from './ipc';

/** A dialog window of Pitwall: the dialog shell, the small base set and the dialog's own providers (rule 34). */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // In the desktop app the dialog talks to its window through the shell plugin; the browser build opens the route by URL.
  // The dialog window's look follows the Appearance settings' change events (no settings command in its contract).
  const bridge = tauriBridge();
  const appearance = [provideSettingsAppearance({ settings: settingsBackend, read: 'events' }), providePitwallAppearance()];
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      dialogs: DIALOGS,
      providers: [
        provideIcons(),
        providePaddockMaterial(),
        { provide: BACKEND, useFactory: createBackend },
        // A window of its own has no frame with a toast host: the Settings window's page shows its confirmations itself.
        { provide: NOTIFICATION_SOURCES, multi: true, useExisting: Toasts },
        { provide: OWN_TOAST_HOST, useValue: true },
        ...dialogBaseProviders(),
        ...appearance,
      ],
      route,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
