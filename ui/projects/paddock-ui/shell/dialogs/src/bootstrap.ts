import {
  ApplicationConfig,
  EnvironmentProviders,
  inject,
  makeEnvironmentProviders,
  provideBrowserGlobalErrorListeners,
  Provider,
} from '@angular/core';
import { provideShellCore, type DialogDef, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { DialogService } from './dialog.service';
import { DIALOG_IO, type DialogIo } from './io';
import { DIALOG_ROUTE, parseDialogRoute, type DialogRoute } from './route';
import { DIALOG_LOOKUP, DialogPresenter } from './types';
import { WindowDialogBackend } from './window-backend';
import { InPageDialogBackend } from './in-page';

type Providers = readonly (Provider | EnvironmentProviders)[];

export interface DialogsOptions {
  /**
   * The window system (`() => tauriDialogIo(bridge)` in the desktop build): dialogs then open as OS windows. Without it
   * they open in the page (the browser build, e2e).
   */
  readonly io?: () => DialogIo;
}

/**
 * The dialog layer of a main window (or a pop-out): `DialogService` opens contributed dialogs as windows or in the page.
 * Put `<pd-dialog-host />` in the window's root template, and route `ShellPorts.openDialog` to `openDialogPort()`.
 */
export function provideDialogs(options: DialogsOptions = {}): EnvironmentProviders {
  const io = options.io;
  return makeEnvironmentProviders([
    ...(io ? [{ provide: DIALOG_IO, useFactory: io }] : []),
    { provide: DialogPresenter, useExisting: io ? WindowDialogBackend : InPageDialogBackend },
  ]);
}

/** `ShellPorts.openDialog` over the window's `DialogService` (call in an injection context, e.g. the ports' factory). */
export function openDialogPort(): ShellPorts['openDialog'] {
  const dialogs = inject(DialogService);
  return (id, inputs) => dialogs.openById(id, inputs);
}

export interface DialogWindowOptions {
  /** The app's dialogs (the `dialogs` of its registration batches): the window finds its own by the route's id. */
  readonly dialogs: readonly DialogDef[];
  /**
   * What every dialog window of the app boots with besides the shell's (its IPC backend, logger, icons, theme): keep it
   * small (rule 34). A dialog's own providers come from its `providers` loader.
   */
  readonly providers?: Providers;
  /** The window system; defaults to the browser build's (the route opened by URL). */
  readonly io?: () => DialogIo;
  /** Defaults to the window's location. */
  readonly route?: DialogRoute | null;
}

/**
 * The application config of a dialog window (rule 34: each OS window is its own small bootstrap): the shell core, the
 * route and window system, the dialog lookup and the app's base set. It has no main-window machinery and no other
 * window's stores; the dialog's component and providers load when the shell starts.
 *
 * ```ts
 * const route = parseDialogRoute(location.hash);
 * if (route) {
 *   mountDialogWindowHost();
 *   bootstrapApplication(DialogShell, dialogWindowConfig({ dialogs: APP_DIALOGS, providers: dialogBase(), io, route }));
 * }
 * ```
 */
export function dialogWindowConfig(options: DialogWindowOptions): ApplicationConfig {
  const route = options.route === undefined ? (typeof location === 'undefined' ? null : parseDialogRoute(location.hash)) : options.route;
  const byId = new Map(options.dialogs.map((d) => [d.id, d]));
  return {
    providers: [
      provideBrowserGlobalErrorListeners(),
      provideShellCore(),
      { provide: DIALOG_ROUTE, useValue: route },
      { provide: DIALOG_LOOKUP, useValue: (id: string) => byId.get(id) },
      // A dialog may open another dialog (its contract then lists `dialog_open`).
      provideDialogs({ io: options.io }),
      ...(options.providers ?? []),
    ],
  };
}

/** Replaces the page's body with the dialog shell's element (`bootstrapApplication(DialogShell, ...)` renders there). */
export function mountDialogWindowHost(doc: Document = document): HTMLElement {
  const el = doc.createElement('pd-dialog-shell');
  doc.body.replaceChildren(el);
  return el;
}
