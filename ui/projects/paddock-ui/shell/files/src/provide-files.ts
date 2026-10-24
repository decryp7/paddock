import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { FilesBackend } from './backend';
import { FilesStore } from './files.store';

export interface ShellFilesOptions {
  /** Builds the window's backend (in an injection context): `new TauriFilesBackend(ipc)`, a `MemoryFilesBackend` in the browser. */
  readonly backend: () => FilesBackend;
}

/**
 * The user's files for a window or page (rule 32, 34): the backend and the store. Provide it at the narrowest scope that needs
 * it (a page's providers, a dialog window's), so no other window pays for it.
 */
export function provideShellFiles(options: ShellFilesOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: FilesBackend, useFactory: options.backend }, FilesStore];
  return makeEnvironmentProviders(providers);
}
