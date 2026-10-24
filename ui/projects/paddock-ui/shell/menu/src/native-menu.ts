import {
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
import { SHELL_IDLE, ShellContextHost } from '@decryp7/paddock-ui/shell';
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

/**
 * Schedules a send of the menus for when the window is next idle and returns its cancel: changes that arrive before
 * then are sent as one (the menus settle in bursts while features register). Tests run the work by hand.
 */
export type NativeMenuSchedule = (work: () => void) => () => void;

export const NATIVE_MENU_SCHEDULE = new InjectionToken<NativeMenuSchedule>('PADDOCK_NATIVE_MENU_SCHEDULE', {
  providedIn: 'root',
  factory: () => {
    const idle = inject(SHELL_IDLE);
    return (work) => {
      let live = true;
      idle(() => live && work());
      return () => void (live = false);
    };
  },
});

/**
 * Opens when the app is ready: by default once the app's part of the context has loaded (or failed to; until then
 * `when`, `enabled` and `checked` cannot be asked, so the menus are not final). The load is started when the window is
 * idle. The start-up contract or the app may provide its own ready signal. The native menu bar is then built once instead
 * of once per feature that registers during the start.
 */
export const NATIVE_MENU_GATE = new InjectionToken<Signal<boolean>>('PADDOCK_NATIVE_MENU_GATE', {
  providedIn: 'root',
  factory: () => {
    const host = inject(ShellContextHost);
    const open = signal(host.context() !== null);
    if (!open()) {
      const settle = () => open.set(true);
      inject(SHELL_IDLE)(() => void host.load().then(settle, settle));
    }
    return open.asReadonly();
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
 * is ready (`NATIVE_MENU_GATE`), changes coalesced until the window is idle (`NATIVE_MENU_SCHEDULE`), and only when it
 * changed (Rust diffs again and updates check marks in place); it is sent again when the window gets the focus, as the
 * menu bar shows the focused window's menus.
 */
@Injectable({ providedIn: 'root' })
export class NativeMenuBridge {
  private readonly port = inject(NATIVE_MENU_PORT);
  private readonly app = inject(AppMenu);
  private readonly keymap = inject(Keymap);
  private readonly destroyRef = inject(DestroyRef);
  private readonly injector = inject(Injector);
  private readonly gate = inject(NATIVE_MENU_GATE);
  private readonly later = inject(NATIVE_MENU_SCHEDULE);
  private readonly noShortcut = inject(NO_NATIVE_SHORTCUT);
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
    // An effect because the native menu bar is outside the signal graph (IPC); it only schedules a send.
    effect(
      () => {
        const model = toNativeModel(this.app.menus(), this.noShortcut);
        if (!this.gate()) return;
        untracked(() => this.schedule(model));
      },
      { injector: this.injector },
    );
    const onFocus = () => {
      if (!untracked(this.gate)) return;
      this.lastSent = '';
      this.flush(toNativeModel(untracked(this.app.menus), this.noShortcut));
    };
    window.addEventListener('focus', onFocus);
    let unlisten: (() => void) | null = null;
    let destroyed = false;
    // Menu events are listened to once the app is ready, like the first build: nothing reaches the IPC before then.
    const listen = () =>
      this.port.onMenu((id) => this.chosen(id)).then(
        (u) => (destroyed ? u() : (unlisten = u)),
        (e: unknown) => console.warn(`[paddock-menu] native menu events are unavailable: ${String(e)}`),
      );
    const listening = effect(
      () => {
        if (!this.gate()) return;
        untracked(listen);
        listening.destroy();
      },
      { injector: this.injector },
    );
    this.destroyRef.onDestroy(() => {
      destroyed = true;
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

  /** Sends `model` when the window is next idle, replacing a model still waiting. */
  private schedule(model: NativeMenuModel): void {
    this.pending = model;
    this.cancelPending?.();
    this.cancelPending = this.later(() => {
      this.cancelPending = null;
      const m = this.pending;
      this.pending = null;
      if (m) this.send(m);
    });
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
