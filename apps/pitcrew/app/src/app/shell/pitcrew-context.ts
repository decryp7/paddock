import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notify } from '../state/notify';
import { StatusStore } from '../state/status.store';
import { RegisterLauncher } from './register-launcher';

/** Pitcrew's part of the shell context: small and purpose-built (interface segregation). */
export interface PitcrewContext {
  /** Runners that need the user (the Runners page's badge). */
  readonly attention: () => number;
  /** Runners with an update waiting (the Updates page's badge). */
  readonly updatesAvailable: () => number;
  /** Opens the registration wizard. */
  register(): Promise<boolean>;
  /** Writes the redacted diagnostics bundle and says where it went. */
  exportDiagnostics(): Promise<void>;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const pitcrewContext: AppContextFactory<PitcrewContext> = () => {
  const status = inject(StatusStore);
  const backend = inject(BACKEND);
  const notify = inject(Notify);
  const launcher = inject(RegisterLauncher);
  return {
    attention: () => status.attention().length,
    updatesAvailable: () => status.runners().filter((r) => r.updateTarget !== null && r.updatePhase.phase === 'idle').length,
    register: () => launcher.open(),
    exportDiagnostics: async () => {
      try {
        const path = await backend.exportDiagnostics();
        notify.show({ tone: 'success', title: 'Diagnostics saved', body: path });
      } catch (e) {
        notify.show({ tone: 'danger', title: 'Diagnostics could not be written', body: toCommandError(e).message });
      }
    },
  };
};
