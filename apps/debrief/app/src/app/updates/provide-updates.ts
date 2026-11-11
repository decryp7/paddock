import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { provideNotificationSource } from '../health/provide-notification-host';
import { createUpdatesBackend, UpdatesBackend } from './updates-backend';
import { UpdatesSource } from './updates-source';
import { UPDATES_COMMANDS_CONTRIBUTIONS } from './updates.contributions';

/** Signed updates: the backend, Help ▸ Check for updates… and the update-available notification. One line in `app.config.ts`. */
export function provideUpdates(): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: UpdatesBackend, useFactory: createUpdatesBackend },
    // Stubs only: the command's code loads when the window is idle or it runs.
    ...UPDATES_COMMANDS_CONTRIBUTIONS,
    provideNotificationSource(UpdatesSource),
  ]);
}
