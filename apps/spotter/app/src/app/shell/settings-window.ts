import type { EnvironmentProviders, Provider } from '@angular/core';
import { provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import { jobsBackend, settingsBackend } from './engines';
import { SETTINGS_PAGES } from './contributions';

/** The Settings window's providers (its dialog's `providers`): loaded with it, never in the main window's first paint. */
export const SETTINGS_WINDOW: (Provider | EnvironmentProviders)[] = [
  // The page shows the browser install's phase and progress as the engine's job reports them, with Cancel.
  provideShellJobs({ backend: jobsBackend, stillWorkingAfterMs: 10_000 }),
  ...settingsWindowProviders({ backend: settingsBackend, pages: SETTINGS_PAGES }),
];
