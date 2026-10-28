import type { EnvironmentProviders, Provider } from '@angular/core';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { MockStartupBackend, provideStartupBackend, provideStartupContract, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { tauriIpc } from './demo-ipc';

/**
 * What every dialog window of the demo boots with besides the shell's set: the start-up contract (a dialog window
 * reports ready and answers liveness pings, ADR 0058). The boot harness uses the same function, so a provider missing
 * here fails in a spec; the Material defaults (`providePaddockMaterial`, with `css/material.css`) are for the checkboxes and progress bars of About and Updates; what these providers send is the contract file's `base`.
 */
export function dialogBaseProviders(): (Provider | EnvironmentProviders)[] {
  const ipc = tauriIpc();
  return [providePaddockMaterial(), provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())), provideStartupContract()];
}

/** The commands the base providers send (`dialogContractFile(..., { base })`; `paddock-check-dialog-commands` checks it). */
export const DIALOG_BASE_COMMANDS = ['window_pong', 'window_ready'] as const;
