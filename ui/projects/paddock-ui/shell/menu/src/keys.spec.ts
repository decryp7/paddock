import { ariaKeyShortcuts, currentOs, currentPlatform, eventKey, eventPressed, formatKey, isTextEntry, matchesKey, recordKey, textFieldKeeps, type KeyInput } from './keys';

const key = (k: string, mods: Partial<KeyInput> = {}): KeyInput => ({ key: k, code: '', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

describe('key events', () => {
  it('knows the platform from the browser', () => {
    expect(currentPlatform({ platform: 'MacIntel' })).toBe('mac');
    expect(currentPlatform({ platform: '', userAgent: 'Mozilla/5.0 (iPad; CPU OS 18_0)' })).toBe('mac');
    expect(currentPlatform({ platform: 'Win32' })).toBe('other');
    expect(currentPlatform(undefined)).toBe('other');
  });

  it("takes the shell's data-platform before the browser's (the window's real OS, or the mock's)", () => {
    const doc = (platform?: string) => ({ documentElement: { dataset: platform ? { platform } : {} } as unknown as HTMLElement });
    expect(currentPlatform({ platform: 'MacIntel' }, doc('windows'))).toBe('other');
    expect(currentPlatform({ platform: 'Win32' }, doc('macos'))).toBe('mac');
    expect(currentOs({ platform: 'MacIntel' }, doc('windows'))).toBe('windows');
    expect(currentOs({ platform: 'MacIntel' }, doc('linux'))).toBe('other');
    expect(currentOs({ platform: 'Win32' }, doc())).toBe('windows');
    expect(currentOs({ platform: 'Win32' }, doc('beos'))).toBe('windows');
  });

  it('spells event keys as bindings do', () => {
    expect(eventKey(key('k'))).toBe('K');
    expect(eventKey(key('ArrowLeft'))).toBe('Left');
    expect(eventKey(key(' '))).toBe('Space');
    expect(eventKey(key('+'))).toBe('Plus');
    expect(eventKey(key('-'))).toBe('Minus');
    expect(eventKey(key('F5'))).toBe('F5');
    expect(eventKey(key('Esc'))).toBe('Escape');
    // Option turns letters into symbols and Shift turns digits into symbols: the physical key counts.
    expect(eventKey(key('˚', { altKey: true, code: 'KeyK' }))).toBe('K');
    expect(eventKey(key('!', { shiftKey: true, code: 'Digit1' }))).toBe('1');
    expect(eventKey(key('Unidentified'))).toBeNull();
    expect(eventKey({ key: undefined as unknown as string, code: undefined as unknown as string, altKey: false })).toBeNull();
  });

  it('matches bindings with Mod resolved per platform', () => {
    const cmdK = key('k', { metaKey: true });
    const ctrlK = key('k', { ctrlKey: true });
    expect(matchesKey('Mod+K', cmdK, 'mac')).toBe(true);
    expect(matchesKey('Mod+K', ctrlK, 'mac')).toBe(false);
    expect(matchesKey('Mod+K', ctrlK, 'other')).toBe(true);
    expect(matchesKey('Ctrl+K', ctrlK, 'mac')).toBe(true);
    expect(matchesKey('Mod+Shift+H', key('H', { ctrlKey: true, shiftKey: true }), 'other')).toBe(true);
    expect(matchesKey('Mod+Shift+H', key('H', { ctrlKey: true }), 'other')).toBe(false);
    expect(matchesKey('Alt+Left', key('ArrowLeft', { altKey: true }), 'other')).toBe(true);
    expect(matchesKey('Hyper+K', ctrlK, 'other')).toBe(false);
    expect(eventPressed(key('Shift'))).toBeNull();
    expect(eventPressed(key('p', { ctrlKey: true, altKey: true, shiftKey: true, metaKey: true }))).toBe('Ctrl+Alt+Shift+Meta+P');
  });

  it('formats hints the platform way', () => {
    expect(formatKey('Mod+Shift+P', 'mac')).toBe('⇧⌘P');
    expect(formatKey('Ctrl+Alt+Left', 'mac')).toBe('⌃⌥←');
    expect(formatKey('Mod+Shift+P', 'other')).toBe('Ctrl+Shift+P');
    expect(formatKey('Mod+Ctrl+P', 'other')).toBe('Ctrl+P');
    expect(formatKey('Meta+Plus', 'other')).toBe('Win++');
    expect(formatKey('Mod+Minus', 'mac')).toBe('⌘-');
    expect(formatKey('F5', 'other')).toBe('F5');
    expect(formatKey('Nope+X', 'other')).toBe('');
    expect(ariaKeyShortcuts('Mod+Shift+P', 'other')).toBe('Control+Shift+P');
    expect(ariaKeyShortcuts('Mod+Left', 'mac')).toBe('Meta+ArrowLeft');
    expect(ariaKeyShortcuts('Alt+Plus', 'mac')).toBe('Alt++');
    expect(ariaKeyShortcuts('', 'mac')).toBeNull();
  });
});

describe('recordKey', () => {
  it('records a chord with the platform modifier as Mod', () => {
    expect(recordKey(key('k', { metaKey: true }), 'mac')).toEqual({ kind: 'binding', binding: 'Mod+K' });
    expect(recordKey(key('k', { ctrlKey: true }), 'mac')).toEqual({ kind: 'binding', binding: 'Ctrl+K' });
    expect(recordKey(key('k', { ctrlKey: true, shiftKey: true }), 'other')).toEqual({ kind: 'binding', binding: 'Mod+Shift+K' });
    expect(recordKey(key('k', { metaKey: true }), 'other')).toEqual({ kind: 'binding', binding: 'Meta+K' });
    expect(recordKey(key('F6'), 'other')).toEqual({ kind: 'binding', binding: 'F6' });
    expect(recordKey(key('Dead', { altKey: true, code: 'KeyN' }), 'mac')).toEqual({ kind: 'binding', binding: 'Alt+N' });
    // macOS: Option composes a character (Option+G is "©", Option+1 is "¡"); the physical key names the binding.
    expect(recordKey(key('©', { metaKey: true, altKey: true, code: 'KeyG' }), 'mac')).toEqual({ kind: 'binding', binding: 'Mod+Alt+G' });
    expect(recordKey(key('¡', { metaKey: true, altKey: true, code: 'Digit1' }), 'mac')).toEqual({ kind: 'binding', binding: 'Mod+Alt+1' });
  });

  it('cancels on Escape, clears on Backspace or Delete, waits on modifiers', () => {
    expect(recordKey(key('Escape'), 'other')).toEqual({ kind: 'cancel' });
    expect(recordKey(key('Backspace'), 'other')).toEqual({ kind: 'clear' });
    expect(recordKey(key('Delete'), 'mac')).toEqual({ kind: 'clear' });
    expect(recordKey(key('Shift', { shiftKey: true }), 'other')).toEqual({ kind: 'wait' });
  });

  it('refuses keys that would fire while typing', () => {
    expect(recordKey(key('k'), 'other')).toEqual({ kind: 'invalid' });
    expect(recordKey(key('K', { shiftKey: true }), 'other')).toEqual({ kind: 'invalid' });
    expect(recordKey(key('§', { ctrlKey: true }), 'other')).toEqual({ kind: 'invalid' });
  });
});

describe('text fields', () => {
  it('knows where text is typed', () => {
    const input = document.createElement('input');
    const checkbox = Object.assign(document.createElement('input'), { type: 'checkbox' });
    const area = document.createElement('textarea');
    const editable = document.createElement('div');
    editable.contentEditable = 'true';
    Object.defineProperty(editable, 'isContentEditable', { value: true });
    expect([input, area, editable].map(isTextEntry)).toEqual([true, true, true]);
    expect(isTextEntry(checkbox)).toBe(false);
    expect(isTextEntry(document.createElement('button'))).toBe(false);
    expect(isTextEntry(null)).toBe(false);
  });

  it('keep typing and editing chords but not command chords', () => {
    expect(textFieldKeeps(key('a'), 'other')).toBe(true);
    expect(textFieldKeeps(key('A', { shiftKey: true }), 'other')).toBe(true);
    expect(textFieldKeeps(key('z', { ctrlKey: true }), 'other')).toBe(true);
    expect(textFieldKeeps(key('z', { metaKey: true, shiftKey: true }), 'mac')).toBe(true);
    expect(textFieldKeeps(key('ArrowLeft', { altKey: true }), 'mac')).toBe(true);
    expect(textFieldKeeps(key('h', { ctrlKey: true, shiftKey: true }), 'other')).toBe(false);
    expect(textFieldKeeps(key('k', { metaKey: true }), 'mac')).toBe(false);
    expect(textFieldKeeps(key('F5'), 'other')).toBe(false);
    expect(textFieldKeeps(key('k', { altKey: true }), 'other')).toBe(false);
  });
});
