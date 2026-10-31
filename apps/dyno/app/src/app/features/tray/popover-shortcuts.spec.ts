import { POPOVER_BINDINGS, shortcutLabel } from './popover-shortcuts';

describe('shortcutLabel', () => {
  it('shows no shortcut when the popover window binds none, which is how it ships', () => {
    expect(POPOVER_BINDINGS).toEqual({ settings: false, quit: false });
    expect(shortcutLabel('macos', 'settings')).toBeNull();
    expect(shortcutLabel('windows', 'quit')).toBeNull();
  });
  it('reads Command on macOS and Ctrl on Windows and Linux, only for a bound key', () => {
    const bound = { settings: true, quit: false };
    expect(shortcutLabel('macos', 'settings', bound)).toBe('⌘,');
    expect(shortcutLabel('windows', 'settings', bound)).toBe('Ctrl+,');
    expect(shortcutLabel('linux', 'settings', bound)).toBe('Ctrl+,');
    expect(shortcutLabel('macos', 'quit', bound)).toBeNull();
    expect(shortcutLabel('linux', 'quit', { settings: false, quit: true })).toBe('Ctrl+Q');
  });
});
