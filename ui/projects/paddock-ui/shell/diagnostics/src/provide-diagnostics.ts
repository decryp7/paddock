import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { DiagnosticsBackend } from './backend';
import { DiagnosticsStore } from './diagnostics.store';

export interface ShellDiagnosticsOptions {
  /** Builds the window's backend (in an injection context): `new TauriDiagnosticsBackend(ipc)`, a fake in the browser. */
  readonly backend: () => DiagnosticsBackend;
}

/**
 * Report a problem for one dialog window (rule 32, 34): the backend and the per-window store. Only the Report a Problem
 * window needs it; the dialog's `providers` module calls it, so no other window pays for it.
 */
export function provideShellDiagnostics(options: ShellDiagnosticsOptions): EnvironmentProviders {
  const providers: Provider[] = [{ provide: DiagnosticsBackend, useFactory: options.backend }, DiagnosticsStore];
  return makeEnvironmentProviders(providers);
}
