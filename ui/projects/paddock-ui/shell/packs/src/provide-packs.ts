import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { PacksBackend } from './backend';
import { PacksStore } from './packs.store';

export interface ShellPacksOptions {
  /** Builds the window's backend (in an injection context): `new TauriPacksBackend(ipc)`, a fake in the browser. */
  readonly backend: () => PacksBackend;
}

/** The knowledge packs for the Settings window's page (rule 32, 34): the backend and the per-window store. */
export function provideShellPacks(options: ShellPacksOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: PacksBackend, useFactory: options.backend }, PacksStore];
  return makeEnvironmentProviders(providers);
}
