import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { provideNotificationSource } from '../health/provide-notification-host';
import { createUpdatesBackend, UpdatesBackend } from './updates-backend';
import { UpdatesCommands } from './updates.commands';
import { UpdatesSource } from './updates-source';
import { contributeFrom } from '../extensions/contributions';

/** Signed updates: the backend, Help ▸ Check for updates… and the update-available notification. One line in `app.config.ts`. */
export function provideUpdates(): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: UpdatesBackend, useFactory: createUpdatesBackend },
    ...contributeFrom('commands', () => inject(UpdatesCommands).commands),
    provideNotificationSource(UpdatesSource),
  ]);
}
