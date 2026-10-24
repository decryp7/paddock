import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { PitwallBackend } from './backend';
import { PitwallStore } from './pitwall.store';

export interface PitwallOptions {
  /** Builds the window's backend (in an injection context): `new TauriPitwallBackend(ipc)`, a fake in the browser. */
  readonly backend: () => PitwallBackend;
}

/**
 * The needs-AI state for one window or page (rules 32, 34): the backend and the store, so every `<pd-needs-ai>` below it
 * shares one snapshot and one listener. Provide it at the narrowest scope that has AI features.
 */
export function providePitwall(options: PitwallOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: PitwallBackend, useFactory: options.backend }, PitwallStore];
  return makeEnvironmentProviders(providers);
}
