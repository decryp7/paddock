import { ApplicationConfig, inject, provideAppInitializer, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter, withComponentInputBinding, withHashLocation } from '@angular/router';
import { routes } from './app.routes';
import { BACKEND, isTauri, type PitcrewBackend } from './core/backend';
import { MockBackend, type MockScenario } from './core/mock-backend';
import { provideNativeFeel } from './core/native-feel';
import { applyWindowChrome } from './core/window-chrome';
import { TauriBackend } from './core/tauri-backend';
import { provideIcons } from './shared/icons';

const SCENARIOS: MockScenario[] = ['demo', 'empty', 'unavailable', 'notInstalled', 'mismatch'];

/** The browser build's mock; `?mock=<scenario>&platform=<os>` picks what it shows (development and screenshots). */
function browserBackend(): PitcrewBackend {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const scenario = params.get('mock') as MockScenario | null;
  const platform = params.get('platform');
  return new MockBackend({
    scenario: scenario && SCENARIOS.includes(scenario) ? scenario : 'demo',
    latencyMs: 120,
    simulate: params.get('simulate') !== '0',
    platform: platform === 'macos' || platform === 'windows' || platform === 'linux' ? platform : 'browser',
  });
}

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withHashLocation(), withComponentInputBinding()),
    provideIcons(),
    provideNativeFeel(),
    // Inside the app the real Tauri backend; in the browser build (development, UI tests) the mock.
    { provide: BACKEND, useFactory: () => (isTauri() ? new TauriBackend() : browserBackend()) },
    provideAppInitializer(async () => {
      const backend = inject(BACKEND);
      if (backend instanceof TauriBackend) await backend.init();
      applyWindowChrome(document.documentElement, backend.platform());
    }),
  ],
};
