import { InjectionToken } from '@angular/core';
import { InPageDesktop } from './in-page';
import type { PanelCommand } from '../generated/panel-commands';
import { PANEL_EVENTS, type GhostRequest, type PanelsEvent, type PopOutBounds, type PopOutInfo, type PopOutRequest, type SyncPublish } from '../generated/panels';

/** Stops listening. */
export type Unlisten = () => void;

/** The drag ghost's calls (`drag_ghost_*`): one session per drag. */
export interface GhostIo {
  /** Is there a ghost here at all? (Not in a plain browser.) */
  readonly available: boolean;
  /** Starts a session; false where the ghost cannot follow the pointer (Wayland, no displays): keep the in-window preview. */
  start(request: GhostRequest): Promise<boolean>;
  show(visible: boolean): Promise<void>;
  /** Ends the session and hides the ghost: where it was (logical px of its display, with the display's scale), or null when it was not showing. */
  end(): Promise<PopOutBounds | null>;
}

/**
 * Everything the panel layer needs from the window system, in one seam (code against abstractions): the desktop build
 * uses `tauriPanelsIo` over the app's Tauri API, the browser build and the specs use `InPageDesktop` (simulated windows).
 * The wire types are generated from `paddock-panels`.
 */
export interface PanelsIo {
  /** This window's label (`main`, `w-2`, `p-main-1`). */
  label(): Promise<string>;
  /** `panel_pop_out`: a window for the panel, or the one that shows it (focused). */
  popOut(request: PopOutRequest): Promise<PopOutInfo>;
  /** `panel_dock_back`: closes a pop-out; its panel docks back (a pop-out names no panel for itself). */
  dockBack(panel?: string): Promise<void>;
  /** `panels_list`: the pop-outs of this window's group. */
  list(): Promise<PopOutInfo[]>;
  /** `panel_focus`: brings a pop-out to the front. */
  focus(panel: string): Promise<boolean>;
  /** `panel_keep_on_top`: in a pop-out, stay above the host window (or not). */
  keepOnTop(on: boolean): Promise<void>;
  /** `ui_sync_publish`: how many windows got the message. */
  publish(sync: SyncPublish): Promise<number>;
  /** `paddock://panels`, sent to a host window. */
  onPanels(handler: (event: PanelsEvent) => void): Promise<Unlisten>;
  /** `paddock://ui-sync`, sent to the other windows of a host. The payload is untrusted. */
  onSync(handler: (message: unknown) => void): Promise<Unlisten>;
  /** `pointer_button_down`: is the primary button held anywhere on screen? null when the platform cannot tell. */
  pointerDown(): Promise<boolean | null>;
  readonly ghost: GhostIo;
}

/** What `tauriPanelsIo` needs of `@tauri-apps/api` (the app passes it in, so the package does not depend on Tauri). */
export interface PanelsBridge {
  /** `invoke` from `@tauri-apps/api/core` (through the app's command routing). */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** `getCurrentWebviewWindow().listen` (events sent to this window only). */
  listen(event: string, handler: (event: { payload: unknown }) => void): Promise<Unlisten>;
  /** `getCurrentWebviewWindow().label`. */
  label(): Promise<string> | string;
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === 'string' && v.length > 0;

/** A `paddock://panels` payload at the edge: null when it is not one of the layer's events. */
export function parsePanelsEvent(payload: unknown): PanelsEvent | null {
  if (!isRecord(payload)) return null;
  switch (payload['kind']) {
    case 'returned':
      return text(payload['panel']) && text(payload['label']) ? { kind: 'returned', panel: payload['panel'], label: payload['label'] } : null;
    case 'changed':
      return { kind: 'changed' };
    case 'moved':
      return text(payload['panel']) && text(payload['label']) && Number.isFinite(payload['x']) && Number.isFinite(payload['y'])
        ? { kind: 'moved', panel: payload['panel'], label: payload['label'], x: payload['x'] as number, y: payload['y'] as number }
        : null;
    case 'redock':
      return text(payload['panel']) && text(payload['label']) && (payload['look'] === 'opaque' || payload['look'] === 'translucent' || payload['look'] === 'outline')
        ? { kind: 'redock', panel: payload['panel'], label: payload['label'], look: payload['look'] }
        : null;
    default:
      return null;
  }
}

/** The panel layer over Tauri: the commands of `paddock_panels::COMMANDS` and its two events. */
export function tauriPanelsIo(tauri: PanelsBridge): PanelsIo {
  const call = <T>(command: PanelCommand, args?: Record<string, unknown>): Promise<T> => tauri.invoke<T>(command, args);
  return {
    label: async () => tauri.label(),
    popOut: (request) => call<PopOutInfo>('panel_pop_out', { request }),
    dockBack: (panel) => call<void>('panel_dock_back', { panel: panel ?? null }),
    list: () => call<PopOutInfo[]>('panels_list'),
    focus: (panel) => call<boolean>('panel_focus', { panel }),
    keepOnTop: (on) => call<void>('panel_keep_on_top', { on }),
    publish: (sync) => call<number>('ui_sync_publish', { sync }),
    onPanels: (handler) =>
      tauri.listen(PANEL_EVENTS.panels, (e) => {
        const parsed = parsePanelsEvent(e.payload);
        if (parsed) handler(parsed);
      }),
    onSync: (handler) => tauri.listen(PANEL_EVENTS.uiSync, (e) => handler(e.payload)),
    pointerDown: () => call<boolean | null>('pointer_button_down'),
    ghost: {
      available: true,
      start: (request) => call<boolean>('drag_ghost_start', { request }),
      show: (visible) => call<void>('drag_ghost_show', { visible }),
      end: () => call<PopOutBounds | null>('drag_ghost_end'),
    },
  };
}

/** The panel layer's window system. Defaults to simulated windows (the browser build); a desktop app provides `tauriPanelsIo`. */
export const PANELS_IO = new InjectionToken<PanelsIo>('PADDOCK_PANELS_IO', { providedIn: 'root', factory: () => new InPageDesktop().window('main') });
