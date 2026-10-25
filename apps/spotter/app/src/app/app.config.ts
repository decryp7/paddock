import { ApplicationConfig, provideAppInitializer, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter, withComponentInputBinding, withHashLocation } from '@angular/router';
import { routes } from './app.routes';
import { BACKEND, isTauri, type SpotterBackend } from './core/backend';
import { MockBackend, type MockScenario } from './core/mock-backend';
import { provideNativeFeel } from './core/native-feel';
import { TauriBackend, platformFromUserAgent } from './core/tauri-backend';
import { applyWindowChrome } from './core/window-chrome';
import { provideIcons } from './shared/icons';

const SCENARIOS: MockScenario[] = ['demo', 'empty', 'broken', 'products'];

/** The browser build's mock; `?mock=<scenario>` picks what it shows (development, screenshots and e2e tests). */
function browserBackend(): SpotterBackend {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const scenario = params.get('mock') as MockScenario | null;
  return new MockBackend({ scenario: scenario && SCENARIOS.includes(scenario) ? scenario : 'demo', latencyMs: 80, jobStepMs: 350 });
}

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withHashLocation(), withComponentInputBinding()),
    provideIcons(),
    provideNativeFeel(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : browserBackend()) },
    // Synchronous: no IPC before the first paint. The platform only picks the window chrome.
    provideAppInitializer(() => applyWindowChrome(document.documentElement, platformFromUserAgent(navigator.userAgent))),
  ],
};
