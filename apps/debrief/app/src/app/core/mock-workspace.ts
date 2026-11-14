import { InjectionToken } from '@angular/core';
import type { MockBackend } from './mock/mock-backend';

/**
 * The raw mock workspace of the browser build: the `MockBackend` that computes answers, without the `?mockFail=` failure
 * injection and the logging that `provideBackend` puts around the backend the UI receives. Every mock backend (home, share,
 * search, analysis ...) reads its workspace through this token, never through `DebriefBackend`: a mock's own read is no call of
 * the UI, so it must not use up a failure simulated for the UI's calls (one boundary, not a rule per command).
 *
 * No default: `provideBackend` provides it, a test provides it (`provideTinyBackendBare`), and a mock built without it fails
 * at once with NG0201 instead of silently reading the UI-facing backend.
 */
export const MOCK_WORKSPACE = new InjectionToken<MockBackend>('MOCK_WORKSPACE');
