import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { HELP_COMMANDS_CONTRIBUTIONS } from './help.contributions';

export { SHORTCUTS_KEY } from './help.ids';

/** One line in `app.config.ts`. */
export function provideHelp(): EnvironmentProviders {
  // Stubs only: the commands' code (`help-commands.ts`) loads when the window is idle or one runs.
  return makeEnvironmentProviders(HELP_COMMANDS_CONTRIBUTIONS);
}
