import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { ChannelsBackend } from './backend';
import { ChannelsStore } from './channels.store';

export interface ShellChannelsOptions {
  /** Builds the window's backend (in an injection context): `new TauriChannelsBackend(ipc)`, a fake in the browser. */
  readonly backend: () => ChannelsBackend;
}

/**
 * The notification channels for the Settings window's page (rule 32, 34): the backend and the per-window store. Only the
 * Notification channels page needs it; the page's `providers` module calls it, so no other window pays for it.
 */
export function provideShellChannels(options: ShellChannelsOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: ChannelsBackend, useFactory: options.backend }, ChannelsStore];
  return makeEnvironmentProviders(providers);
}

/**
 * The secret dialog's window needs the backend only, never the store (the secret is not state): its `providers` module calls this.
 */
export function provideChannelsBackend(options: ShellChannelsOptions): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: ChannelsBackend, useFactory: options.backend }]);
}
