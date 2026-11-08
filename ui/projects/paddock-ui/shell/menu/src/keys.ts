import { normalizeKey, parseKey, pressedKeys } from '@decryp7/paddock-ui/shell';

/**
 * Key events against bindings (`Mod+Shift+P`, see `parseKey` in the shell core): matching, the hint text a menu shows,
 * `aria-keyshortcuts`, and reading a key press while the user records a binding. Pure; the platform is a parameter.
 */

/** `mac`: `Mod` is Cmd; `other`: `Mod` is Ctrl (Windows, Linux). */
export type KeyPlatform = 'mac' | 'other';

/** The operating system of this browser, for key hints and menu wording: macOS and iOS, Windows, or any other. */
export type MenuOs = 'mac' | 'windows' | 'other';

/** The browser fields `currentOs` reads. */
type Nav = { readonly platform?: string; readonly userAgent?: string } | undefined;
/** The document field `currentOs` reads. */
type Doc = { readonly documentElement?: { readonly dataset?: DOMStringMap } } | undefined;

/**
 * The operating system of this window (the one place that reads the user agent): the shell's `data-platform` on `<html>` when it
 * is set (by Rust before the page boots, or by an app's browser mock for another OS), else the user agent.
 */
export function currentOs(nav: Nav = globalThis.navigator, doc: Doc = globalThis.document): MenuOs {
  const set = doc?.documentElement?.dataset?.['platform'];
  if (set === 'macos') return 'mac';
  if (set === 'windows') return 'windows';
  if (set === 'linux') return 'other';
  const p = `${nav?.platform ?? ''} ${nav?.userAgent ?? ''}`;
  if (/Mac|iPhone|iPad/.test(p)) return 'mac';
  return /Win/i.test(p) ? 'windows' : 'other';
}

/** The key platform of this window (Cmd on macOS and iOS), from `currentOs`. */
export function currentPlatform(nav: Nav = globalThis.navigator, doc: Doc = globalThis.document): KeyPlatform {
  return currentOs(nav, doc) === 'mac' ? 'mac' : 'other';
}

/** The part of a keyboard event the helpers read. */
export type KeyInput = Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>;

const EVENT_KEYS: Readonly<Record<string, string>> = {
  arrowleft: 'Left',
  arrowright: 'Right',
  arrowup: 'Up',
  arrowdown: 'Down',
  ' ': 'Space',
  esc: 'Escape',
  del: 'Delete',
  '+': 'Plus',
  '-': 'Minus',
};

/** The key of an event in binding spelling (`ArrowLeft` is `Left`, `k` is `K`), or null for a key a binding cannot name. */
export function eventKey(e: Pick<KeyInput, 'key' | 'code' | 'altKey'>): string | null {
  const key = e.key ?? '';
  const lower = key.toLowerCase();
  const named = EVENT_KEYS[lower];
  // Letters come from the key itself; the physical key only helps where Alt/Option turns them into symbols.
  const letter = /^Key([A-Z])$/.exec(e.code ?? '');
  const digit = /^Digit([0-9])$/.exec(e.code ?? '');
  const fromCode = letter?.[1] ?? digit?.[1] ?? null;
  if (named) return named;
  // Alt/Option and Shift turn letters and digits into symbols (`⌥K` is `˚`, `⇧1` is `!`): the physical key counts.
  if (e.altKey && fromCode && !/^[a-z0-9]$/i.test(key)) return fromCode;
  return parseKey(key)?.key ?? fromCode;
}

/** What an event presses, in the form of `pressedKeys` (`Meta+Shift+P`), or null when the key cannot be bound. */
export function eventPressed(e: KeyInput): string | null {
  const key = eventKey(e);
  if (!key) return null;
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Meta'].filter((m): m is string => !!m);
  return [...mods, key].join('+');
}

/** Whether a key event presses `binding` on `platform` (`Mod+K` is Cmd+K on macOS, Ctrl+K elsewhere). */
export function matchesKey(binding: string, e: KeyInput, platform: KeyPlatform): boolean {
  const want = pressedKeys(binding, platform);
  return want !== null && want === eventPressed(e);
}

const ARROWS: Readonly<Record<string, string>> = { Left: '←', Right: '→', Up: '↑', Down: '↓' };

/** The hint of a binding in menus and tooltips: `⌘⇧P` on macOS, `Ctrl+Shift+P` elsewhere; `''` when malformed. */
export function formatKey(binding: string, platform: KeyPlatform): string {
  const p = parseKey(binding);
  if (!p) return '';
  const mac = { Mod: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧', Meta: '⌘' } as const;
  const other = { Mod: 'Ctrl', Ctrl: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Meta: 'Win' } as const;
  const key = ARROWS[p.key] ?? (p.key === 'Plus' ? '+' : p.key === 'Minus' ? '-' : p.key);
  if (platform === 'mac') {
    // macOS orders ⌃⌥⇧⌘.
    const order = ['Ctrl', 'Alt', 'Shift', 'Mod', 'Meta'] as const;
    const mods = order.filter((m) => p.modifiers.includes(m)).map((m) => mac[m]);
    return [...new Set(mods)].join('') + key;
  }
  return [...new Set(p.modifiers.map((m) => other[m])), key].join('+');
}

/** The `aria-keyshortcuts` value of a binding (`Control+Shift+P`, `Meta+Shift+P` on macOS), or null when malformed. */
export function ariaKeyShortcuts(binding: string, platform: KeyPlatform): string | null {
  const pressed = pressedKeys(binding, platform);
  if (!pressed) return null;
  const names: Record<string, string> = { Ctrl: 'Control', Left: 'ArrowLeft', Right: 'ArrowRight', Up: 'ArrowUp', Down: 'ArrowDown', Plus: '+', Minus: '-' };
  return pressed
    .split('+')
    .map((part) => names[part] ?? part)
    .join('+');
}

/** Keys that never make a binding on their own. */
const MODIFIER_KEYS = new Set(['control', 'shift', 'alt', 'meta', 'altgraph', 'os', 'capslock', 'numlock', 'scrolllock', 'fn', 'process', 'unidentified', 'hyper', 'super']);

/** The result of reading a key press while the user records a binding. */
export type Recorded =
  | { readonly kind: 'binding'; readonly binding: string }
  | { readonly kind: 'cancel' }
  | { readonly kind: 'clear' }
  /** Only modifiers so far: keep waiting. */
  | { readonly kind: 'wait' }
  /** A key that cannot be bound alone (a letter without Ctrl/Alt/Cmd, Tab...): tell the user. */
  | { readonly kind: 'invalid' };

/**
 * Reads a key press as a binding, with the platform's primary modifier as `Mod`. Escape cancels and Backspace or
 * Delete (alone) clear, as in other editors; keys other than F1-F24 need Ctrl, Alt or Cmd, since a bare or shifted key
 * would fire while the user types.
 */
export function recordKey(e: KeyInput, platform: KeyPlatform): Recorded {
  const name = (e.key ?? '').toLowerCase();
  if (MODIFIER_KEYS.has(name)) return { kind: 'wait' };
  const bare = !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
  if (bare && name === 'escape') return { kind: 'cancel' };
  if (bare && (name === 'backspace' || name === 'delete')) return { kind: 'clear' };
  const key = eventKey(e);
  if (!key) return { kind: 'invalid' };
  const mac = platform === 'mac';
  const primary = mac ? e.metaKey : e.ctrlKey;
  const secondary = mac ? e.ctrlKey : e.metaKey;
  if (!/^F\d+$/.test(key) && !primary && !secondary && !e.altKey) return { kind: 'invalid' };
  const mods = [primary && 'Mod', secondary && (mac ? 'Ctrl' : 'Meta'), e.altKey && 'Alt', e.shiftKey && 'Shift'].filter((m): m is string => !!m);
  const binding = normalizeKey([...mods, key].join('+'));
  return binding ? { kind: 'binding', binding } : { kind: 'invalid' };
}

const TEXT_INPUTS = new Set(['', 'text', 'search', 'email', 'url', 'tel', 'password', 'number']);

/** Whether the element is a place where text is typed (so the browser's own editing keys apply there). */
export function isTextEntry(el: Element | null): boolean {
  if (!el || !(el instanceof HTMLElement)) return false;
  if (el.tagName === 'TEXTAREA' || el.isContentEditable) return true;
  return el.tagName === 'INPUT' && TEXT_INPUTS.has((el.getAttribute('type') ?? '').toLowerCase());
}

/**
 * Whether a text field keeps a key press for itself rather than letting it run a command: plain and shifted keys
 * (typing), the editing chords (Mod+A, C, V, X, Z, Y, Shift+Mod+Z) and Alt+arrows or Alt+Backspace (word moves).
 */
export function textFieldKeeps(e: KeyInput, platform: KeyPlatform): boolean {
  const key = eventKey(e);
  const primary = platform === 'mac' ? e.metaKey : e.ctrlKey;
  const secondary = platform === 'mac' ? e.ctrlKey : e.metaKey;
  if (key && /^F\d+$/.test(key)) return false;
  if (!primary && !secondary && !e.altKey) return true;
  if (primary && !e.altKey && !secondary && key && ['A', 'C', 'V', 'X', 'Z', 'Y', 'Left', 'Right', 'Up', 'Down', 'Backspace', 'Delete', 'Home', 'End'].includes(key)) return true;
  if (e.altKey && !primary && !secondary && key && ['Left', 'Right', 'Up', 'Down', 'Backspace', 'Delete'].includes(key)) return true;
  return false;
}
