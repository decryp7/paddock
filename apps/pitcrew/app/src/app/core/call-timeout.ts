import type { CommandError } from './api.types';

/** How long an ordinary call may take before the UI stops waiting and offers Retry. */
export const DEFAULT_TIMEOUT_MS = 15_000;
/** Calls that scan or delete folders, download, or elevate: they legitimately take longer. */
const LONG_TIMEOUT_MS = 120_000;
const MEDIUM_TIMEOUT_MS = 60_000;

const LONG = new Set(['engine_prune', 'cache_clean', 'channel_test', 'clean_disk', 'register_runner', 'install_service', 'uninstall_service', 'install_app_update', 'check_app_update', 'export_diagnostics']);
const MEDIUM = new Set(['fleet_insights', 'engine_report', 'engine_prune_preview', 'cache_report', 'cache_preview', 'open_in_debrief', 'runner_disk', 'add_runner', 'remove_runner', 'save_settings']);
/** Reading, previewing and process queries are quick; saving a config file may wait for the runner to restart. */
const QUICK = new Set(['runner_config_get', 'runner_config_preview', 'runner_processes', 'runner_process_keep']);
const SAVE = new Set(['runner_config_save', 'runner_process_stop']);
const QUICK_TIMEOUT_MS = 10_000;
const SAVE_TIMEOUT_MS = 30_000;

/** The time one command gets (pure). */
export function timeoutFor(cmd: string): number {
  if (QUICK.has(cmd)) return QUICK_TIMEOUT_MS;
  if (SAVE.has(cmd)) return SAVE_TIMEOUT_MS;
  return LONG.has(cmd) ? LONG_TIMEOUT_MS : MEDIUM.has(cmd) ? MEDIUM_TIMEOUT_MS : DEFAULT_TIMEOUT_MS;
}

/** The error a call that did not answer in time becomes. */
export function timeoutError(ms: number): CommandError {
  return {
    code: 'timeout',
    message: `Pitcrew did not answer within ${Math.round(ms / 1000)} seconds. The machine may be busy or the service stuck; try again.`,
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
