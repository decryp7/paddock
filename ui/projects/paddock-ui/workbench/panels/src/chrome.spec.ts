import { browserWindowChrome, tauriWindowChrome, type TauriWindowBridge } from './chrome';

describe('window chrome', () => {
  it('maps the Tauri window and picks the frame by platform', async () => {
    const calls: string[] = [];
    const win: TauriWindowBridge = {
      minimize: async () => void calls.push('minimize'),
      toggleMaximize: async () => void calls.push('toggle'),
      close: async () => void calls.push('close'),
      isMaximized: async () => true,
      onResized: async () => () => undefined,
      startDragging: async () => void calls.push('drag'),
      startResizeDragging: async (d) => void calls.push(`resize:${d}`),
    };
    expect(tauriWindowChrome(win, true).kind).toBe('overlay');
    const chrome = tauriWindowChrome(win, false);
    expect(chrome.kind).toBe('frameless');
    expect(chrome.supportsKeepOnTop).toBe(true);
    await chrome.minimize();
    await chrome.toggleMaximize();
    await chrome.close();
    expect(await chrome.isMaximized()).toBe(true);
    expect(await chrome.startDragging()).toBe(true);
    await chrome.startResize('NorthWest');
    await chrome.onResized(() => undefined);
    expect(calls).toEqual(['minimize', 'toggle', 'close', 'drag', 'resize:NorthWest']);
  });

  it('a browser popup offers nothing but closing', async () => {
    const listeners = new Set<() => void>();
    let closed = 0;
    const chrome = browserWindowChrome({ close: () => void closed++, addEventListener: (_t: string, h: EventListenerOrEventListenerObject) => void listeners.add(h as () => void), removeEventListener: (_t: string, h: EventListenerOrEventListenerObject) => void listeners.delete(h as () => void) });
    expect(chrome.kind).toBe('browser');
    expect(chrome.supportsKeepOnTop).toBe(false);
    await chrome.minimize();
    await chrome.toggleMaximize();
    await chrome.startResize('East');
    expect(await chrome.isMaximized()).toBe(false);
    expect(await chrome.startDragging()).toBe(false);
    const off = await chrome.onResized(() => undefined);
    expect(listeners.size).toBe(1);
    off();
    expect(listeners.size).toBe(0);
    await chrome.close();
    expect(closed).toBe(1);
  });
});
