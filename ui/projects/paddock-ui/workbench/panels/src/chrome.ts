import { InjectionToken } from '@angular/core';
import type { Unlisten } from './io';

/** How a pop-out window is framed: native traffic lights over the app's title bar (`overlay`, macOS), no native frame (`frameless`: the bar draws minimise, maximise and close), or the browser's own popup frame. */
export type ChromeKind = 'overlay' | 'frameless' | 'browser';
export type ResizeDirection = 'East' | 'North' | 'NorthEast' | 'NorthWest' | 'South' | 'SouthEast' | 'SouthWest' | 'West';

/** What a pop-out's title bar asks of its OS window (one seam, so the bar needs no Tauri). */
export interface WindowChromeIo {
  readonly kind: ChromeKind;
  /** Can a window be kept above its owner here? (Not in a browser.) */
  readonly supportsKeepOnTop: boolean;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  /** Closes this window; for a pop-out that docks its panel back. */
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  /** Called after every resize (the maximised state may have changed). */
  onResized(handler: () => void): Promise<Unlisten>;
  /** Lets the OS move the window with the pointer; false when the platform cannot. */
  startDragging(): Promise<boolean>;
  /** Frameless windows: resize from an edge. */
  startResize(direction: ResizeDirection): Promise<void>;
}

/** What `tauriWindowChrome` needs of `getCurrentWindow()` from `@tauri-apps/api/window`. */
export interface TauriWindowBridge {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  onResized(handler: () => void): Promise<Unlisten>;
  startDragging(): Promise<void>;
  startResizeDragging(direction: ResizeDirection): Promise<void>;
}

/** The window chrome over Tauri; `mac` picks the overlay title bar (native traffic lights), else the app draws the controls. */
export function tauriWindowChrome(win: TauriWindowBridge, mac: boolean): WindowChromeIo {
  return {
    kind: mac ? 'overlay' : 'frameless',
    supportsKeepOnTop: true,
    minimize: () => win.minimize(),
    toggleMaximize: () => win.toggleMaximize(),
    close: () => win.close(),
    isMaximized: () => win.isMaximized(),
    onResized: (h) => win.onResized(h),
    startDragging: async () => {
      await win.startDragging();
      return true;
    },
    startResize: (d) => win.startResizeDragging(d),
  };
}

/** A browser popup: the browser frames it, nothing is offered but closing. */
export function browserWindowChrome(win: Pick<Window, 'close' | 'addEventListener' | 'removeEventListener'> = window): WindowChromeIo {
  return {
    kind: 'browser',
    supportsKeepOnTop: false,
    minimize: async () => undefined,
    toggleMaximize: async () => undefined,
    close: async () => win.close(),
    isMaximized: async () => false,
    onResized: async (h) => {
      win.addEventListener('resize', h);
      return () => win.removeEventListener('resize', h);
    },
    startDragging: async () => false,
    startResize: async () => undefined,
  };
}

/** The window's chrome. Defaults to the browser's; a desktop app provides `tauriWindowChrome`. */
export const WINDOW_CHROME = new InjectionToken<WindowChromeIo>('PADDOCK_WINDOW_CHROME', { providedIn: 'root', factory: () => browserWindowChrome() });
