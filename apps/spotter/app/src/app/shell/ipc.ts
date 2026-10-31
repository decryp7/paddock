import { shellInvoke } from '@decryp7/paddock-ui/shell';
import type { TauriBridge } from '@decryp7/paddock-ui/shell/dialogs';

/** The part of Tauri's page globals Spotter's shell wiring uses (what `@tauri-apps/api` wraps). */
interface TauriInternals {
  invoke(cmd: string, args?: unknown): Promise<unknown>;
  transformCallback(cb: (event: { payload: unknown }) => void, once?: boolean): number;
  /** This webview's label (Tauri sets it before the page's scripts run). */
  metadata?: { currentWebview?: { label?: string } };
}

function internals(): TauriInternals | null {
  return (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__ ?? null;
}

/** The IPC every shell backend takes: any command name, plus events. */
export interface SpotterIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/**
 * The IPC inside Tauri, without `@tauri-apps/api` in the shell's backends: `null` in a plain browser, where the mock
 * backends serve. The shell's commands go to the `paddock-shell` plugin; Spotter's own pass through unchanged.
 */
export function tauriIpc(): SpotterIpc | null {
  const tauri = internals();
  if (!tauri) return null;
  return {
    invoke: shellInvoke(<T>(command: string, args?: Record<string, unknown>) => tauri.invoke(command, args ?? {}) as Promise<T>),
    listen: (event, handler) => listenTo(tauri, event, { kind: 'Any' }, (e) => handler(e.payload as never)),
  };
}

/** The dialog layer's bridge inside Tauri: the shell's commands, events for this window, and `emitTo`. */
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
