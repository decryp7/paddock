import { ApplicationConfig, provideBrowserGlobalErrorListeners, provideEnvironmentInitializer } from '@angular/core';
import { provideRouter, withComponentInputBinding, withHashLocation } from '@angular/router';
import { routes } from './app.routes';
import { BACKEND, isTauri } from './core/backend';
import { MockBackend, applyMockPlatform, mockOptionsFromUrl } from './core/mock-backend';
import { provideNativeFeel } from './core/native-feel';
import { TauriBackend } from './core/tauri-backend';
import { provideIcons } from './shared/icons';

/** The query string as the page was opened: the hash router rewrites the URL (dropping `?mock=…`) once it starts. */
const OPENED_WITH = typeof window === 'undefined' ? '' : window.location.search;

/**
 * Nothing here awaits the backend: initializers return void and start any work in the background, so the first paint
 * never waits for IPC (a window whose first call goes unanswered must still render).
 */
export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withComponentInputBinding(), withHashLocation()),
    provideIcons(),
    provideNativeFeel(),
    provideEnvironmentInitializer(() => {
      // In the browser build the mock pretends to be a machine of a given OS, and the window chrome follows it.
      if (!isTauri()) applyMockPlatform(OPENED_WITH, document.documentElement);
    }),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : new MockBackend(mockOptionsFromUrl(OPENED_WITH))) },
  ],
};
