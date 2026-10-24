import { effect, untracked, EnvironmentProviders, inject, Injector, makeEnvironmentProviders, provideEnvironmentInitializer, Provider, type Signal } from '@angular/core';
import { SHELL_IDLE, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { AboutBackend, UpdatesBackend } from './backend';
import { ABOUT_DIALOG_IDS } from './about.contributions';
import { UPDATES_CONFIG, UpdatesStore } from './updates.store';

export interface ShellAboutOptions {
  /** Builds the window's About backend (in an injection context): `new TauriAboutBackend(ipc)`, a fake in the browser. */
  readonly about: () => AboutBackend;
  /** Builds the window's updates backend: `new TauriUpdatesBackend(ipc)`, a fake in the browser. */
  readonly updates: () => UpdatesBackend;
  /**
   * The main window: announce a newly found update as a notice, and once per start ask the engine whether to open What's
   * New (after an update to a version with notes; never on a first run). Off in dialog windows.
   */
  readonly main?: boolean;
  /**
   * The main window's start-up work waits for this (in an injection context): the window's ready signal, so nothing reaches
   * the IPC before the start-up contract's reply (`windowReady` from `shell/startup`). Default: ready.
   */
  readonly ready?: () => Signal<boolean>;
}

/**
 * About, Updates and What's New for one window (rule 32, 34): the backends and the updates store. The main window calls it
 * in its bootstrap with `main: true`; the dialogs' `providers` module calls it without.
 */
export function provideShellAbout(options: ShellAboutOptions): EnvironmentProviders {
  const main = options.main ?? false;
  const providers: (Provider | EnvironmentProviders)[] = [
    { provide: AboutBackend, useFactory: options.about },
    { provide: UpdatesBackend, useFactory: options.updates },
    { provide: UPDATES_CONFIG, useValue: Object.freeze({ announce: main }) },
    UpdatesStore,
  ];
  if (main) providers.push(provideEnvironmentInitializer(() => whatsNewOnStart(options.ready?.() ?? null)));
  return makeEnvironmentProviders(providers);
}

/**
 * Once the window is ready (its start-up reply handled) and then idle: starts the main window's updates store (it listens
 * for update events, so a found update is announced), and asks the engine once whether to show What's New, opening it if
 * so.
 */
function whatsNewOnStart(ready: Signal<boolean> | null): void {
  const injector = inject(Injector);
  const ports = inject(SHELL_PORTS);
  const idle = inject(SHELL_IDLE);
  const start = () =>
    idle(() => {
      injector.get(UpdatesStore);
      injector
        .get(AboutBackend)
        .whatsNewOnStart()
        .then((d) => (d === 'show' ? ports.openDialog(ABOUT_DIALOG_IDS.whatsNew) : undefined))
        .catch((e: unknown) => console.warn(`[paddock-about] What's New at start-up: ${e instanceof Error ? e.message : String(e)}`));
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
