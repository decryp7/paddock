import { provideShellDiagnostics, provideWebviewLog } from '@decryp7/paddock-ui/shell/diagnostics';
import { diagnosticsBackend } from './engines';
import { tauriIpc } from './ipc';

const ipc = tauriIpc();

/** What the Report a problem window boots with (loaded with it, never in the main window's first paint). */
export const DIAGNOSTICS_PROVIDERS = [
  provideShellDiagnostics({ backend: diagnosticsBackend }),
  // The window's own errors reach the log too (its contract names `log_from_webview`); the browser build has no log.
  ...(ipc ? [provideWebviewLog({ ipc: () => ({ invoke: (command, args) => ipc.invoke(command, args) }) })] : []),
];
