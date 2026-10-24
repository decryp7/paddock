import { ipcBootErrorSender, startBootErrorForwarding } from '@decryp7/paddock-ui/shell/startup';
import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { tauriIpc } from './app/demo-ipc';

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = tauriIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

// One bundle, two kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the
// dialog shell and its dialog's providers; any other window boots the demo. Each is its own lazy chunk.
const route = parseDialogRoute(location.hash);
const boot = route ? import('./dialog-window').then((m) => m.bootDialogWindow(route)) : import('./main-window').then((m) => m.bootMainWindow());
boot.catch((e: unknown) => console.error(e));
