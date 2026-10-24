
/** The part of Tauri's page globals the demo uses (what `@tauri-apps/api` wraps). */
interface TauriInternals {
  invoke(cmd: string, args?: unknown): Promise<unknown>;
  transformCallback(cb: (event: { payload: unknown }) => void, once?: boolean): number;
}

/** The demo's IPC: any command name (each shell part narrows it to its own table: `StartupIpc`, `AboutIpc`, ...). */
export interface DemoIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/**
 * The demo's IPC inside Tauri, without `@tauri-apps/api` (an app passes `invoke` and `listen` from it): `null` in a plain
 * browser, where the mock backends serve. The e2e installs a fake `__TAURI_INTERNALS__` to hold and answer the calls.
 */
export function tauriIpc(): DemoIpc | null {
  const internals = (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
  if (!internals) return null;
  return {
    invoke: <T>(command: string, args?: Record<string, unknown>) => internals.invoke(command, args ?? {}) as Promise<T>,
    listen: async <T>(event: string, handler: (payload: T) => void) => {
      const callback = internals.transformCallback((e) => handler(e.payload as T));
      const eventId = await internals.invoke('plugin:event|listen', { event, target: { kind: 'Any' }, handler: callback });
      return () => void internals.invoke('plugin:event|unlisten', { event, eventId }).catch(() => undefined);
    },
  };
}
