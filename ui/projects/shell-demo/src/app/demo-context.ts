import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { DemoEngineData, DemoHealthEngine } from './demo-health';
import { DemoJobsEngine } from './demo-jobs';
import { GreetingLog } from './greeting-log';
import { RunnerData } from './runner-data';

/** The demo's part of the shell context: small and purpose-built (interface segregation). */
export interface DemoContext {
  readonly greetings: () => number;
  greet(): number;
  reset(): void;
  /** Starts the sample job (phases, a step that cannot be cancelled); returns its id at once. */
  startJob(): number;
  /** Starts a job that reports nothing until cancelled ("Still working"). */
  startQuietJob(): number;
  /** Whether the page is laid out compact (a menu toggle). */
  readonly compact: () => boolean;
  toggleCompact(): void;
  /** Three identical failures of the demo engine (its Repair works); the engine panel's load meets it. */
  breakEngine(): void;
  /** The search index fails (its Repair fails too). */
  breakIndex(): void;
  /** Runners that need the user (the Runners page's badge). */
  readonly attention: () => number;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const demoContext: AppContextFactory<DemoContext> = () => {
  const log = inject(GreetingLog);
  const jobs = inject(DemoJobsEngine);
  const health = inject(DemoHealthEngine);
  const data = inject(DemoEngineData);
  const runners = inject(RunnerData);
  return {
    greetings: log.greetings,
    greet: () => log.greet(),
    reset: () => log.reset(),
    startJob: () => jobs.startImport(),
    startQuietJob: () => jobs.startQuiet(),
    compact: log.compact,
    toggleCompact: () => log.toggleCompact(),
    breakEngine: () => {
      health.breakEngine();
      data.load();
    },
    breakIndex: () => health.breakIndex(),
    attention: () => runners.attention,
  };
};
