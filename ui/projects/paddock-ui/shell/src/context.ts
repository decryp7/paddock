import {
  EnvironmentInjector,
  EnvironmentProviders,
  inject,
  Injectable,
  InjectionToken,
  Injector,
  makeEnvironmentProviders,
  provideEnvironmentInitializer,
  runInInjectionContext,
  Signal,
  signal,
} from '@angular/core';
import { DISMISSED, type DialogOutcome } from './types';

/** A notice the shell shows (plan S4 renders it; `kind` names a contributed notification kind). */
export interface ShellNotice {
  readonly level: 'info' | 'success' | 'warning' | 'error';
  readonly message: string;
  /**
   * A short form for a small window, where a toast is one line ("Layout fitted"); the full `message` stays in Notifications.
   * Kept for a notice this window keeps itself (one with an `action`); the engine's messages show their title.
   */
  readonly short?: string;
  readonly kind?: string;
  /**
   * One button for a window-local follow-up (Undo a layout fit, Retry a pop-out): the notice stays in this window (the
   * engine cannot run it) and the button runs the window's `NOTICE_ACTION_HANDLERS` entry of `action.type`.
   */
  readonly action?: { readonly type: string } & Readonly<Record<string, unknown>>;
}

/** The shell's part of every context: what any contribution may ask the shell to do. */
export interface ShellCore {
  /** Runs a contributed command by id; resolves false when it is unknown, unavailable or failed (never throws). */
  runCommand(id: string, args?: unknown): Promise<boolean>;
  /** Shows a contributed page (pages frame, plan S14). */
  showPage(id: string): void;
  /** Opens a contributed dialog and resolves with its outcome (answered with a value, or dismissed; ADR 0026 §2). */
  openDialog<R = unknown>(id: string, inputs?: unknown): Promise<DialogOutcome<R>>;
  /** Shows a notice (notification host, plan S4). */
  notify(notice: ShellNotice): void;
}

/**
 * What contributions' live functions receive: the shell's part plus the app's (`ShellContext<PitwallContext>`). The app's
 * part is loaded on first use or when the window is idle (see `provideShellContext`), so its stores are not in the
 * first-paint bundle. Keep the app part small and purpose-built (interface segregation).
 */
export type ShellContext<T extends object = object> = ShellCore & Readonly<T>;

/**
 * The parts of `ShellCore` that later packages implement: pages (S14), dialogs (S6) and notifications (S4). Until a
 * window provides one, the default logs a warning and does nothing (degrade, don't fail).
 */
export interface ShellPorts {
  showPage(id: string): void;
  openDialog(id: string, inputs?: unknown): Promise<DialogOutcome<unknown>>;
  notify(notice: ShellNotice): void;
}

export const SHELL_PORTS = new InjectionToken<ShellPorts>('SHELL_PORTS', {
  providedIn: 'root',
  factory: () => ({
    showPage: (id) => console.warn(`[paddock-shell] no pages frame in this window; cannot show page ${id}`),
    openDialog: (id) => {
      console.warn(`[paddock-shell] no dialog service in this window; cannot open dialog ${id}`);
      return Promise.resolve(DISMISSED);
    },
    notify: (n) => (n.level === 'error' ? console.error : console.warn)(`[paddock-shell] ${n.message}`),
  }),
});

/** Runs a command for `ShellCore.runCommand`: `CommandService`, wired by `provideShellCore` (a token avoids an import cycle). */
export const SHELL_COMMAND_RUNNER = new InjectionToken<(id: string, args?: unknown) => Promise<boolean>>('SHELL_COMMAND_RUNNER');

/** Builds the app's part of the context; runs once, in the root injection context, so it may `inject()` stores. */
export type AppContextFactory<T extends object> = () => T;

/** When the app's context is loaded: when the window is idle after start-up (default), or only on first use. */
export type ContextPrefetch = 'idle' | 'first-use';

interface AppContextConfig {
  readonly load: () => Promise<AppContextFactory<object>>;
  readonly prefetch: ContextPrefetch;
}

const APP_CONTEXT = new InjectionToken<AppContextConfig>('PADDOCK_APP_CONTEXT');

/**
 * Registers the app's part of the context as a loader, so the module that builds it (and the stores it injects) is a
 * lazy chunk: `provideShellContext<PitwallContext>(() => import('./app-context').then((m) => m.pitwallContext))`.
 */
export function provideShellContext<T extends object>(
  load: () => Promise<AppContextFactory<T>>,
  options: { prefetch?: ContextPrefetch } = {},
): EnvironmentProviders {
  const prefetch = options.prefetch ?? 'idle';
  return makeEnvironmentProviders([
    { provide: APP_CONTEXT, useValue: { load, prefetch } satisfies AppContextConfig },
    ...(prefetch === 'idle' ? [provideEnvironmentInitializer(() => inject(ShellContextHost).prefetchWhenIdle())] : []),
  ]);
}

/** Schedules work for when the window is idle (`requestIdleCallback`, else a macrotask). */
export const SHELL_IDLE = new InjectionToken<(work: () => void) => void>('SHELL_IDLE', {
  providedIn: 'root',
  factory: () => (work) => {
    const w = globalThis as { requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number };
    if (typeof w.requestIdleCallback === 'function') w.requestIdleCallback(work, { timeout: 2000 });
    else setTimeout(work, 0);
  },
});

/** Holds the window's context: the shell's part at once, the app's part once loaded. */
@Injectable({ providedIn: 'root' })
export class ShellContextHost {
  private readonly injector = inject(EnvironmentInjector);
  private readonly ports = inject(SHELL_PORTS);
  private readonly config = inject(APP_CONTEXT, { optional: true });
  private readonly idle = inject(SHELL_IDLE);
  private readonly core: ShellCore = Object.freeze({
    runCommand: (id: string, args?: unknown) => {
      const run = this.injector.get(SHELL_COMMAND_RUNNER, null);
      if (run) return run(id, args);
      console.warn(`[paddock-shell] no command runner in this window (provideShellCore); cannot run ${id}`);
      return Promise.resolve(false);
    },
    showPage: (id: string) => this.ports.showPage(id),
    openDialog: <R>(id: string, inputs?: unknown) => this.ports.openDialog(id, inputs) as Promise<DialogOutcome<R>>,
    notify: (n: ShellNotice) => this.ports.notify(n),
  });
  private readonly loaded = signal<ShellContext | null>(this.config ? null : this.core);
  private pending: Promise<ShellContext> | null = null;

  /** The context once complete (immediately when the app has no part); `null` while the app's part is not loaded. */
  readonly context: Signal<ShellContext | null> = this.loaded.asReadonly();

  /** The complete context, loading the app's part on first call. A failed load is retried on the next call. */
  load(): Promise<ShellContext> {
    const ready = this.loaded();
    if (ready) return Promise.resolve(ready);
    if (!this.pending) {
      const config = this.config as AppContextConfig;
      this.pending = config
        .load()
        .then((factory) => {
          const app = runInInjectionContext(this.injector as Injector, factory);
          const ctx = combine(this.core, app);
          this.loaded.set(ctx);
          return ctx;
        })
        .catch((e: unknown) => {
          this.pending = null;
          throw e;
        });
    }
    return this.pending;
  }

  /** Starts loading when the window is idle; a failure is reported once and retried on first use. */
  prefetchWhenIdle(): void {
    if (this.loaded()) return;
    this.idle(() => {
      this.load().catch((e: unknown) => this.ports.notify({ level: 'error', message: $localize`:@@pd.shell.context-failed:The app context failed to load: ${describe(e)}:cause:` }));
    });
  }
}

/** The shell's methods win over an app member of the same name; app getters keep their own `this`. */
function combine(core: ShellCore, app: object): ShellContext {
  return new Proxy(app, {
    get: (target, prop) => (prop in core ? core[prop as keyof ShellCore] : Reflect.get(target, prop, target)),
    has: (target, prop) => prop in core || prop in target,
    set: () => false,
    defineProperty: () => false,
    deleteProperty: () => false,
  }) as ShellContext;
}

export function describe(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
