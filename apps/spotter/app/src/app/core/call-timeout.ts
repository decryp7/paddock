import type { CommandError } from './api.types';

/** How long an ordinary call may take before the UI stops waiting and offers Retry. */
export const DEFAULT_TIMEOUT_MS = 15_000;
const LONG_TIMEOUT_MS = 120_000;
const MEDIUM_TIMEOUT_MS = 45_000;

/** Updates are plain calls that legitimately take longer than reads. */
const LONG = new Set(['install_app_update', 'check_app_update']);
const MEDIUM = new Set(['save_watch', 'discover_feeds', 'import_list']);

/**
 * Commands that run as a job (`spotter://job` events with their phase and progress, and Cancel). The UI never gives
 * up on these: the engine ends them, and while they run the UI shows what they are doing.
 */
const JOBS = new Set(['download_release', 'install_renderer', 'check_now', 'test_source']);

/** Whether a command is a long-running job the UI waits for without a limit. */
export function isJobCommand(cmd: string): boolean {
  return JOBS.has(cmd);
}

/** The time one command gets (pure); `null` for a job, which has no UI timeout. */
export function timeoutFor(cmd: string): number | null {
  if (JOBS.has(cmd)) return null;
  return LONG.has(cmd) ? LONG_TIMEOUT_MS : MEDIUM.has(cmd) ? MEDIUM_TIMEOUT_MS : DEFAULT_TIMEOUT_MS;
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
