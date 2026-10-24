import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { DEMO_DIALOGS } from './app/demo.contributions';
import { dialogBaseProviders } from './app/dialog-base';
import { provideDemoAppearance } from './app/demo-appearance';

/** A dialog window of the demo: the dialog shell, the demo's small base set and the dialog's own providers. */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // The desktop build passes `io: () => tauriDialogIo(bridge)`; the browser build opens the route by URL (e2e). The
  // dialog window follows a theme change made in another window while it is open (plan S15).
  return bootstrapApplication(DialogShell, dialogWindowConfig({ dialogs: DEMO_DIALOGS, providers: [...dialogBaseProviders(), provideDemoAppearance()], route }));
}
