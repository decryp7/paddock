import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { KNOWN_ISSUES_COMMANDS_CONTRIBUTIONS } from './known-issues.contributions';

/** One line in `app.config.ts`: the known-issues feature (ADR 0039): its palette command. The section lives in the Problems pane. */
export function provideKnownIssues(): EnvironmentProviders {
  // Stubs only: the command's code loads when the window is idle or it runs.
  return makeEnvironmentProviders(KNOWN_ISSUES_COMMANDS_CONTRIBUTIONS);
}
