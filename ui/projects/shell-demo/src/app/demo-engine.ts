import { inject, InjectionToken } from '@angular/core';
import { DemoHealthEngine } from './demo-health';
import { tauriIpc, type DemoIpc } from './demo-ipc';
import { DemoJobsEngine } from './demo-jobs';

/**
 * What the demo's commands ask of its engine: the Rust engine of the desktop app (its `src-tauri`), or the
 * in-memory stand-ins in the browser build. Each returns once the engine took the request; a refusal rejects, and the
 * shell's command service shows it.
 */
export interface DemoEngineActions {
  /** The sample import (phases on their own scales, a step that cannot be cancelled); resolves to its job id. */
  startImport(): Promise<number>;
  /** A job that reports nothing until it is cancelled ("Still working"); resolves to its job id. */
  startQuiet(): Promise<number>;
  /** The demo engine fails (its Restart works). */
  breakEngine(): Promise<void>;
  /** The search index fails (its Reset fails too). */
  breakIndex(): Promise<void>;
}

/** The desktop app's engine: the demo's own commands (`APP_COMMANDS`). */
export function tauriEngine(ipc: Pick<DemoIpc, 'invoke'>): DemoEngineActions {
  return {
    startImport: () => ipc.invoke<number>('demo_job_start', { kind: 'import' }),
    startQuiet: () => ipc.invoke<number>('demo_job_start', { kind: 'quiet' }),
    breakEngine: () => ipc.invoke<void>('demo_break', { service: 'sd.engine' }),
    breakIndex: () => ipc.invoke<void>('demo_break', { service: 'sd.index' }),
  };
}

/** The browser build's engine: the in-memory stand-ins of `paddock-jobs` and `paddock-health`. */
export function memoryEngine(jobs: DemoJobsEngine, health: DemoHealthEngine): DemoEngineActions {
  return {
    startImport: async () => jobs.startImport(),
    startQuiet: async () => jobs.startQuiet(),
    breakEngine: async () => health.breakEngine(),
    breakIndex: async () => health.breakIndex(),
  };
}

/** This window's engine: Rust's in the desktop app, the stand-ins in the browser. */
export const DEMO_ENGINE = new InjectionToken<DemoEngineActions>('DEMO_ENGINE', {
  providedIn: 'root',
  factory: () => {
    const ipc = tauriIpc();
    return ipc ? tauriEngine(ipc) : memoryEngine(inject(DemoJobsEngine), inject(DemoHealthEngine));
  },
});
