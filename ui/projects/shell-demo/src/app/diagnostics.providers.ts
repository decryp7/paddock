import { provideShellDiagnostics, provideWebviewLog } from '@decryp7/paddock-ui/shell/diagnostics';
import { demoDiagnosticsBackend } from './demo-diagnostics';
import { tauriIpc } from './demo-ipc';

const ipc = tauriIpc();

/** What the Report a Problem window boots with (loaded with it, never in the main window's first paint). */
export const DIAGNOSTICS_PROVIDERS = [
  provideShellDiagnostics({ backend: demoDiagnosticsBackend }),
  // The window's own errors reach the engine's log too (its contract names `log_from_webview`); the browser build has no log.
  ...(ipc ? [provideWebviewLog({ ipc: () => ({ invoke: (command, args) => ipc.invoke(command, args) }) })] : []),
];
