import type { EnvironmentProviders, Provider } from '@angular/core';
import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { settingsBackend } from './engines';
import { SETTINGS_PAGES } from './contributions';

/** The Settings window's providers (its dialog's `providers`): loaded with it, never in the main window's first paint. */
export const SETTINGS_WINDOW: (Provider | EnvironmentProviders)[] = [
  { provide: BACKEND, useFactory: createBackend },
  ...settingsWindowProviders({ backend: settingsBackend, pages: SETTINGS_PAGES }),
];
