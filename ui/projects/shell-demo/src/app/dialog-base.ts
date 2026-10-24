import type { EnvironmentProviders, Provider } from '@angular/core';

/**
 * What every dialog window of the demo boots with besides the shell's set: nothing yet (an app adds its IPC backend,
 * logger and icons here; the boot harness uses the same function, so a provider missing here fails in a spec).
 */
export function dialogBaseProviders(): (Provider | EnvironmentProviders)[] {
  return [];
}
