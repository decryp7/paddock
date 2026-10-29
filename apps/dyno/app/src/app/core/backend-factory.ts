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
  return (browser ??= new MockBackend(mockOptionsFromUrl(OPENED_WITH)));
}
