import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { DEMO_DIALOGS } from './app/demo.contributions';
import { dialogBaseProviders } from './app/dialog-base';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { provideDemoAppearance } from './app/demo-appearance';
import { demoSettingsBackend } from './app/demo-settings';

/** A dialog window of the demo: the dialog shell, the demo's small base set and the dialog's own providers. */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // The desktop build passes `io: () => tauriDialogIo(bridge)`; the browser build opens the route by URL (e2e). The
  // dialog window's look follows the Appearance settings' change events (no settings command in its contract).
  const appearance = [provideSettingsAppearance({ settings: demoSettingsBackend, read: 'events' }), provideDemoAppearance()];
  return bootstrapApplication(DialogShell, dialogWindowConfig({ dialogs: DEMO_DIALOGS, providers: [...dialogBaseProviders(), ...appearance], route }));
}
