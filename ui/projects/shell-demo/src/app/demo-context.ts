import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { GreetingLog } from './greeting-log';

/** The demo's part of the shell context: small and purpose-built (interface segregation). */
export interface DemoContext {
  readonly greetings: () => number;
  greet(): number;
  reset(): void;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const demoContext: AppContextFactory<DemoContext> = () => {
  const log = inject(GreetingLog);
  return { greetings: log.greetings, greet: () => log.greet(), reset: () => log.reset() };
};
