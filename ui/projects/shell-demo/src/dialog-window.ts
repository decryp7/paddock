import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { DEMO_DIALOGS } from './app/demo.contributions';
import { dialogBaseProviders } from './app/dialog-base';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { provideDemoAppearance } from './app/demo-appearance';
import { tauriBridge } from './app/demo-ipc';
import { demoSettingsBackend } from './app/demo-settings';

/** A dialog window of the demo: the dialog shell, the demo's small base set and the dialog's own providers. */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // In the desktop app the dialog talks to its window through the shell plugin; the browser build opens the route by
  // URL (e2e). The dialog window's look follows the Appearance settings' change events (no settings command in its
  // contract).
  const bridge = tauriBridge();
  const appearance = [provideSettingsAppearance({ settings: demoSettingsBackend, read: 'events' }), provideDemoAppearance()];
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      dialogs: DEMO_DIALOGS,
      providers: [...dialogBaseProviders(), ...appearance],
      route,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
