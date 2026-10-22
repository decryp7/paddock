import type { AppUpdateStatus, RunnerSnapshot } from './api.types';
import type { PitcrewBackend } from './backend';

/** Plausible demo data for the browser build and UI tests. Never the reference for behaviour (the engine is). */
export const DEMO_RUNNERS: RunnerSnapshot[] = [
  {
    name: 'build-pc',
    kind: 'gitea-runner',
    status: 'busy',
    currentVersion: '5.2.0',
    latestVersion: '5.2.0',
    lastUpdate: { result: 'updated', from: '5.1.0', to: '5.2.0', reason: null, finishedAtMs: Date.UTC(2026, 9, 21, 3, 12) },
    updatePhase: 'idle',
  },
  {
    name: 'mac-mini',
    kind: 'gitea-runner',
    status: 'stalled',
    currentVersion: '5.1.0',
    latestVersion: '5.2.0',
    lastUpdate: { result: 'rolledBack', from: '5.1.0', to: '5.2.0', reason: 'The server did not accept the new runner in time.', finishedAtMs: Date.UTC(2026, 9, 21, 3, 20) },
    updatePhase: 'idle',
  },
];

/** The browser backend. */
export class MockBackend implements PitcrewBackend {
  constructor(private readonly runners: RunnerSnapshot[] = DEMO_RUNNERS) {}

  async status(): Promise<RunnerSnapshot[]> {
    return structuredClone(this.runners);
  }

  async appUpdateStatus(): Promise<AppUpdateStatus> {
    return { available: false, reason: 'Updates are not available in the browser build.', currentVersion: '0.1.0' };
  }
}
