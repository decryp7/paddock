import { applyStoredAppearance } from '@decryp7/paddock-ui/shell';
import { ipcBootErrorSender, startBootErrorForwarding } from '@decryp7/paddock-ui/shell/startup';
import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { parsePopoutRoute } from '@decryp7/paddock-ui/workbench/panels';
import { desktopIpc } from './app/desktop';

/*
 * The workbench reference app (ADR 0013 §4; plan W4, W8): every workbench contribution kind, in the browser against mock
 * backends and in the desktop app (apps/workbench-demo) against the real `paddock-shell` and `paddock-workbench` plugins.
 *
 * One bundle, three kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the dialog
 * shell; a pop-out window (`index.html#/popout?panel=...`) boots only the pop-out shell and its view's providers; any other
 * window boots the workbench. Each is its own lazy chunk.
 */
applyStoredAppearance(document.documentElement, 'graphite');

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = desktopIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

const dialog = parseDialogRoute(location.hash);
const popout = parsePopoutRoute(location.hash);
const boot = dialog
  ? import('./dialog-window').then((m) => m.bootDialogWindow(dialog))
  : popout
    ? import('./popout-window').then((m) => m.bootPopoutWindow())
    : import('./main-window').then((m) => m.bootMainWindow());
boot.catch((e: unknown) => console.error(e));
