import type { Provider } from '@angular/core';
import { MemoryJobsBackend, provideShellJobs, TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { tauriIpc } from './ipc';

/**
 * The register window follows the job it starts (the wizard shows the engine's refusal in its own page, and the shell does not
 * announce the end a second time): the jobs of this window, over the shell's job commands.
 */
export const REGISTER_WINDOW_PROVIDERS: Provider[] = [
  provideShellJobs({
    backend: () => {
      const ipc = tauriIpc();
      return ipc ? new TauriJobsBackend(ipc) : new MemoryJobsBackend();
    },
  }) as unknown as Provider,
];
