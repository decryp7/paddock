import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { BACKEND } from '../core/backend';
import { Notifier } from '../shared/notifier';

/** Dyno's part of the shell context: small and purpose-built (interface segregation). */
export interface DynoContext {
  /** Saves the flight recorder's last ten minutes. */
  capture(): Promise<void>;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const dynoContext: AppContextFactory<DynoContext> = () => {
  const backend = inject(BACKEND);
  const notifier = inject(Notifier);
  return {
    capture: async () => {
      try {
        const info = await backend.capture();
        notifier.show(`Captured the last ten minutes (${info.samples} samples)`);
      } catch (e) {
        notifier.show(e instanceof Error ? e.message : 'The capture failed.', 'bad');
      }
    },
  };
};
