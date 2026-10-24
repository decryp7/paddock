import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { ActivityBackend } from './activity-backend';
import { ActivityStore } from './activity.store';

export interface ShellActivityOptions {
  /** Builds the window's backend (in an injection context): `new TauriActivityBackend(ipc)` in the app, a fake in the browser. */
  readonly backend: () => ActivityBackend;
}

/**
 * The activity log of one window (rules 32, 34): the backend and the store. The Activity page is the lazy page
 * `ACTIVITY_PAGE` registers; give it these providers (`providers`) so the store lives with the page, or provide them in
 * the window's bootstrap when other parts read the log.
 */
export function provideShellActivity(options: ShellActivityOptions): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: ActivityBackend, useFactory: options.backend }, ActivityStore]);
}
