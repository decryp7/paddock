import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter, withComponentInputBinding, withHashLocation } from '@angular/router';
import { routes } from './app.routes';
import { BACKEND, isTauri } from './core/backend';
import { MockBackend, mockOptionsFromUrl } from './core/mock-backend';
import { TauriBackend } from './core/tauri-backend';
import { provideIcons } from './shared/icons';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withComponentInputBinding(), withHashLocation()),
    provideIcons(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : new MockBackend(mockOptionsFromUrl(window.location.search))) },
  ],
};
