/**
 * Key bindings as written in contributions: `Mod+Shift+P`, `Alt+Left`, `F5`. `Mod` is Cmd on macOS and Ctrl elsewhere.
 * Pure parsing and normalisation; matching key events and user overrides are the menu package's (plan S13).
 */

/** The modifiers a binding may name, in canonical order. */
export const KEY_MODIFIERS = ['Mod', 'Ctrl', 'Alt', 'Shift', 'Meta'] as const;
export type KeyModifier = (typeof KEY_MODIFIERS)[number];

const NAMED_KEYS = [
  'Enter', 'Escape', 'Tab', 'Space', 'Backspace', 'Delete', 'Insert', 'Home', 'End', 'PageUp', 'PageDown',
  'Left', 'Right', 'Up', 'Down', 'Plus', 'Minus',
] as const;
const FUNCTION_KEY = /^F([1-9]|1[0-9]|2[0-4])$/;
const PRINTABLE = /^[A-Za-z0-9`\-=[\];',./]$/;

/** A parsed binding: its modifiers (canonical order, no duplicates) and its key (upper case for letters). */
export interface ParsedKey {
  readonly modifiers: readonly KeyModifier[];
  readonly key: string;
}

/** Parses a binding; `null` when it is malformed (unknown modifier or key, repeated modifier, no key). */
export function parseKey(binding: string): ParsedKey | null {
  if (typeof binding !== 'string' || binding.length === 0 || binding.length > 64) return null;
  // `Mod++` would be ambiguous: the plus key is written `Plus`.
  const parts = binding.split('+').map((p) => p.trim());
  const keyPart = parts.pop();
  if (!keyPart) return null;
  const mods = new Set<KeyModifier>();
  for (const p of parts) {
    const m = KEY_MODIFIERS.find((k) => k.toLowerCase() === p.toLowerCase());
    if (!m || mods.has(m)) return null;
    mods.add(m);
  }
  const named = NAMED_KEYS.find((k) => k.toLowerCase() === keyPart.toLowerCase());
  const key = named ?? (FUNCTION_KEY.test(keyPart.toUpperCase()) ? keyPart.toUpperCase() : PRINTABLE.test(keyPart) ? keyPart.toUpperCase() : null);
  if (!key) return null;
  return { modifiers: KEY_MODIFIERS.filter((m) => mods.has(m)), key };
}

/** The canonical form of a binding (`Mod+Shift+P`), or `null` when malformed. */
export function normalizeKey(binding: string): string | null {
  const p = parseKey(binding);
  return p ? [...p.modifiers, p.key].join('+') : null;
}

/**
 * What the binding presses on a platform: `Mod` resolved to Meta (macOS) or Ctrl (elsewhere). Two bindings conflict when
 * they press the same keys on either platform (`Mod+R` and `Ctrl+R` collide on Windows and Linux).
 */
export function pressedKeys(binding: string, platform: 'mac' | 'other'): string | null {
  const p = parseKey(binding);
  if (!p) return null;
  const mods = new Set<KeyModifier>(p.modifiers.map((m) => (m === 'Mod' ? (platform === 'mac' ? 'Meta' : 'Ctrl') : m)));
  return [...KEY_MODIFIERS.filter((m) => mods.has(m)), p.key].join('+');
}

/** The Tauri/native menu accelerator of a binding (`CmdOrCtrl+Shift+P`), or `null` when malformed. */
export function toAccelerator(binding: string): string | null {
  const p = parseKey(binding);
  if (!p) return null;
  const names: Record<KeyModifier, string> = { Mod: 'CmdOrCtrl', Ctrl: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Meta: 'Super' };
  return [...p.modifiers.map((m) => names[m]), p.key].join('+');
}
