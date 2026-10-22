import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter, withHashLocation } from '@angular/router';
import { routes } from './app.routes';
import type { PitwallBackend } from './core/backend';
import { BACKEND, isTauri } from './core/backend';
import { MockBackend, type MockOptions } from './core/mock-backend';
import { TauriBackend } from './core/tauri-backend';
import { withTimeout } from './core/timeout-backend';
import { provideNativeFeel } from './core/native-feel';
import { provideIcons } from './shared/icons';

/**
 * Mock options from the browser URL, for UI tests and screenshots: `?mock-fail=overview,history` makes those calls
 * fail, `?mock-empty` starts with nothing, `?mock-static` stops the simulated live updates, `?mock-consent` shows the first-run service question and
 * `?mock-update` offers an app update, `?mock-updates` offers model updates.
 */
export function mockOptionsFrom(search: string): MockOptions {
  const q = new URLSearchParams(search);
  const fail = q.get('mock-fail');
  return {
    latencyMs: q.has('mock-fast') ? 0 : 120,
    autoTick: !q.has('mock-static'),
    streamDelayMs: q.has('mock-fast') ? 0 : 28,
    empty: q.has('mock-empty'),
    askConsent: q.has('mock-consent'),
    updateAvailable: q.has('mock-update'),
    modelUpdates: q.has('mock-updates'),
    crashed: q.has('mock-crash'),
    failing: fail ? (fail.split(',') as (keyof PitwallBackend)[]) : [],
  };
}

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withHashLocation()),
    provideIcons(),
    provideNativeFeel(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => withTimeout(isTauri() ? new TauriBackend() : new MockBackend(mockOptionsFrom(location.search))) },
  ],
};
