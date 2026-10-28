import { applyStoredAppearance } from '@decryp7/paddock-ui/shell';
import { initI18n } from '@decryp7/paddock-ui/shell/i18n';
import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { ipcBootErrorSender, startBootErrorForwarding } from '@decryp7/paddock-ui/shell/startup';
import { ACCENT } from './app/shell/appearance';
import { tauriIpc } from './app/shell/ipc';

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = tauriIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

// One bundle, three kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the
// dialog shell and its dialog's providers; the tray popover (`#/tray`) only its popover; any other window boots the
// pages frame. Each is its own lazy chunk.
const route = parseDialogRoute(location.hash);
const tray = location.hash.startsWith('#/tray');
// The stored appearance before the first paint; then the language, before any module with `$localize` is imported.
applyStoredAppearance(document.documentElement, ACCENT);
const boot = initI18n({ loaders: {} }).then(() =>
  route
    ? import('./app/shell/dialog-window').then((m) => m.bootDialogWindow(route))
    : tray
      ? import('./app/shell/tray-window').then((m) => m.bootTrayWindow())
      : import('./app/shell/main-window').then((m) => m.bootMainWindow()),
);
boot.catch((e: unknown) => console.error(e));
