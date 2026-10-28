import { Injector, inject, type EnvironmentProviders, type Provider } from '@angular/core';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import { BACKEND, isTauri } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { TauriBackend } from '../core/tauri-backend';
import { provideIcons } from '../shared/icons';

/** Pitcrew's backend and icons for a dialog window that shows Pitcrew's own components (the real one in the app, the mock in the browser). */
export function backendProviders(): (Provider | EnvironmentProviders)[] {
  return [provideIcons(), {
      provide: BACKEND,
      useFactory: () => {
        // Long work started in this window is followed through its jobs store, looked up when a job starts (only a window that
        // provides the jobs, like the register window, can start one).
        const injector = inject(Injector);
        return isTauri() ? new TauriBackend(undefined, undefined, (id) => injector.get(JobsStore).follow(id)) : new MockBackend({ latencyMs: 0, simulate: false });
      },
    }];
}
