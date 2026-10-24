import { applyStoredAppearance } from '@decryp7/paddock-ui/shell';
import { initI18n, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import { ipcBootErrorSender, startBootErrorForwarding } from '@decryp7/paddock-ui/shell/startup';
import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { DEMO_ACCENT } from './app/demo-appearance';
import { tauriIpc } from './app/demo-ipc';

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = tauriIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

// One bundle, three kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the
// dialog shell and its dialog's providers; any other window boots the demo. Each is its own lazy chunk.
const route = parseDialogRoute(location.hash);
// The stored appearance before the first paint; then the language, before any module with `$localize` is imported
// (plan S15). The loader table is the app's: `paddock-i18n merge` wrote the shell's and the demo's messages into it.
applyStoredAppearance(document.documentElement, DEMO_ACCENT);
// The tray popover (`index.html#/tray`, opened by the tray icon) is a third kind: only its popover.
const tray = location.hash.startsWith('#/tray');
const boot = initI18n({
  loaders: { [PSEUDO_LOCALE]: () => import('./locale/messages.en-XA.json').then((m) => m.default.translations) },
}).then(() =>
  route
    ? import('./dialog-window').then((m) => m.bootDialogWindow(route))
    : tray
      ? import('./tray-window').then((m) => m.bootTrayWindow())
      : import('./main-window').then((m) => m.bootMainWindow()),
);
boot.catch((e: unknown) => console.error(e));
