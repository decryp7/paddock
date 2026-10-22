import { InjectionToken } from '@angular/core';
import type { AppUpdateStatus, RunnerSnapshot } from './api.types';

/**
 * Everything the UI asks the backend. Components never call Tauri directly: they get this through DI, the real Tauri
 * backend in the app and the mock in the browser build and tests.
 */
export interface PitcrewBackend {
  /** Every runner's status. */
  status(): Promise<RunnerSnapshot[]>;
  /** Whether Pitcrew can update itself. */
  appUpdateStatus(): Promise<AppUpdateStatus>;
}

export const BACKEND = new InjectionToken<PitcrewBackend>('PitcrewBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
