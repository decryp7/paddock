import {
  computed,
  DestroyRef,
  effect,
  EnvironmentProviders,
  inject,
  Injectable,
  InjectionToken,
  Injector,
  makeEnvironmentProviders,
  provideEnvironmentInitializer,
  Signal,
  signal,
  untracked,
} from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { MENU_EVENT, MENU_SET_COMMAND, type MenuApplied, type NativeMenuEntry, type NativeMenuModel } from '../generated/menu';
import { AppMenu } from './app-menu';
import { Keymap } from './keymap';
import type { MenuNode, TopMenu } from './menu-model';

/** The shell side of the native menu bar (`paddock-menu` on macOS); fakes in tests. */
export interface NativeMenuPort {
  /** Hands the model to the shell, which builds (or updates in place) the native menu bar from it. */
  setAppMenu(menu: NativeMenuModel): Promise<MenuApplied | void>;
  /** Calls `handler` with the item id when a native menu item is chosen while this window is the menu bar's. */
  onMenu(handler: (id: string) => void): Promise<() => void>;
}

/** The IPC an app hands to `tauriNativeMenuPort` (from `@tauri-apps/api`: `invoke`, and the current window's `listen`). */
export interface NativeMenuIpc {
  invoke(command: string, args: Record<string, unknown>): Promise<unknown>;
  /** Listens to an event sent to this window; resolves to the unlisten function. */
  listen(event: string, handler: (payload: unknown) => void): Promise<() => void>;
}

/**
 * The port over Tauri: `menu_set` and `paddock://menu`. The app passes its IPC functions, so the shell package does not
 * depend on `@tauri-apps/api`; `command` is the plugin's name for it once the shell is a plugin
 * (`plugin:paddock-shell|menu_set`, plan S17).
 */
export function tauriNativeMenuPort(ipc: NativeMenuIpc, command: string = MENU_SET_COMMAND): NativeMenuPort {
  return {
    setAppMenu: async (menu) => (await ipc.invoke(command, { menu })) as MenuApplied,
    onMenu: (handler) =>
      ipc.listen(MENU_EVENT, (payload) => {
        if (typeof payload === 'string') handler(payload);
      }),
  };
}

/** No native menu bar (the browser, tests): models are dropped and no item is ever chosen. */
export const NO_NATIVE_MENU: NativeMenuPort = Object.freeze({
  setAppMenu: () => Promise.resolve(),
  onMenu: () => Promise.resolve(() => undefined),
});

export const NATIVE_MENU_PORT = new InjectionToken<NativeMenuPort>('PADDOCK_NATIVE_MENU_PORT', { providedIn: 'root', factory: () => NO_NATIVE_MENU });

/** When the menu bar is built: after the gate opens (or at most `maxWaitMs` after start), changes coalesced. */
export interface NativeMenuTiming {
  /** Changes within this long are sent as one (the menus settle in bursts while features register). */
  readonly debounceMs: number;
  /** The quiet time before the first build: features deferred to the first idle moment register just after ready. */
  readonly firstDebounceMs: number;
  /** The menus are sent this long after start even if the gate never opens. */
  readonly maxWaitMs: number;
  /** Runs `fn` after `ms`; returns a cancel function (tests run timers by hand). */
  setTimer(fn: () => void, ms: number): () => void;
}

export const NATIVE_MENU_TIMING = new InjectionToken<NativeMenuTiming>('PADDOCK_NATIVE_MENU_TIMING', {
  providedIn: 'root',
  factory: () => ({
    debounceMs: 150,
    firstDebounceMs: 600,
    maxWaitMs: 3000,
    setTimer: (fn, ms) => {
      const id = setTimeout(fn, ms);
      return () => clearTimeout(id);
    },
  }),
});

/**
 * Opens when the app is ready: by default once the app's part of the context is loaded (until then `when`, `enabled`
 * and `checked` cannot be asked, so the menus are not final). The start-up contract (plan S7) or the app may provide its
 * own ready signal. The native menu bar is then built once instead of once per feature that registers during the start.
 */
export const NATIVE_MENU_GATE = new InjectionToken<Signal<boolean>>('PADDOCK_NATIVE_MENU_GATE', {
  providedIn: 'root',
  factory: () => {
    const host = inject(ShellContextHost);
    return computed(() => host.context() !== null);
  },
});

/**
 * Commands whose keys the standard macOS Edit items (Undo/Redo of text fields) already show: listed without a shortcut
 * so the menu bar does not offer the same key twice. The key still reaches the page.
 */
export const NO_NATIVE_SHORTCUT = new InjectionToken<ReadonlySet<string>>('PADDOCK_NO_NATIVE_SHORTCUT', {
  providedIn: 'root',
  factory: () => new Set(['pd.edit.undo', 'pd.edit.redo']),
});

function entries(nodes: readonly MenuNode[], noShortcut: ReadonlySet<string>): NativeMenuEntry[] {
  return nodes.map((n): NativeMenuEntry => {
    switch (n.kind) {
      case 'separator':
        return { kind: 'separator' };
      case 'submenu':
        return { kind: 'submenu', label: n.label, items: entries(n.items, noShortcut) };
      default:
        return {
          kind: 'item',
          id: n.id,
          label: n.label,
          keybinding: n.keybinding && !noShortcut.has(n.id) ? n.keybinding : null,
          checked: n.checked ?? null,
          enabled: n.enabled,
        };
    }
  });
}

/** The wire form of the menus for the native menu bar. Pure. */
export function toNativeModel(menus: readonly TopMenu[], noShortcut: ReadonlySet<string> = new Set()): NativeMenuModel {
  return { menus: menus.map((m) => ({ id: m.id, label: m.label, items: entries(m.items, noShortcut) })) };
}

/** A menu event this soon after a key press that ran the same command is that key press echoed by the menu bar. */
export const ECHO_MS = 400;

/**
 * Mirrors the menus into the macOS native menu bar and runs what is chosen there. The first model is sent once the app
 * is ready (`NATIVE_MENU_GATE`), later changes coalesced (`NATIVE_MENU_TIMING`), and only when it changed (Rust diffs
 * again and updates check marks in place); it is sent again when the window gets the focus, as the menu bar shows the
 * focused window's menus.
 */
@Injectable({ providedIn: 'root' })
export class NativeMenuBridge {
  private readonly port = inject(NATIVE_MENU_PORT);
  private readonly app = inject(AppMenu);
  private readonly keymap = inject(Keymap);
  private readonly destroyRef = inject(DestroyRef);
  private readonly injector = inject(Injector);
  private readonly gate = inject(NATIVE_MENU_GATE);
  private readonly timing = inject(NATIVE_MENU_TIMING);
  private readonly noShortcut = inject(NO_NATIVE_SHORTCUT);
  /** The start's longest wait is over. */
  private readonly waited = signal(false);
  private readonly open = computed(() => this.gate() || this.waited());
  private readonly sentCount = signal(0);
  private pending: NativeMenuModel | null = null;
  private cancelPending: (() => void) | null = null;
  private started = false;
  private lastSent = '';

  /** How many models were sent (a start sends one). */
  readonly sent: Signal<number> = this.sentCount.asReadonly();

  /** Starts mirroring. Idempotent. */
  start(): void {
    if (this.started) return;
    this.started = true;
    const cancelWait = this.timing.setTimer(() => this.waited.set(true), this.timing.maxWaitMs);
    // An effect because the native menu bar is outside the signal graph (IPC); it only schedules a send.
    effect(
      () => {
        const model = toNativeModel(this.app.menus(), this.noShortcut);
        if (!this.open()) return;
        untracked(() => this.schedule(model));
      },
      { injector: this.injector },
    );
    const onFocus = () => {
      if (!untracked(this.open)) return;
      this.lastSent = '';
      this.flush(toNativeModel(untracked(this.app.menus), this.noShortcut));
    };
    window.addEventListener('focus', onFocus);
    let unlisten: (() => void) | null = null;
    let destroyed = false;
    this.port.onMenu((id) => this.chosen(id)).then(
      (u) => (destroyed ? u() : (unlisten = u)),
      (e: unknown) => console.warn(`[paddock-menu] native menu events are unavailable: ${String(e)}`),
    );
    this.destroyRef.onDestroy(() => {
      destroyed = true;
      cancelWait();
      this.cancelPending?.();
      window.removeEventListener('focus', onFocus);
      unlisten?.();
    });
  }

  /** Runs a native menu item, unless it echoes the key press that just ran it. */
  chosen(id: string): void {
    if (this.keymap.ranByKeyWithin(id, ECHO_MS)) return;
    void this.app.run(id);
  }

  /** Sends `model` after the debounce, replacing a model still waiting. */
  private schedule(model: NativeMenuModel): void {
    this.pending = model;
    this.cancelPending?.();
    this.cancelPending = this.timing.setTimer(
      () => {
        this.cancelPending = null;
        const m = this.pending;
        this.pending = null;
        if (m) this.send(m);
      },
      this.sentCount() > 0 ? this.timing.debounceMs : this.timing.firstDebounceMs,
    );
  }

  /** Sends `model` now, dropping a model still waiting. */
  private flush(model: NativeMenuModel): void {
    this.cancelPending?.();
    this.cancelPending = null;
    this.pending = null;
    this.send(model);
  }

  private send(model: NativeMenuModel): void {
    const json = JSON.stringify(model);
    if (json === this.lastSent) return;
    this.lastSent = json;
    this.sentCount.update((n) => n + 1);
    this.port.setAppMenu(model).catch((e: unknown) => {
      this.lastSent = '';
      console.warn(`[paddock-menu] the native menu bar could not be updated: ${messageOf(e)}`);
    });
  }
}

function messageOf(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === 'object' && e !== null && typeof (e as { message?: unknown }).message === 'string') return (e as { message: string }).message;
  return String(e);
}

/**
 * The native menu bar of this window (macOS desktop app): its port (`tauriNativeMenuPort(...)`), started when the menu
 * mode is native. Only the main window class provides it; dialogs have no menus of their own.
 */
export function provideNativeMenu(options: { readonly port?: NativeMenuPort; readonly gate?: () => Signal<boolean> } = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...(options.port ? [{ provide: NATIVE_MENU_PORT, useValue: options.port }] : []),
    ...(options.gate ? [{ provide: NATIVE_MENU_GATE, useFactory: options.gate }] : []),
    provideEnvironmentInitializer(() => {
      if (inject(AppMenu).mode === 'native') inject(NativeMenuBridge).start();
    }),
  ]);
}
