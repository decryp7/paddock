import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { BACKEND, isTauri } from './core/backend';
import { MockBackend } from './core/mock-backend';
import { TauriBackend } from './core/tauri-backend';
import { provideIcons } from './shared/icons';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideIcons(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : new MockBackend()) },
  ],
};
