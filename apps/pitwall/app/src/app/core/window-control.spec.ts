import { TestBed } from '@angular/core/testing';
import {
  BrowserWindowControl,
  NAVIGATE_EVENT,
  PENDING_ROUTE_KEY,
  PENDING_ROUTE_TTL_MS,
  PendingRoute,
  storageListener,
  TauriWindowControl,
  WINDOW_CONTROL,
  type TauriWindowPort,
} from './window-control';

/** An in-memory Storage shared by the "windows" of one test, as the app's webviews share their origin's storage. */
function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (k) => map.get(k) ?? null,
    key: (i) => [...map.keys()][i] ?? null,
    removeItem: (k) => void map.delete(k),
    setItem: (k, v) => void map.set(k, String(v)),
  };
}

function fakePort(opts: { focusFails?: boolean } = {}) {
  const calls: string[] = [];
  const listeners = new Map<string, () => void>();
  let storageHandler: (() => void) | null = null;
  let unlistened = 0;
  const port: TauriWindowPort = {
    invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
      calls.push(`invoke ${command}${args ? ' ' + JSON.stringify(args) : ''}`);
      if (opts.focusFails && command.endsWith('window_focus')) throw new Error('refused');
      return undefined;
    }),
    emitTo: vi.fn(async (label: string, event: string) => {
      calls.push(`emitTo ${label} ${event}`);
      listeners.get(event)?.();
    }),
    listen: vi.fn(async (event: string, handler: () => void) => {
      listeners.set(event, handler);
      return () => void (listeners.delete(event), unlistened++);
    }),
    onStorage: (h) => {
      storageHandler = h;
      return () => (storageHandler = null);
    },
  };
  return { port, calls, ring: (event: string) => listeners.get(event)?.(), storageChanged: () => storageHandler?.(), listening: () => listeners.size, unlistened: () => unlistened };
}

describe('PendingRoute', () => {
  it('gives a route once', () => {
    const storage = memoryStorage();
    const p = new PendingRoute(() => storage, () => 1000);
    p.put('/models');
    expect(p.take()).toBe('/models');
    expect(p.take()).toBeNull();
  });

  it('drops a stale route, a foreign or malformed one, and survives refusing storage', () => {
    const storage = memoryStorage();
    let now = 0;
    const p = new PendingRoute(() => storage, () => now);
    p.put('/models');
    now = PENDING_ROUTE_TTL_MS + 1;
    expect(p.take()).toBeNull();
    for (const route of ['//evil.example/x', 'https://x', '/a b', '/a\\b']) {
      p.put(route);
      expect(p.take(), route).toBeNull();
    }
    storage.setItem(PENDING_ROUTE_KEY, '{nope');
    expect(p.take()).toBeNull();
    storage.setItem(PENDING_ROUTE_KEY, JSON.stringify({ route: 7, at: now }));
    expect(p.take()).toBeNull();
    storage.setItem(PENDING_ROUTE_KEY, 'null');
    expect(p.take()).toBeNull();
    const refusing = new PendingRoute(() => {
      throw new Error('denied');
    });
    expect(() => refusing.put('/')).not.toThrow();
    expect(refusing.take()).toBeNull();
    expect(new PendingRoute(() => null).take()).toBeNull();
  });
});

describe('TauriWindowControl', () => {
  it('leaves the route, raises main through the shell, rings, then closes the popover, in that order', async () => {
    const storage = memoryStorage();
    const { port, calls } = fakePort();
    await new TauriWindowControl(port, new PendingRoute(() => storage)).openMain('/models');
    expect(calls).toEqual([
      'invoke plugin:paddock-shell|window_focus {"label":"main"}',
      `emitTo main ${NAVIGATE_EVENT}`,
      'invoke plugin:paddock-shell|tray_close',
    ]);
    expect(JSON.parse(storage.getItem(PENDING_ROUTE_KEY)!).route).toBe('/models');
  });

  it('fits the popover to its content through the shell', async () => {
    const { port, calls } = fakePort();
    await new TauriWindowControl(port, new PendingRoute(memoryStorage)).fitTray(431);
    expect(calls).toEqual(['invoke plugin:paddock-shell|tray_fit_height {"height":431}']);
    await expect(new BrowserWindowControl().fitTray()).resolves.toBeUndefined();
  });

  it('puts the popover away and quits through the app\'s own commands; a tab has neither', async () => {
    const { port, calls } = fakePort();
    const control = new TauriWindowControl(port, new PendingRoute(memoryStorage));
    await control.closeTray();
    await control.quit();
    expect(calls).toEqual(['invoke plugin:paddock-shell|tray_close', 'invoke quit_from_tray']);
    await expect(new BrowserWindowControl().closeTray()).resolves.toBeUndefined();
    await expect(new BrowserWindowControl().quit()).resolves.toBeUndefined();
  });

  it('opens on the dashboard by default', async () => {
    const storage = memoryStorage();
    const { port } = fakePort();
    await new TauriWindowControl(port, new PendingRoute(() => storage)).openMain();
    expect(new PendingRoute(() => storage).take()).toBe('/');
  });

  it('a main window that was closed takes the route when it is rebuilt, after the doorbell rang', async () => {
    const storage = memoryStorage();
    const tray = fakePort();
    await new TauriWindowControl(tray.port, new PendingRoute(() => storage)).openMain('/models');
    // The rebuilt window starts listening only now.
    const main = fakePort();
    const handler = vi.fn();
    const stop = new TauriWindowControl(main.port, new PendingRoute(() => storage)).onNavigate(handler);
    expect(handler).toHaveBeenCalledExactlyOnceWith('/models');
    await vi.waitFor(() => expect(main.listening()).toBe(1));
    main.ring(NAVIGATE_EVENT);
    expect(handler).toHaveBeenCalledTimes(1);
    stop();
    expect(main.listening()).toBe(0);
  });

  it('a main window that is open goes on the doorbell or on the storage change, once', async () => {
    const storage = memoryStorage();
    const main = fakePort();
    const handler = vi.fn();
    const control = new TauriWindowControl(main.port, new PendingRoute(() => storage));
    const stop = control.onNavigate(handler);
    await vi.waitFor(() => expect(main.listening()).toBe(1));
    expect(handler).not.toHaveBeenCalled();
    new PendingRoute(() => storage).put('/apps');
    main.storageChanged();
    main.ring(NAVIGATE_EVENT);
    expect(handler).toHaveBeenCalledExactlyOnceWith('/apps');
    stop();
    new PendingRoute(() => storage).put('/models');
    main.storageChanged();
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('stopping before the listener is ready unlistens as soon as it is', async () => {
    const main = fakePort();
    const stop = new TauriWindowControl(main.port, new PendingRoute(() => memoryStorage())).onNavigate(vi.fn());
    stop();
    await vi.waitFor(() => expect(main.unlistened()).toBe(1));
    expect(main.listening()).toBe(0);
  });

  it('when main cannot be shown, drops the route and still closes the popover', async () => {
    const storage = memoryStorage();
    const { port, calls } = fakePort({ focusFails: true });
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await new TauriWindowControl(port, new PendingRoute(() => storage)).openMain('/models');
    expect(calls.at(-1)).toBe('invoke plugin:paddock-shell|tray_close');
    expect(storage.getItem(PENDING_ROUTE_KEY)).toBeNull();
    expect(error).toHaveBeenCalled();
  });

  it('survives a doorbell or a listener that fails', async () => {
    const storage = memoryStorage();
    const { port } = fakePort();
    port.emitTo = vi.fn().mockRejectedValue(new Error('no window'));
    port.listen = vi.fn().mockRejectedValue(new Error('no events'));
    const control = new TauriWindowControl(port, new PendingRoute(() => storage));
    await expect(control.openMain('/apps')).resolves.toBeUndefined();
    const handler = vi.fn();
    control.onNavigate(handler);
    await vi.waitFor(() => expect(handler).toHaveBeenCalledWith('/apps'));
  });
});

describe('storageListener', () => {
  it('hears only a new pending route and stops', () => {
    const target = new EventTarget() as unknown as Window;
    const handler = vi.fn();
    const stop = storageListener(handler, target);
    target.dispatchEvent(new StorageEvent('storage', { key: 'other', newValue: 'x' }));
    target.dispatchEvent(new StorageEvent('storage', { key: PENDING_ROUTE_KEY, newValue: null }));
    target.dispatchEvent(new StorageEvent('storage', { key: PENDING_ROUTE_KEY, newValue: '{}' }));
    expect(handler).toHaveBeenCalledTimes(1);
    stop();
    target.dispatchEvent(new StorageEvent('storage', { key: PENDING_ROUTE_KEY, newValue: '{}' }));
    expect(handler).toHaveBeenCalledTimes(1);
  });
});

describe('BrowserWindowControl', () => {
  it('in the main window, navigates to the route as a hash', async () => {
    const nav = vi.fn();
    await new BrowserWindowControl(nav, vi.fn(), new PendingRoute(memoryStorage), () => false).openMain('/models');
    expect(nav).toHaveBeenCalledWith('#/models');
    await new BrowserWindowControl(nav, vi.fn(), new PendingRoute(memoryStorage), () => false).openMain();
    expect(nav).toHaveBeenCalledWith('#/');
  });

  it('in the tray, leaves the route and reloads the tab as the main window, which takes it on start', async () => {
    const storage = memoryStorage();
    const reboot = vi.fn();
    await new BrowserWindowControl(vi.fn(), reboot, new PendingRoute(() => storage), () => true).openMain('/models');
    expect(reboot).toHaveBeenCalledWith('#/');
    const handler = vi.fn();
    const stop = new BrowserWindowControl(vi.fn(), vi.fn(), new PendingRoute(() => storage), () => false).onNavigate(handler);
    expect(handler).toHaveBeenCalledWith('/models');
    expect(() => stop()).not.toThrow();
    const none = vi.fn();
    new BrowserWindowControl(vi.fn(), vi.fn(), new PendingRoute(() => storage), () => false).onNavigate(none);
    expect(none).not.toHaveBeenCalled();
  });

  it('sets location.hash by default outside the tray', async () => {
    const before = location.hash;
    await new BrowserWindowControl().openMain('/apps');
    expect(location.hash).toBe('#/apps');
    location.hash = before;
  });

  it('is what the injection token provides outside Tauri', () => {
    expect(TestBed.inject(WINDOW_CONTROL)).toBeInstanceOf(BrowserWindowControl);
  });
});
