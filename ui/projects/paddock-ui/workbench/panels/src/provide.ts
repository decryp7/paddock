import { EnvironmentProviders, inject, makeEnvironmentProviders, provideAppInitializer, type Provider } from '@angular/core';
import { PANEL_WINDOWS, VIEW_PANEL } from '@decryp7/paddock-ui/workbench';
import { DIALOG_ROUTE } from '@decryp7/paddock-ui/shell/dialogs';
import { WINDOW_CHROME, type WindowChromeIo } from './chrome';
import { PANELS_IO, type PanelsIo } from './io';
import { hostTitleValue, PANEL_WINDOWS_OPTIONS, PanelWindows, type PanelWindowsOptions } from './panel-windows';
import { POPOUT_ROUTE } from './popout-route';
import { PopoutState } from './popout-state';
import { syncedValues, UiSync } from './ui-sync';
import { WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';

/**
 * Pop-out windows for the main window (plan W3): `PanelWindows` provided for the core's `PANEL_WINDOWS` port, over `io` (the
 * desktop build passes `tauriPanelsIo(bridge)`; the browser build and specs use `InPageDesktop().window('main')`, which
 * simulates popped-out panels without real windows). The host side starts at app start (it adopts pop-outs the shell still
 * has after a reload). A pop-out window uses `providePopoutWindow` instead, never both.
 */
export function providePanelWindows(io: PanelsIo, options: PanelWindowsOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: PANELS_IO, useValue: io },
    { provide: PANEL_WINDOWS_OPTIONS, useValue: options },
    UiSync,
    PanelWindows,
    { provide: PANEL_WINDOWS, useExisting: PanelWindows },
    provideAppInitializer(() => void inject(PanelWindows).start()),
  ]);
}

/** What a pop-out window is set up with. */
export interface PopoutWindowOptions {
  /** The window's chrome (the desktop build passes `tauriWindowChrome(getCurrentWindow(), isMac)`); default: the browser's. */
  readonly chrome?: WindowChromeIo;
}

/**
 * The providers of a pop-out window (rule 34: each OS window is its own bootstrap with a small provider set). The app's
 * `main.ts` bootstraps `PdPopoutShell` when `parsePopoutRoute(location.hash)` finds a route, with these and the app's
 * contributions (so the registry knows the view and the shared state), but none of the host-only services (autosave, file
 * opening, the dock): the window has no `PANEL_WINDOWS` (a pop-out never hosts), and `VIEW_PANEL` is the panel it shows.
 */
export function providePopoutWindow(io: PanelsIo, options: PopoutWindowOptions = {}): EnvironmentProviders {
  const providers: Provider[] = [
    { provide: PANELS_IO, useValue: io },
    { provide: PANEL_WINDOWS, useValue: null },
    {
      provide: VIEW_PANEL,
      useFactory: () => {
        const route = inject(POPOUT_ROUTE);
        if (!route) throw new Error('this window is not a pop-out (no #/popout route)');
        return route.panel;
      },
    },
    PopoutState,
    UiSync,
    ...(options.chrome ? [{ provide: WINDOW_CHROME, useValue: options.chrome }] : []),
  ];
  return makeEnvironmentProviders(providers);
}

/**
 * A dialog window shares the host's display state (theme, formats: the `display`-scope `SharedStateDef`s) like a pop-out
 * does, and nothing else. One line in the dialog window's providers; it starts at app start and never fails the window.
 */
export function provideDialogUiSync(io: PanelsIo): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: PANELS_IO, useValue: io },
    UiSync,
    provideAppInitializer(() => {
      const route = inject(DIALOG_ROUTE);
      if (!route) return;
      const sync = inject(UiSync);
      const registry = inject(WorkbenchRegistry);
      sync.start(io, 'dialog', route.label, [...syncedValues(registry, 'dialog'), hostTitleValue(() => '', () => undefined)]);
    }),
  ]);
}

/**
 * Makes the page the pop-out shell's: the app's `index.html` carries the main window's root element (`<app-root>`), which a
 * pop-out window does not bootstrap; this puts `<pd-popout-shell>` in its place. Call it before `bootstrapApplication(PdPopoutShell, …)`
 * (the twin of the dialogs' `mountDialogWindowHost`).
 */
export function mountPopoutWindowHost(doc: Document = document): HTMLElement {
  const el = doc.createElement('pd-popout-shell');
  doc.body.replaceChildren(el);
  return el;
}
