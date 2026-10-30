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
import { Toasts } from './core/toasts';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { NOTIFICATION_SOURCES } from '@decryp7/paddock-ui/shell/notify';

/**
 * Mock options from the browser URL, for UI tests and screenshots: `?mock-fail=overview,history` makes those calls
 * fail, `?mock-empty` starts with nothing, `?mock-static` stops the simulated live updates, `?mock-down` takes the server down (the service controls keep working), `?mock-startfailed` shows launchd refusing the service after an update, `?mock-consent` shows the first-run service question and
 * `?mock-update` offers an app update, `?mock-updates` offers model updates, `?mock-install` shows llama.cpp installing itself and `?mock-quit`
 * opens the "Quit Pitwall?" question, `?mock-clean` leaves no old files on disk.
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
    serverDown: q.has('mock-down'),
    startFailed: q.has('mock-startfailed'),
    updateAvailable: q.has('mock-update'),
    modelUpdates: q.has('mock-updates'),
    crashed: q.has('mock-crash'),
    installing: q.has('mock-install'),
    noLeftovers: q.has('mock-clean'),
    quitAsk: q.has('mock-quit'),
    failing: fail ? (fail.split(',') as (keyof PitwallBackend)[]) : [],
  };
}

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withHashLocation()),
    provideIcons(),
    // Material's family defaults (outlined fields, no ripples, plain toggles, tooltip delay): rule 47, every window.
    providePaddockMaterial(),
    // Pitwall's toasts go to the family's one toast host (`pd-notification-host` in the root component).
    { provide: NOTIFICATION_SOURCES, multi: true, useExisting: Toasts },
    provideNativeFeel(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => withTimeout(isTauri() ? new TauriBackend() : new MockBackend(mockOptionsFrom(location.search))) },
  ],
};
