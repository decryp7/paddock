import type { DebriefBackend } from './backend';

/** The raw mock behind each UI-facing backend of the browser build (see `registerRawBackend`). */
const raws = new WeakMap<object, DebriefBackend>();

/**
 * Records `raw`, the mock workspace as it computes answers, as what sits behind `uiFacing`, the backend the UI receives
 * (the mock behind the failure injection and the failure-logging proxy). Called by `provideBackend` only.
 */
export function registerRawBackend(uiFacing: DebriefBackend, raw: DebriefBackend): void {
  raws.set(uiFacing, raw);
}

/**
 * The backend a mock backend (home, share, search, analysis ...) reads its workspace through: the raw mock behind the one the
 * UI got, or `backend` itself where it is not wrapped (a test's own backend). A mock's own read is no call of the UI, so it
 * never takes a failure that `?mockFail=` simulates for the UI's calls (the class fixed in one place, not per command).
 */
export function rawBackend(backend: DebriefBackend): DebriefBackend {
  return raws.get(backend) ?? backend;
}
