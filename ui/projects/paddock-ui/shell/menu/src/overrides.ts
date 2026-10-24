import { normalizeKey, pressedKeys, type KeybindingDef } from '@decryp7/paddock-ui/shell';
import type { KeyPlatform } from './keys';

/**
 * User key bindings over the ones contributions declare: an override maps a command id to a binding, or to `null` for
 * "no key". It replaces every binding the command declares (in the scope of its first one, `window` when it declares
 * none). Pure: no framework, no storage (see `Keymap`).
 */

/** Overrides by command id; `null` = the user removed the key. */
export type KeybindingOverrides = Readonly<Record<string, string | null>>;

/** Most overrides kept (far more than any app has commands; bounds corrupt or hostile storage). */
export const MAX_OVERRIDES = 1000;
const MAX_ID_LENGTH = 200;
const COMMAND_ID = /^[A-Za-z0-9._:/-]+$/;

/** A binding in effect: declared by a contribution or set by the user. `key` is canonical (`Mod+Shift+P`). */
export interface EffectiveBinding {
  readonly command: string;
  readonly key: string;
  readonly scope: string;
  readonly source: 'default' | 'user';
}

/** The bindings in effect: declared ones (malformed ones dropped, which the lint reports), overridden ones replaced. */
export function effectiveBindings(declared: readonly KeybindingDef[], overrides: KeybindingOverrides): EffectiveBinding[] {
  const out: EffectiveBinding[] = [];
  const scopeOf = new Map<string, string>();
  const seen = new Set<string>();
  for (const k of declared) {
    if (!scopeOf.has(k.command)) scopeOf.set(k.command, k.scope);
    if (Object.hasOwn(overrides, k.command)) continue;
    const key = normalizeKey(k.key);
    const slot = `${k.scope}|${key}|${k.command}`;
    if (!key || seen.has(slot)) continue;
    seen.add(slot);
    out.push({ command: k.command, key, scope: k.scope, source: 'default' });
  }
  for (const command of Object.keys(overrides).sort()) {
    const key = overrides[command];
    if (typeof key !== 'string') continue;
    const canon = normalizeKey(key);
    if (canon) out.push({ command, key: canon, scope: scopeOf.get(command) ?? 'window', source: 'user' });
  }
  return out;
}

/** The keys a command declares itself, canonical, ignoring the user's overrides. */
export function declaredKeys(declared: readonly KeybindingDef[], command: string): string[] {
  return declared.filter((k) => k.command === command).flatMap((k) => normalizeKey(k.key) ?? []);
}

/**
 * `overrides` with `command` bound to `key` (`null` = no key). An override that gives back what the command declares
 * (its one key, or no key when it declares none) is dropped, so setting the default by hand leaves no trace.
 */
export function withOverride(overrides: KeybindingOverrides, command: string, key: string | null, declared: readonly string[]): KeybindingOverrides {
  const rest = withoutOverride(overrides, command);
  const canon = key === null ? null : normalizeKey(key);
  if (key !== null && canon === null) return rest;
  const isDefault = canon === null ? declared.length === 0 : declared.length === 1 && declared[0] === canon;
  return isDefault ? rest : { ...rest, [command]: canon };
}

/** `overrides` without `command`. */
export function withoutOverride(overrides: KeybindingOverrides, command: string): KeybindingOverrides {
  const rest: Record<string, string | null> = { ...overrides };
  delete rest[command];
  return rest;
}

/** Narrows stored (untrusted) overrides: invalid ids and bindings are dropped, the count is capped. Never throws. */
export function sanitizeOverrides(raw: unknown): KeybindingOverrides {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return {};
  const out: Record<string, string | null> = {};
  let n = 0;
  for (const [id, v] of Object.entries(raw)) {
    if (n >= MAX_OVERRIDES) break;
    if (!id || id.length > MAX_ID_LENGTH || !COMMAND_ID.test(id)) continue;
    if (v === null) out[id] = null;
    else if (typeof v === 'string') {
      const key = normalizeKey(v);
      if (!key) continue;
      out[id] = key;
    } else continue;
    n++;
  }
  return out;
}

/** Two bindings of different commands that press the same keys in one scope. */
export interface KeyConflict {
  readonly scope: string;
  /** What both press (`Ctrl+R`). */
  readonly pressed: string;
  /** Where they collide: `mac`, `other` (Windows and Linux), or both. */
  readonly platforms: readonly KeyPlatform[];
  readonly first: EffectiveBinding;
  readonly second: EffectiveBinding;
}

/** Every conflict among the bindings (per scope, on either platform), each pair once. */
export function conflictsOf(bindings: readonly EffectiveBinding[]): KeyConflict[] {
  const pairs = new Map<string, { scope: string; pressed: string; platforms: KeyPlatform[]; first: EffectiveBinding; second: EffectiveBinding }>();
  for (const platform of ['mac', 'other'] as const) {
    const seen = new Map<string, EffectiveBinding>();
    for (const b of bindings) {
      const pressed = pressedKeys(b.key, platform);
      if (!pressed) continue;
      const slot = `${b.scope}|${pressed}`;
      const first = seen.get(slot);
      if (!first) {
        seen.set(slot, b);
        continue;
      }
      if (first.command === b.command) continue;
      const id = `${b.scope}|${[first.command, b.command].sort().join('|')}`;
      const known = pairs.get(id);
      if (known) known.platforms.push(platform);
      else pairs.set(id, { scope: b.scope, pressed, platforms: [platform], first, second: b });
    }
  }
  return [...pairs.values()];
}

/** A sentence for a conflict, for a settings page or a log line. */
export function describeConflict(c: KeyConflict): string {
  const where = c.platforms.length === 2 ? '' : c.platforms[0] === 'mac' ? ' on macOS' : ' on Windows and Linux';
  return `${c.first.key} (${c.first.command}) and ${c.second.key} (${c.second.command}) both press ${c.pressed} in scope ${c.scope}${where}`;
}

/** A key the app handles outside the command registry (a focus shortcut, region keys), which bindings must not take. */
export interface ReservedKey {
  readonly key: string;
  readonly description: string;
  /** Defaults to `window`. */
  readonly scope?: string;
}

/** What already presses `key` in `scope` on either platform: other commands' bindings and reserved keys. */
export function occupantsOf(
  key: string,
  scope: string,
  command: string,
  bindings: readonly EffectiveBinding[],
  reserved: readonly ReservedKey[] = [],
): { readonly commands: readonly EffectiveBinding[]; readonly reserved: readonly ReservedKey[] } {
  const same = (a: string, b: string) => (['mac', 'other'] as const).some((p) => pressedKeys(a, p) !== null && pressedKeys(a, p) === pressedKeys(b, p));
  return {
    commands: bindings.filter((b) => b.command !== command && b.scope === scope && same(b.key, key)),
    reserved: reserved.filter((r) => (r.scope ?? 'window') === scope && same(r.key, key)),
  };
}
