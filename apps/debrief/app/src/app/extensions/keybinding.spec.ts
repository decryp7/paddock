import { ariaKeyshortcuts, formatKeybinding, isMac, matchesKeybinding } from './keybinding';

const SAFARI = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15';
const CHROME_WIN = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36';
const key = (init: Partial<KeyboardEvent> & { key: string }) => ({ ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...init }) as KeyboardEvent;

describe('isMac', () => {
  it('follows the platform and the user agent together', () => {
    expect(isMac({ platform: 'MacIntel', userAgent: '' })).toBe(true);
    expect(isMac({ platform: 'iPad', userAgent: '' })).toBe(true);
    expect(isMac({ platform: 'Win32', userAgent: CHROME_WIN })).toBe(false);
    expect(isMac({ platform: 'Linux x86_64', userAgent: 'Mozilla/5.0 (X11; Linux x86_64)' })).toBe(false);
    expect(isMac({})).toBe(false);
    expect(isMac(undefined)).toBe(isMac(typeof navigator === 'undefined' ? undefined : navigator));
  });

  it('is true for a Linux platform with a Mac user agent (WebKit emulating Safari on a Linux runner), so Mod is Cmd there', () => {
    const nav = { platform: 'Linux x86_64', userAgent: SAFARI };
    expect(isMac(nav)).toBe(true);
    expect(matchesKeybinding('Mod+K', key({ key: 'k', metaKey: true }), isMac(nav))).toBe(true);
    expect(matchesKeybinding('Mod+K', key({ key: 'k', ctrlKey: true }), isMac(nav))).toBe(false);
    expect(formatKeybinding('Mod+Shift+P', isMac(nav))).toBe('⌘⇧P');
    expect(ariaKeyshortcuts('Mod+Shift+P', isMac(nav))).toBe('Meta+Shift+P');
  });
});
