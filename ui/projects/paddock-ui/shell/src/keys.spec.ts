import { normalizeKey, parseKey, pressedKeys, toAccelerator } from './keys';

describe('key bindings', () => {
  it('parses modifiers in canonical order and upper-cases letters', () => {
    expect(parseKey('shift+mod+p')).toEqual({ modifiers: ['Mod', 'Shift'], key: 'P' });
    expect(normalizeKey('Alt+left')).toBe('Alt+Left');
    expect(normalizeKey('f5')).toBe('F5');
    expect(normalizeKey('Mod+,')).toBe('Mod+,');
    expect(normalizeKey('Mod+Plus')).toBe('Mod+Plus');
  });

  it('refuses malformed bindings', () => {
    for (const bad of ['', 'Mod+', 'Hyper+P', 'Mod+Mod+P', 'Mod+PP', 'F25', 'Mod++', 'x'.repeat(65)]) expect(parseKey(bad), bad).toBeNull();
    expect(parseKey(42 as unknown as string)).toBeNull();
    expect(normalizeKey('Mod+')).toBeNull();
    expect(pressedKeys('Mod+', 'mac')).toBeNull();
    expect(toAccelerator('Mod+')).toBeNull();
  });

  it('resolves Mod per platform', () => {
    expect(pressedKeys('Mod+R', 'mac')).toBe('Meta+R');
    expect(pressedKeys('Mod+R', 'other')).toBe('Ctrl+R');
    expect(pressedKeys('Ctrl+Mod+R', 'other')).toBe('Ctrl+R');
  });

  it('writes native accelerators', () => {
    expect(toAccelerator('Mod+Shift+P')).toBe('CmdOrCtrl+Shift+P');
    expect(toAccelerator('Ctrl+Alt+Meta+Delete')).toBe('Ctrl+Alt+Super+Delete');
  });
});
