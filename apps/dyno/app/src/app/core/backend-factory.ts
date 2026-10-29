import { inject } from '@angular/core';
import { BrowserEngines } from '../shell/engines';
import { isTauri, type DynoBackend } from './backend';
import { MockBackend, applyMockPlatform, mockOptionsFromUrl } from './mock-backend';
import { TauriBackend } from './tauri-backend';

/** The query string as the page was opened: the hash router rewrites the URL (dropping `?mock=…`) once it starts. */
const OPENED_WITH = typeof window === 'undefined' ? '' : window.location.search;

let browser: DynoBackend | null = null;

/**
 * The window's backend: the real Tauri backend in the app; in the browser build (development, UI tests) the mock, one per
 * page so the window and its in-page dialogs share it. `?mock=…`, `fail=…` and `latency=…` shape it.
 */
export function createBackend(): DynoBackend {
  if (isTauri()) return new TauriBackend();
  if (typeof document !== 'undefined') applyMockPlatform(OPENED_WITH, document.documentElement);
  if (browser) return browser;
  // The mock plays the agent: its jobs reach the shell's jobs store through the in-memory jobs backend, and the Jobs view's
  // Cancel reaches the mock.
  const engines = inject(BrowserEngines);
  const mock = new MockBackend({ ...mockOptionsFromUrl(OPENED_WITH), jobs: engines.jobs });
  engines.cancelJob = (id) => void mock.cancelJob(id);
  return (browser = mock);
}
