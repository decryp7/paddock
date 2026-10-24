import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { McpBackend } from './backend';
import { McpStore } from './mcp.store';

export interface ShellMcpOptions {
  /** Builds the window's backend (in an injection context): `new TauriMcpBackend(ipc)`, a fake in the browser. */
  readonly backend: () => McpBackend;
}

/**
 * The AI access settings for the Settings window's page (rule 32, 34): the backend and the per-window store. Only the
 * AI access page needs it; the page's `providers` module calls it, so no other window pays for it.
 */
export function provideShellMcp(options: ShellMcpOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: McpBackend, useFactory: options.backend }, McpStore];
  return makeEnvironmentProviders(providers);
}

/**
 * The key dialog's window needs the backend only, never the store (the key is not state): its `providers` module calls this.
 */
export function provideMcpBackend(options: ShellMcpOptions): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: McpBackend, useFactory: options.backend }]);
}
