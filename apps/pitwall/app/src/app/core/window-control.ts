import { InjectionToken } from '@angular/core';
import { isTauri } from './backend';

/** Window actions of the tray popover (not a server call, so not part of the backend). */
export interface WindowControl {
  /** Shows and focuses the main window (rebuilt when it was closed), on a route, and closes the popover. */
  openMain(route?: string): Promise<void>;
  /** In the main window: routes the tray asks for, including one asked for before this window existed. Returns a stop. */
  onNavigate(handler: (route: string) => void): () => void;
  /** In the tray popover: makes the popover window as tall as its content (logical px). */
  fitTray(height: number): Promise<void>;
}

/** The storage key of the route the tray left for the main window. */
export const PENDING_ROUTE_KEY = 'pw.pending-route';
/** The doorbell the tray rings at the main window: "a route waits for you" (no payload; the route is in the slot). */
export const NAVIGATE_EVENT = 'pitwall://navigate';
/** A route older than this is stale (a window that never opened); it is dropped, never followed later. */
export const PENDING_ROUTE_TTL_MS = 60_000;

/**
 * The route waiting for the main window, in the app's own origin storage (every Pitwall webview shares it). The tray
 * puts it before it raises the main window; the main window takes it when it starts (a rebuilt window), when the tray
 * rings, or when the storage says the slot changed, whichever comes first. Taking empties the slot, so a route is
 * followed once. It plays the part of the shell's `window_take_pending`, whose queue only Rust can fill.
 */
export class PendingRoute {
  constructor(
    private readonly storage: () => Storage | null = () => globalThis.localStorage ?? null,
    private readonly now: () => number = Date.now,
  ) {}

  put(route: string): void {
    try {
      this.storage()?.setItem(PENDING_ROUTE_KEY, JSON.stringify({ route, at: this.now() }));
    } catch {
      // Storage refused (private mode, quota): the window still opens, on its last screen.
    }
  }

  /** The waiting route, once: null when there is none, it is stale, or it is not one of the app's own routes. */
  take(): string | null {
    try {
      const store = this.storage();
      const raw = store?.getItem(PENDING_ROUTE_KEY);
      if (!store || !raw) return null;
      store.removeItem(PENDING_ROUTE_KEY);
      const { route, at } = (JSON.parse(raw) ?? {}) as { route?: unknown; at?: unknown };
      if (typeof route !== 'string' || typeof at !== 'number') return null;
      if (!isAppRoute(route) || Math.abs(this.now() - at) > PENDING_ROUTE_TTL_MS) return null;
      return route;
    } catch {
      return null;
    }
  }
}

/** One of the app's own routes: `/` then a path, never `//` (another origin), a backslash or white space. */
function isAppRoute(route: string): boolean {
  return route.startsWith('/') && !route.startsWith('//') && route.length <= 512 && !/[\\\s]/.test(route);
}

/** What the Tauri window control needs from the webview: the IPC, an event to another window, and listening. */
export interface TauriWindowPort {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  emitTo(label: string, event: string): Promise<void>;
  listen(event: string, handler: () => void): Promise<() => void>;
  /** Calls `handler` when another webview leaves a pending route; returns the stop. */
  onStorage(handler: () => void): () => void;
}

/**
 * The tray popover inside Tauri: only commands of the shell's tray set (`paddock-shell:tray`) and Tauri's events
 * (`core:event:default`), no `core:window:*` permission. `window_focus { label: 'main' }` goes through the shell's
 * `reveal`, which rebuilds `main` when it was closed (destroyed); the route waits in {@link PendingRoute} for the window
 * to take it, and the doorbell only tells a window that already listens. Then `tray_close` closes the popover.
 * `tray_fit_height` makes the popover as tall as its content.
 */
export class TauriWindowControl implements WindowControl {
  constructor(
    private readonly port: TauriWindowPort,
    private readonly pending = new PendingRoute(),
  ) {}

  async openMain(route = '/'): Promise<void> {
    this.pending.put(route);
    try {
      await this.port.invoke('plugin:paddock-shell|window_focus', { label: 'main' });
      await this.port.emitTo('main', NAVIGATE_EVENT).catch(() => undefined);
    } catch (e) {
      // The main window could not be shown: the route must not surprise a later start.
      this.pending.take();
      console.error('Pitwall could not open its main window', e);
    } finally {
      await this.port.invoke('plugin:paddock-shell|tray_close').catch(() => undefined);
    }
  }

  async fitTray(height: number): Promise<void> {
    await this.port.invoke('plugin:paddock-shell|tray_fit_height', { height });
  }

  onNavigate(handler: (route: string) => void): () => void {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    const deliver = () => {
      if (stopped) return;
      const route = this.pending.take();
      if (route) handler(route);
    };
    void this.port.listen(NAVIGATE_EVENT, deliver).then(
      (u) => (stopped ? u() : (unlisten = u)),
      () => undefined,
    );
    const stopStorage = this.port.onStorage(deliver);
    // A window the tray rebuilt starts after the doorbell rang: its route waits in the slot.
    deliver();
    return () => {
      stopped = true;
      unlisten?.();
      stopStorage();
    };
  }
}

/** Listens for another webview's pending route (the `storage` event fires in every other same-origin document). */
export function storageListener(handler: () => void, target: Pick<Window, 'addEventListener' | 'removeEventListener'> = window): () => void {
  const onStorage = (e: Event) => {
    const s = e as StorageEvent;
    if (s.key === PENDING_ROUTE_KEY && s.newValue) handler();
  };
  target.addEventListener('storage', onStorage);
  return () => target.removeEventListener('storage', onStorage);
}

/* v8 ignore start -- needs the Tauri webview; TauriWindowControl is tested with a fake port. */
function tauriPort(): TauriWindowPort {
  return {
    invoke: async (command, args) => (await import('@tauri-apps/api/core')).invoke(command, args ?? {}),
    emitTo: async (label, event) => (await import('@tauri-apps/api/event')).emitTo(label, event),
    listen: async (event, handler) => (await import('@tauri-apps/api/event')).listen(event, () => handler()),
    onStorage: (handler) => storageListener(handler),
  };
}
/* v8 ignore stop */

/**
 * The browser build: one tab plays both windows. In the main window "open on a route" navigates the hash; in the tray
 * popover (`#/tray`, its own bootstrap) the route waits in the same {@link PendingRoute} slot and the tab reloads as the
 * main window, which takes it on start, as a window the shell rebuilt does in the app.
 */
export class BrowserWindowControl implements WindowControl {
  constructor(
    private readonly nav: (url: string) => void = (url) => (location.hash = url),
    private readonly reboot: (url: string) => void = (url) => {
      location.hash = url;
      location.reload();
    },
    private readonly pending = new PendingRoute(),
    private readonly inTray: () => boolean = () => location.hash.startsWith('#/tray'),
  ) {}

  async openMain(route = '/'): Promise<void> {
    if (!this.inTray()) {
      this.nav(`#${route}`);
      return;
    }
    this.pending.put(route);
    this.reboot('#/');
  }

  onNavigate(handler: (route: string) => void): () => void {
    const route = this.pending.take();
    if (route) handler(route);
    return () => undefined;
  }

  /** The tab is as large as the browser makes it. */
  async fitTray(): Promise<void> {
    return undefined;
  }
}

export const WINDOW_CONTROL = new InjectionToken<WindowControl>('WindowControl', {
  providedIn: 'root',
  factory: () => (isTauri() ? new TauriWindowControl(tauriPort()) : new BrowserWindowControl()),
});
