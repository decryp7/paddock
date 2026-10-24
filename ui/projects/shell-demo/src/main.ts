import { parseDialogRoute } from '@decryp7/paddock-ui/shell/dialogs';

// One bundle, two kinds of window (rule 34): a dialog window (Rust loads `index.html#/dialog?id=...`) boots only the
// dialog shell and its dialog's providers; any other window boots the demo. Each is its own lazy chunk.
const route = parseDialogRoute(location.hash);
const boot = route ? import('./dialog-window').then((m) => m.bootDialogWindow(route)) : import('./main-window').then((m) => m.bootMainWindow());
boot.catch((e: unknown) => console.error(e));
