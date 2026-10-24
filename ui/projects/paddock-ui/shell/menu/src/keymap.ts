import {
  computed,
  DestroyRef,
  Directive,
  EnvironmentProviders,
  inject,
  Injectable,
  InjectionToken,
  input,
  makeEnvironmentProviders,
  provideEnvironmentInitializer,
  Signal,
  signal,
} from '@angular/core';
import { CommandService, ContributionRegistry, normalizeKey, pressedKeys } from '@decryp7/paddock-ui/shell';
import { currentPlatform, eventPressed, isTextEntry, textFieldKeeps, type KeyInput, type KeyPlatform } from './keys';
import {
  conflictsOf,
  declaredKeys,
  effectiveBindings,
  occupantsOf,
  sanitizeOverrides,
  withOverride,
  withoutOverride,
  type EffectiveBinding,
  type KeybindingOverrides,
  type KeyConflict,
  type ReservedKey,
} from './overrides';

/** The platform whose modifiers `Mod` means (Cmd on macOS). Replaceable in tests. */
export const KEY_PLATFORM = new InjectionToken<KeyPlatform>('PADDOCK_KEY_PLATFORM', { providedIn: 'root', factory: () => currentPlatform() });

/** Keys the app handles outside the command registry; a user binding that takes one is reported as a conflict. */
export const RESERVED_KEYS = new InjectionToken<readonly ReservedKey[]>('PADDOCK_RESERVED_KEYS', { providedIn: 'root', factory: () => [] });

/** Where the user's bindings are kept (a UI preference of this machine). */
export interface KeybindingStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** The storage key of the overrides, and the version of what is stored there. */
export const KEYBINDINGS_KEY = 'paddock.keybindings';
export const KEYBINDINGS_VERSION = 1;

/** The overrides' storage: the browser's `localStorage`, or none where it is unavailable (the bindings then last a session). */
export const KEYBINDING_STORAGE = new InjectionToken<KeybindingStorage | null>('PADDOCK_KEYBINDING_STORAGE', {
  providedIn: 'root',
  factory: () => {
    try {
      return globalThis.localStorage ?? null;
    } catch {
      return null;
    }
  },
});

/** Reads stored overrides; anything missing, corrupt or of another version gives none. Never throws. */
export function loadOverrides(storage: Pick<KeybindingStorage, 'getItem'> | null): KeybindingOverrides {
  try {
    const parsed = JSON.parse(storage?.getItem(KEYBINDINGS_KEY) ?? 'null') as { version?: unknown; overrides?: unknown } | null;
    return parsed?.version === KEYBINDINGS_VERSION ? sanitizeOverrides(parsed.overrides) : {};
  } catch {
    return {};
  }
}

/** Stores overrides; a storage that is unavailable or full only costs the persistence. Returns whether it was stored. */
export function saveOverrides(overrides: KeybindingOverrides, storage: Pick<KeybindingStorage, 'setItem' | 'removeItem'> | null): boolean {
  if (!storage) return false;
  try {
    if (Object.keys(overrides).length === 0) storage.removeItem(KEYBINDINGS_KEY);
    else storage.setItem(KEYBINDINGS_KEY, JSON.stringify({ version: KEYBINDINGS_VERSION, overrides }));
    return true;
  } catch {
    return false;
  }
}

/** The answer of `Keymap.bind`. */
export type BindResult =
  | { readonly ok: true }
  /** The key is not a binding (`Hyper+Q`). */
  | { readonly ok: false; readonly reason: 'invalid' }
  /** Other commands or reserved keys press it already in that scope; `bind(..., { force: true })` takes it anyway. */
  | { readonly ok: false; readonly reason: 'conflict'; readonly commands: readonly EffectiveBinding[]; readonly reserved: readonly ReservedKey[] };

/** The attribute that names a key scope on an element: keys pressed inside it try that scope's bindings first. */
export const KEY_SCOPE_ATTRIBUTE = 'data-pd-key-scope';

/** The scopes active for a key press at `target`, innermost first, ending in `window`. */
export function activeScopes(target: EventTarget | null): string[] {
  const out: string[] = [];
  let el = target instanceof Element ? target : null;
  while (el) {
    const scope = el.getAttribute(KEY_SCOPE_ATTRIBUTE);
    if (scope && !out.includes(scope)) out.push(scope);
    el = el.parentElement;
  }
  if (!out.includes('window')) out.push('window');
  return out;
}

/** Marks an element as a key scope: `<section pdKeyScope="pw.models">`. */
@Directive({ selector: '[pdKeyScope]', host: { [`[attr.${KEY_SCOPE_ATTRIBUTE}]`]: 'pdKeyScope()' } })
export class PdKeyScope {
  readonly pdKeyScope = input.required<string>();
}

/**
 * The key bindings of the window: the contributions' bindings with the user's overrides (persisted, followed across
 * windows), their conflicts, and the dispatch of key presses to commands by scope. Menus, the palette and tooltips read
 * the effective keys from here.
 */
@Injectable({ providedIn: 'root' })
export class Keymap {
  private readonly registry = inject(ContributionRegistry);
  private readonly commands = inject(CommandService);
  private readonly storage = inject(KEYBINDING_STORAGE);
  private readonly reserved = inject(RESERVED_KEYS);
  readonly platform = inject(KEY_PLATFORM);
  private readonly state = signal<KeybindingOverrides>(loadOverrides(this.storage));
  private readonly paused = signal(false);
  private lastRun: { readonly id: string; readonly at: number } | null = null;

  /** The user's overrides by command id (`null`: no key). */
  readonly overrides: Signal<KeybindingOverrides> = this.state.asReadonly();
  /** Every binding in effect. */
  readonly bindings: Signal<readonly EffectiveBinding[]> = computed(() => effectiveBindings(this.registry.keybindings(), this.state()));
  /** Bindings of different commands that press the same keys in one scope (declared ones are the lint's; user ones are reported here). */
  readonly conflicts: Signal<readonly KeyConflict[]> = computed(() => conflictsOf(this.bindings()));
  /** While true (a binding is being recorded) key presses run no command. */
  readonly suspended: Signal<boolean> = this.paused.asReadonly();

  constructor() {
    if (typeof window === 'undefined') return;
    const onStorage = (e: StorageEvent) => {
      if (e.key === KEYBINDINGS_KEY || e.key === null) this.state.set(loadOverrides(this.storage));
    };
    window.addEventListener('storage', onStorage);
    inject(DestroyRef).onDestroy(() => window.removeEventListener('storage', onStorage));
  }

  /** The key of `command` in `scope` (its first binding), if any. */
  keyFor(command: string, scope = 'window'): string | undefined {
    return this.bindings().find((b) => b.command === command && b.scope === scope)?.key;
  }

  /** The keys `command` declares itself, ignoring the user's overrides. */
  declared(command: string): string[] {
    return declaredKeys(this.registry.keybindings(), command);
  }

  /** Binds `command` to `key` (`null` removes its key). Refuses an invalid key, and a taken one unless `force`. */
  bind(command: string, key: string | null, options: { readonly force?: boolean } = {}): BindResult {
    if (key !== null) {
      const canon = normalizeKey(key);
      if (!canon) return { ok: false, reason: 'invalid' };
      const scope = this.registry.keybindings().find((k) => k.command === command)?.scope ?? 'window';
      const taken = occupantsOf(canon, scope, command, this.bindings(), this.reserved);
      if (!options.force && (taken.commands.length || taken.reserved.length)) return { ok: false, reason: 'conflict', ...taken };
    }
    this.store(withOverride(this.state(), command, key, this.declared(command)));
    return { ok: true };
  }

  /** Back to the key `command` declares. */
  reset(command: string): void {
    this.store(withoutOverride(this.state(), command));
  }

  /** Back to every declared key. */
  resetAll(): void {
    this.store({});
  }

  /** Stops key presses from running commands (while a binding is recorded) until `resume`. */
  suspend(): void {
    this.paused.set(true);
  }

  resume(): void {
    this.paused.set(false);
  }

  /**
   * Runs the command bound to a key press, if any: the innermost active scope first (`data-pd-key-scope`, then
   * `window`), only commands that are offered and enabled, never while suspended, and never a key a text field keeps
   * for itself. Returns the command id it started, or null.
   */
  dispatch(e: KeyboardEvent): string | null {
    if (e.defaultPrevented || e.isComposing || this.paused()) return null;
    const pressed = eventPressed(e);
    if (!pressed) return null;
    if (isTextEntry(e.target instanceof Element ? e.target : null) && textFieldKeeps(e, this.platform)) return null;
    const command = this.commandFor(pressed, activeScopes(e.target));
    if (!command) return null;
    e.preventDefault();
    this.lastRun = { id: command, at: Date.now() };
    void this.commands.run(command);
    return command;
  }

  /** The command a key press would run now in `scopes` (innermost first), or null. */
  commandForKey(e: KeyInput, scopes: readonly string[] = ['window']): string | null {
    const pressed = eventPressed(e);
    return pressed ? this.commandFor(pressed, scopes) : null;
  }

  /** Whether `id` was just started by a key press (within `ms`): the native menu bar's echo of the same shortcut. */
  ranByKeyWithin(id: string, ms: number, now = Date.now()): boolean {
    return this.lastRun !== null && this.lastRun.id === id && now - this.lastRun.at < ms;
  }

  private commandFor(pressed: string, scopes: readonly string[]): string | null {
    const available = new Set(this.commands.available().map((c) => c.id));
    for (const scope of scopes) {
      for (const b of this.bindings()) {
        if (b.scope !== scope || pressedKeys(b.key, this.platform) !== pressed || !available.has(b.command)) continue;
        const c = this.commands.find(b.command);
        if (c && this.commands.isEnabled(c)) return b.command;
      }
    }
    return null;
  }

  private store(overrides: KeybindingOverrides): void {
    const clean = sanitizeOverrides(overrides);
    this.state.set(clean);
    if (!saveOverrides(clean, this.storage)) console.warn('[paddock-menu] the key bindings could not be stored; they last for this session');
  }
}

/** Runs commands from key presses in this window (a `keydown` listener on the document that `Keymap.dispatch`es). */
export function provideKeyboardShortcuts(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideEnvironmentInitializer(() => {
      if (typeof document === 'undefined') return;
      const keymap = inject(Keymap);
      const onKey = (e: KeyboardEvent) => void keymap.dispatch(e);
      document.addEventListener('keydown', onKey);
      inject(DestroyRef).onDestroy(() => document.removeEventListener('keydown', onKey));
    }),
  ]);
}
