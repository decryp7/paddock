import { applyStoredAppearance } from '@decryp7/paddock-ui/shell';
import { initI18n } from '@decryp7/paddock-ui/shell/i18n';
import { ipcBootErrorSender, startBootErrorForwarding } from '@decryp7/paddock-ui/shell/startup';
import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { tauriIpc } from './app/shell/ipc';
import { ACCENT } from './app/shell/names';

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = tauriIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

// One bundle, two kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the dialog
// shell and its dialog's providers; the main window boots the pages frame. Each is its own lazy chunk. The stored
// appearance is applied before the first paint, the language before any module with `$localize` is imported.
const route = parseDialogRoute(location.hash);
applyStoredAppearance(document.documentElement, ACCENT);
initI18n({ loaders: {} })
  .then(() => (route ? import('./app/shell/dialog-window').then((m) => m.bootDialogWindow(route)) : import('./app/shell/main-window').then((m) => m.bootMainWindow())))
  .catch((e: unknown) => console.error(e));
