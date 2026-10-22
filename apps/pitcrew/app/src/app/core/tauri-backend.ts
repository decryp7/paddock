import { invoke } from '@tauri-apps/api/core';
import type { AppUpdateStatus, RunnerSnapshot } from './api.types';
import type { PitcrewBackend } from './backend';
import { toCommandError } from './command-error';

/** The real backend: the Tauri commands of `app/src-tauri/src/commands.rs`. */
export class TauriBackend implements PitcrewBackend {
  constructor(private readonly call: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T> = invoke) {}

  status(): Promise<RunnerSnapshot[]> {
    return this.run<RunnerSnapshot[]>('runner_status');
  }

  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.run<AppUpdateStatus>('app_update_status');
  }

  private async run<T>(cmd: string): Promise<T> {
    try {
      return await this.call<T>(cmd);
    } catch (e) {
      throw toCommandError(e);
    }
  }
}
