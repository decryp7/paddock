import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { StorageBackend } from './backend';
import { StorageStore } from './storage.store';

export interface ShellStorageOptions {
  /** Builds the window's backend (in an injection context): `new TauriStorageBackend(ipc)`, a `MemoryStorageBackend` in the browser. */
  readonly backend: () => StorageBackend;
}

/**
 * What the app keeps on disk, for the Settings window's Storage page (rule 32, 34): the backend and the per-window store. Only
 * the Storage page needs it; the page's `providers` module calls it, so no other window pays for it.
 */
export function provideShellStorage(options: ShellStorageOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: StorageBackend, useFactory: options.backend }, StorageStore];
  return makeEnvironmentProviders(providers);
}
