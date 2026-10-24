import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { DEMO_DIALOGS } from './app/demo.contributions';
import { dialogBaseProviders } from './app/dialog-base';

/** A dialog window of the demo: the dialog shell, the demo's small base set and the dialog's own providers. */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
  mountDialogWindowHost();
  // The desktop build passes `io: () => tauriDialogIo(bridge)`; the browser build opens the route by URL (e2e).
  return bootstrapApplication(DialogShell, dialogWindowConfig({ dialogs: DEMO_DIALOGS, providers: dialogBaseProviders(), route }));
}
