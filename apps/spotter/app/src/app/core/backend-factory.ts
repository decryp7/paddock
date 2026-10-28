import { inject } from '@angular/core';
import { isTauri, type SpotterBackend } from './backend';
import { MockBackend, type MockScenario } from './mock-backend';
import { TauriBackend } from './tauri-backend';
import { BrowserEngines } from '../shell/engines';

const SCENARIOS: MockScenario[] = ['demo', 'empty', 'broken', 'products'];

let browser: SpotterBackend | null = null;

/**
 * The window's backend (in an injection context): the real Tauri backend in the app; in the browser build (development,
 * UI tests) the mock, one per page so the window and its in-page dialogs share it. `?mock=<scenario>` picks what it shows.
 */
export function createBackend(): SpotterBackend {
  if (isTauri()) return new TauriBackend();
  if (browser) return browser;
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const scenario = params.get('mock') as MockScenario | null;
  const engines = inject(BrowserEngines);
  browser = new MockBackend({
    scenario: scenario && SCENARIOS.includes(scenario) ? scenario : 'demo',
    latencyMs: 80,
    jobStepMs: engines.mockJobs ? Number(params.get('jobStepMs') ?? 350) : 350,
    jobs: engines.mockJobs,
  });
  return browser;
}
