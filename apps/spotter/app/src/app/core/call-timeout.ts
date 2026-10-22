import type { CommandError } from './api.types';

/** How long an ordinary call may take before the UI stops waiting and offers Retry. */
export const DEFAULT_TIMEOUT_MS = 15_000;
const LONG_TIMEOUT_MS = 120_000;
const MEDIUM_TIMEOUT_MS = 45_000;
/** Looking through a big share (the wizard's test) may use the whole scan limit. */
const SCAN_TIMEOUT_MS = 90_000;

/** Network checks, downloads and installs legitimately take longer than reads. */
const LONG = new Set(['download_release', 'install_renderer', 'install_app_update', 'check_app_update']);
const MEDIUM = new Set(['save_watch', 'discover_feeds', 'check_now', 'import_list']);

/** The time one command gets (pure). */
export function timeoutFor(cmd: string): number {
  return LONG.has(cmd) ? LONG_TIMEOUT_MS : cmd === 'test_source' ? SCAN_TIMEOUT_MS : MEDIUM.has(cmd) ? MEDIUM_TIMEOUT_MS : DEFAULT_TIMEOUT_MS;
}

/** The error a call that did not answer in time becomes. */
export function timeoutError(ms: number): CommandError {
  return {
    code: 'unavailable',
    message: `Spotter did not answer within ${Math.round(ms / 1000)} seconds. The machine may be busy; try again.`,
    problems: [],
  };
}

/** Runs `work`, giving up after `ms`; a late answer is ignored. */
export function withTimeout<T>(work: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(timeoutError(ms)), ms);
    work.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e: unknown) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}
