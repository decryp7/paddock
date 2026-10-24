import { shellInvoke } from '@decryp7/paddock-ui/shell';
import type { TauriBridge } from '@decryp7/paddock-ui/shell/dialogs';
import { workbenchInvoke } from '@decryp7/paddock-ui/workbench';
import type { PanelsBridge, TauriWindowBridge } from '@decryp7/paddock-ui/workbench/panels';
import type { AssistantIpc } from '@decryp7/paddock-ui/workbench/assistant';
import type { PitwallIpc } from '@decryp7/paddock-ui/shell/pitwall';

/*
 * The desktop build's seam to Tauri, without `@tauri-apps/api` (an app passes `invoke` and `listen` from it): everything
 * here is `null` in a plain browser, where the mock backends serve (`inTauri()` of `@decryp7/paddock-ui/ipc` decides). The
 * e2e installs a fake `__TAURI_INTERNALS__` to hold and answer the calls.
 */

/** The part of Tauri's page globals the demo uses (what `@tauri-apps/api` wraps). */
export interface TauriInternals {
  invoke(cmd: string, args?: unknown): Promise<unknown>;
  transformCallback(cb: (event: { payload: unknown }) => void, once?: boolean): number;
  /** This webview's label (Tauri sets it before the page's scripts run). */
  metadata?: { currentWebview?: { label?: string } };
}

export function internals(globals: object = globalThis): TauriInternals | null {
  return (globals as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__ ?? null;
}

/** Any command name, so it serves every backend's port (`StartupIpc`, `PitwallIpc`, `AssistantIpc`, ...). */
export interface DesktopIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

type Target = { kind: 'Any' } | { kind: 'WebviewWindow'; label: string };

/** `plugin:event|listen` as `@tauri-apps/api/event` sends it; resolves to the unlisten. */
async function listenTo(tauri: TauriInternals, event: string, target: Target, handler: (e: { payload: unknown }) => void): Promise<() => void> {
  const callback = tauri.transformCallback(handler);
  const eventId = await tauri.invoke('plugin:event|listen', { event, target, handler: callback });
  return () => void tauri.invoke('plugin:event|unlisten', { event, eventId }).catch(() => undefined);
}

/** This window's label (`main`, `p-main-1`, `d-main-1`). */
export function windowLabel(tauri: TauriInternals): string {
  return tauri.metadata?.currentWebview?.label ?? 'main';
}

/** The plain `invoke`: the plugins' commands by their full names (the dock's layout store names them itself). */
export function rawInvoke(): (<T>(command: string, args?: Record<string, unknown>) => Promise<T>) | null {
  const tauri = internals();
  return tauri ? <T>(command: string, args?: Record<string, unknown>) => tauri.invoke(command, args ?? {}) as Promise<T> : null;
}

/**
 * The window's IPC inside Tauri: the shell's commands go to `paddock-shell`, the workbench's (layouts, panels, assistant) to
 * `paddock-workbench`, the app's own (`pitwall_state`, `demo_reindex`) as they are.
 */
export function desktopIpc(): DesktopIpc | null {
  const tauri = internals();
  const raw = rawInvoke();
  if (!tauri || !raw) return null;
  return {
    invoke: workbenchInvoke(shellInvoke(raw)),
    listen: (event, handler) => listenTo(tauri, event, { kind: 'Any' }, (e) => handler(e.payload as never)),
  };
}

/** The dialog layer's bridge: events sent to this window only, and `emitTo` a window by label. */
export function dialogBridge(): TauriBridge | null {
  const tauri = internals();
  const ipc = desktopIpc();
  if (!tauri || !ipc) return null;
  const label = windowLabel(tauri);
  return {
    invoke: ipc.invoke,
    listen: (event, handler) => listenTo(tauri, event, { kind: 'WebviewWindow', label }, handler),
    emitTo: async (target, event, payload) => {
      await tauri.invoke('plugin:event|emit_to', { target: { kind: 'AnyLabel', label: target }, event, payload });
    },
  };
}

/** The panel layer's bridge (`tauriPanelsIo(bridge)`): events of this window, the label of this window. */
export function panelsBridge(): PanelsBridge | null {
  const tauri = internals();
  const ipc = desktopIpc();
  if (!tauri || !ipc) return null;
  const label = windowLabel(tauri);
  return { invoke: ipc.invoke, listen: (event, handler) => listenTo(tauri, event, { kind: 'WebviewWindow', label }, handler), label: () => label };
}

/** The assistant's IPC: its commands and the `paddock://assistant` event (every window hears it). */
export function assistantIpc(): AssistantIpc | null {
  return desktopIpc();
}

/** The needs-AI component's IPC: the app's four `pitwall_*` commands and the `paddock://pitwall` event. */
export function pitwallIpc(): PitwallIpc | null {
  return desktopIpc();
}

/** The pop-out's window controls (`tauriWindowChrome`), over the window plugin's commands. */
export function windowBridge(): TauriWindowBridge | null {
  const tauri = internals();
  if (!tauri) return null;
  const label = windowLabel(tauri);
  const call = <T>(command: string, args: Record<string, unknown> = {}) => tauri.invoke(`plugin:window|${command}`, { label, ...args }) as Promise<T>;
  return {
    minimize: () => call<void>('minimize'),
    toggleMaximize: () => call<void>('internal_toggle_maximize'),
    close: () => call<void>('close'),
    isMaximized: () => call<boolean>('is_maximized'),
    onResized: (handler) => listenTo(tauri, 'tauri://resize', { kind: 'WebviewWindow', label }, () => handler()),
    startDragging: () => call<void>('start_dragging'),
    startResizeDragging: (direction) => call<void>('start_resize_dragging', { value: direction }),
  };
}

/** Whether the desktop app runs on macOS (the pop-out keeps the native traffic lights there). */
export function onMac(): boolean {
  return typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
}
