import type { PitwallBackend } from './backend';
import { isTauri } from './backend';
import { MockBackend, type MockOptions } from './mock-backend';
import { TauriBackend } from './tauri-backend';
import { withTimeout } from './timeout-backend';

/**
 * Mock options from the browser URL, for UI tests and screenshots: `?mock-fail=overview,history` makes those calls
 * fail, `?mock-empty` starts with nothing, `?mock-static` stops the simulated live updates, `?mock-down` takes the server
 * down (the service controls keep working), `?mock-startfailed` shows launchd refusing the service after an update,
 * `?mock-consent` shows the first-run service question, `?mock-updates` offers model updates, `?mock-install` shows
 * llama.cpp installing itself, `?mock-quit` opens the "Quit Pitwall?" question and `?mock-clean` leaves no old files on
 * disk.
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
    modelUpdates: q.has('mock-updates'),
    crashed: q.has('mock-crash'),
    installing: q.has('mock-install'),
    noLeftovers: q.has('mock-clean'),
    quitAsk: q.has('mock-quit'),
    failing: fail ? (fail.split(',') as (keyof PitwallBackend)[]) : [],
  };
}

/** The query string as the page was opened: the hash router may rewrite the URL once it starts. */
const OPENED_WITH = typeof window === 'undefined' ? '' : window.location.search;

let browser: PitwallBackend | null = null;

/**
 * The window's backend: the real Tauri backend in the app; in the browser build (development, UI tests) the mock, one per
 * page so the window and its in-page dialogs share it. Either is wrapped in the per-call timeouts (jobs have none).
 */
export function createBackend(search: string = OPENED_WITH): PitwallBackend {
  if (isTauri()) return withTimeout(new TauriBackend());
  return (browser ??= withTimeout(new MockBackend(mockOptionsFrom(search))));
}

/** Forgets the browser build's one backend (specs that boot a window more than once). */
export function resetBrowserBackend(): void {
  browser = null;
}
