
import { shellInvoke } from '@decryp7/paddock-ui/shell';
import type { TauriBridge } from '@decryp7/paddock-ui/shell/dialogs';

/** The part of Tauri's page globals the demo uses (what `@tauri-apps/api` wraps). */
interface TauriInternals {
  invoke(cmd: string, args?: unknown): Promise<unknown>;
  transformCallback(cb: (event: { payload: unknown }) => void, once?: boolean): number;
  /** This webview's label (Tauri sets it before the page's scripts run). */
  metadata?: { currentWebview?: { label?: string } };
}

function internals(): TauriInternals | null {
  return (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__ ?? null;
}

/** The demo's IPC: any command name, so it serves every shell backend's port (`StartupIpc`, `SettingsIpc`, `AboutIpc`, ...). */
export interface DemoIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/**
 * The demo's IPC inside Tauri, without `@tauri-apps/api` (an app passes `invoke` and `listen` from it): `null` in a plain
 * browser, where the mock backends serve. The e2e installs a fake `__TAURI_INTERNALS__` to hold and answer the calls.
 */
export function tauriIpc(): DemoIpc | null {
  const tauri = internals();
  if (!tauri) return null;
  return {
    // The shell's commands go to the `paddock-shell` plugin (`plugin:paddock-shell|<command>`), the demo's own as they are.
    invoke: shellInvoke(<T>(command: string, args?: Record<string, unknown>) => tauri.invoke(command, args ?? {}) as Promise<T>),
    listen: (event, handler) => listenTo(tauri, event, { kind: 'Any' }, (e) => handler(e.payload as never)),
  };
}

/**
 * The dialog layer's bridge inside Tauri (`tauriDialogIo(bridge)`): the shell's commands, events sent to this window only
 * (`getCurrentWebviewWindow().listen` in `@tauri-apps/api`) and `emitTo` a window by label. `null` in a plain browser.
 */
export function tauriBridge(): TauriBridge | null {
  const tauri = internals();
  const ipc = tauriIpc();
  if (!tauri || !ipc) return null;
  const label = tauri.metadata?.currentWebview?.label ?? 'main';
  return {
    invoke: ipc.invoke,
    listen: (event, handler) => listenTo(tauri, event, { kind: 'WebviewWindow', label }, handler),
    emitTo: async (target, event, payload) => {
      await tauri.invoke('plugin:event|emit_to', { target: { kind: 'AnyLabel', label: target }, event, payload });
    },
  };
}

type EventTarget = { kind: 'Any' } | { kind: 'WebviewWindow'; label: string };

/** `plugin:event|listen` as `@tauri-apps/api/event` sends it; resolves to the unlisten. */
async function listenTo(tauri: TauriInternals, event: string, target: EventTarget, handler: (e: { payload: unknown }) => void): Promise<() => void> {
  const callback = tauri.transformCallback(handler);
  const eventId = await tauri.invoke('plugin:event|listen', { event, target, handler: callback });
  return () => void tauri.invoke('plugin:event|unlisten', { event, eventId }).catch(() => undefined);
}

