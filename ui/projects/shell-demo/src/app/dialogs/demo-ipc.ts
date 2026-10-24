import { inject, Injectable, InjectionToken } from '@angular/core';

/** What `demo_status` answers. */
export interface DemoStatus {
  readonly engine: string;
  readonly uptimeSeconds: number;
  readonly queued: number;
}

/** The demo's IPC seam: Tauri's internals when the page runs in a webview, else the mock below (browser build). */
export type DemoInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface TauriHost {
  __TAURI_INTERNALS__?: { invoke: DemoInvoke };
}

const MOCK: Readonly<Record<string, unknown>> = {
  demo_status: { engine: 'running', uptimeSeconds: 754, queued: 2 } satisfies DemoStatus,
};

export const DEMO_INVOKE = new InjectionToken<DemoInvoke>('DEMO_INVOKE', {
  providedIn: 'root',
  factory: () => {
    return <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      const tauri = (globalThis as TauriHost).__TAURI_INTERNALS__;
      if (tauri) return tauri.invoke<T>(command, args ?? {});
      return command in MOCK ? Promise.resolve(MOCK[command] as T) : Promise.reject(new Error(`the mock backend has no ${command}`));
    };
  },
});

/** The engine as the inspector sees it. Provided only in the windows that show the inspector (its `providers`). */
@Injectable()
export class DemoEngine {
  private readonly invoke = inject(DEMO_INVOKE);

  status(): Promise<DemoStatus> {
    return this.invoke<DemoStatus>('demo_status');
  }
}

/** The inspector dialog's providers (loaded with it, never in the main window's first paint). */
export const INSPECTOR_PROVIDERS = [DemoEngine];
