import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { ONBOARDING_COMMANDS_CONTRIBUTIONS } from './onboarding.contributions';

/** One line in `app.config.ts`: the demo and the guided tour (Help → Try the demo / Take the tour). */
export function provideOnboarding(): EnvironmentProviders {
  // Stubs only: the commands' code loads when the window is idle or one runs.
  return makeEnvironmentProviders(ONBOARDING_COMMANDS_CONTRIBUTIONS);
}
