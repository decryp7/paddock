import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { INCIDENT_COMMANDS_CONTRIBUTIONS } from './incident.contributions';

/** One line in `app.config.ts`: the incident summary report (ADR 0039). */
export function provideIncident(): EnvironmentProviders {
  // Stubs only: the command's code loads when the window is idle or it runs.
  return makeEnvironmentProviders(INCIDENT_COMMANDS_CONTRIBUTIONS);
}
