import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { ServiceBackend } from './backend';
import { ServiceStore } from './service.store';

export interface ShellServiceOptions {
  /** Builds the window's backend (in an injection context): `new TauriServiceBackend(ipc)`, a fake in the browser. */
  readonly backend: () => ServiceBackend;
}

/**
 * The background service for the Settings window's page (rule 32, 34): the backend and the per-window store. Only the
 * Background service page needs it; the page's `providers` module calls it, so no other window pays for it.
 */
export function provideShellService(options: ShellServiceOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: ServiceBackend, useFactory: options.backend }, ServiceStore];
  return makeEnvironmentProviders(providers);
}
