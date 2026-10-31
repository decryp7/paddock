import type { EnvironmentProviders, Provider } from '@angular/core';
import { MockStartupBackend, provideStartupBackend, provideStartupContract, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { backendProviders } from './dialog-providers';
import { tauriIpc } from './ipc';

/**
 * What every dialog window of Pitcrew boots with besides the shell's set: the start-up contract (a dialog window reports ready
 * and answers liveness pings, ADR 0058), Pitcrew's backend and icons (its stores are root singletons of the window; the backend
 * is created only when something asks for it). The boot harness uses the same function; what these providers send is the `base` of Rust's table
 * file's `base`.
 */
export function dialogBaseProviders(): (Provider | EnvironmentProviders)[] {
  const ipc = tauriIpc();
  return [...backendProviders(), provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())), provideStartupContract()];
}

/** The commands the base providers send (the `base` of the Rust dialog table, `src-tauri/src/dialogs.rs`; a spec compares them). */
export const DIALOG_BASE_COMMANDS = ['window_pong', 'window_ready'] as const;
