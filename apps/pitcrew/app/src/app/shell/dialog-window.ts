import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { providePitcrewAppearance } from './appearance';
import { dialogBaseProviders } from './dialog-base';
import { tauriBridge } from './ipc';
import { PITCREW_DIALOGS } from './pitcrew.contributions';
import { settingsBackend } from './settings';

/** A dialog window of Pitcrew: the dialog shell, the small base set and the dialog's own providers (rule 34). */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // In the desktop app the dialog talks to its window through the shell plugin; the browser build opens the route by URL
  // (e2e). The window's look follows the Appearance settings' change events (no settings command in most contracts).
  const bridge = tauriBridge();
  const appearance = [provideSettingsAppearance({ settings: settingsBackend, read: 'events' }), providePitcrewAppearance()];
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      dialogs: PITCREW_DIALOGS,
      providers: [...dialogBaseProviders(), ...appearance],
      route,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
