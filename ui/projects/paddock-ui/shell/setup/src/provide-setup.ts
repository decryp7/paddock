import { effect, EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer, Provider, untracked, type Signal } from '@angular/core';
import { SHELL_IDLE, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { SetupBackend } from './backend';
import { SETUP_DIALOG_ID } from './setup.contributions';
import { SetupStore } from './setup.store';

export interface ShellSetupOptions {
  /** Builds the window's backend (in an injection context): `new TauriSetupBackend(ipc)`, a fake in the browser. */
  readonly backend: () => SetupBackend;
  /**
   * The main window: once the window is ready and idle, ask the engine once whether the checklist opens by itself (a first
   * run with steps still open) and open the Set-up window if so. Off in the Set-up window and the Settings window.
   */
  readonly main?: boolean;
  /**
   * The main window's start-up work waits for this (in an injection context): the window's ready signal, so nothing reaches
   * the IPC before the start-up contract's reply (`windowReady` from `shell/startup`). Default: ready.
   */
  readonly ready?: () => Signal<boolean>;
}

/**
 * The first-run checklist for one window (rule 32, 34): the backend, and the store in the windows that show it. The main
 * window calls it with `main: true` and no store (it only asks once); the Set-up window's `providers` module calls it
 * without, which also provides the store.
 */
export function provideShellSetup(options: ShellSetupOptions): EnvironmentProviders {
  const main = options.main ?? false;
  const providers: (Provider | EnvironmentProviders)[] = [{ provide: SetupBackend, useFactory: options.backend }];
  if (main) providers.push(provideEnvironmentInitializer(() => openOnFirstRun(options.ready?.() ?? null)));
  else providers.push(SetupStore);
  return makeEnvironmentProviders(providers);
}

/**
 * Once the window is ready and then idle: asks the engine whether the checklist opens by itself, and opens it. A failed ask
 * is logged and nothing else happens: the checklist stays one command away (Help > Set-up…).
 */
function openOnFirstRun(ready: Signal<boolean> | null): void {
  const ports = inject(SHELL_PORTS);
  const idle = inject(SHELL_IDLE);
  const backend = inject(SetupBackend);
  const start = () =>
    idle(() => {
      backend
        .state()
        .then((s) => {
          if (!s.showOnStart) return;
          // Shown once: the steps stay in Settings and Help whatever the user does with the window.
          void backend.finish().catch(() => undefined);
          return ports.openDialog(SETUP_DIALOG_ID);
        })
        .catch((e: unknown) => console.warn(`[paddock-setup] first-run check: ${e instanceof Error ? e.message : String(e)}`));
    });
  if (!ready) {
    start();
    return;
  }
  // A side effect outside the signal graph (IPC), once: when the window has reported ready.
  let started = false;
  const watch = effect(() => {
    if (!ready() || started) return;
    started = true;
    untracked(start);
    watch.destroy();
  });
}
