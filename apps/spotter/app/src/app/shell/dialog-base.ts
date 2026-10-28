import type { EnvironmentProviders, Provider } from '@angular/core';
import { MockStartupBackend, provideStartupBackend, provideStartupContract, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { tauriIpc } from './ipc';

/**
 * What every dialog window boots with besides the shell's set: the start-up contract (a dialog window reports ready and
 * answers liveness pings, ADR 0058). The boot harness uses the same function, so a provider missing here fails in a
 * spec; what these providers send is the contract file's `base`.
 */
export function dialogBaseProviders(): (Provider | EnvironmentProviders)[] {
  const ipc = tauriIpc();
  return [provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())), provideStartupContract()];
}

/** The commands the base providers send (`dialogContractFile(..., { base })`). */
export const DIALOG_BASE_COMMANDS = ['window_pong', 'window_ready'] as const;
