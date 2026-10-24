import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { DemoJobsEngine } from './demo-jobs';
import { GreetingLog } from './greeting-log';

/** The demo's part of the shell context: small and purpose-built (interface segregation). */
export interface DemoContext {
  readonly greetings: () => number;
  greet(): number;
  reset(): void;
  /** Starts the sample job (phases, a step that cannot be cancelled); returns its id at once. */
  startJob(): number;
  /** Starts a job that reports nothing until cancelled ("Still working"). */
  startQuietJob(): number;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const demoContext: AppContextFactory<DemoContext> = () => {
  const log = inject(GreetingLog);
  const jobs = inject(DemoJobsEngine);
  return {
    greetings: log.greetings,
    greet: () => log.greet(),
    reset: () => log.reset(),
    startJob: () => jobs.startImport(),
    startQuietJob: () => jobs.startQuiet(),
  };
};
