import type { EnvironmentProviders, Provider } from '@angular/core';
import { BACKEND, isTauri } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { TauriBackend } from '../core/tauri-backend';
import { provideIcons } from '../shared/icons';

/** Pitcrew's backend and icons for a dialog window that shows Pitcrew's own components (the real one in the app, the mock in the browser). */
export function backendProviders(): (Provider | EnvironmentProviders)[] {
  return [provideIcons(), { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : new MockBackend({ latencyMs: 0, simulate: false })) }];
}
