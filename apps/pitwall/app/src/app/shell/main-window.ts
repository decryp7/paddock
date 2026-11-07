import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, provideAppInitializer, provideBrowserGlobalErrorListeners } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { NavigationCancel, NavigationEnd, NavigationError, Router } from '@angular/router';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { PdCommandPalette, providePagesPalette } from '@decryp7/paddock-ui/workbench/palette';
import { PdShellFrame, provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideContributions, provideShellContext, provideShellVersionCheck } from '@decryp7/paddock-ui/shell';
import { provideWebviewLog } from '@decryp7/paddock-ui/shell/diagnostics';
import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { NOTIFICATION_SOURCES, OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend, windowReady } from '@decryp7/paddock-ui/shell/startup';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage } from '@decryp7/paddock-ui/shell/i18n';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { LiveStore } from '../core/live.store';
import { Toasts } from '../core/toasts';
import { settingsPageOf } from '../core/navigation';
import { WINDOW_CONTROL } from '../core/window-control';
import { PITWALL_APP_QUIT, QuitFlow } from '../features/quit/quit-flow';
import { ServiceConsentPrompt } from '../features/service/service-consent';
import { ServiceStore } from '../features/service/service.store';
import { provideIcons } from '../shared/icons';
import { providePitwallAppearance } from './appearance';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIAGNOSTICS_CONTRIBUTIONS } from './contributions';
import { aboutBackend, notifyBackends, settingsBackend, updatesBackend } from './engines';
import { PitwallIcon } from './icon';
import { tauriBridge, tauriIpc } from './ipc';
import type { PitwallContext } from './context';

/** The whole window is the pages frame: pages, status items, menus and dialogs come from the contributions. */
@Component({
  selector: 'pw-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame, PdCommandPalette],
  template: `<pd-shell-frame /><pd-command-palette />`,
  styles: `:host { display: block; height: 100vh; }`,
})
export class PitwallShell {}

/**
 * What the main window starts in the background once it exists (never before the first paint): the live picture of the
 * server and its one subscription, the first-run service question, the "Ask each time" quit question the app may ask for,
 * and the screens the tray popover asks for.
 */
export function startMainWindow(): void {
  const live = inject(LiveStore);
  void live.load();
  live.connect();
  const service = inject(ServiceStore);
  const consent = inject(ServiceConsentPrompt);
  void service.refresh();
  effect(() => {
    if (service.askConsent()) void consent.ask();
  });
  const destroy = inject(DestroyRef);
  destroy.onDestroy(inject(QuitFlow).watch());
  const router = inject(Router);
  const openSettings = inject(OPEN_SETTINGS);
  destroy.onDestroy(inject(WINDOW_CONTROL).onNavigate((route) => goToRoute(router, openSettings, route, destroy)));
}

/**
 * What a route the tray asks for means: a `/settings...` route opens the Settings window at that page (or points the open one
 * there), any other goes to its screen.
 */
export function goToRoute(router: Router, openSettings: (page: string) => void | Promise<void>, route: string, destroy?: DestroyRef): void {
  const page = settingsPageOf(route);
  if (page) void openSettings(page);
  else goWhenRouted(router, route, destroy);
}

/**
 * Goes to `route`, after the window's first navigation when that has not finished yet: a window the tray rebuilt takes
 * its route while it starts, and the start's own navigation (to the URL it was loaded with) must not undo it.
 */
export function goWhenRouted(router: Router, route: string, destroy?: DestroyRef): void {
  if (router.navigated) {
    void router.navigateByUrl(route);
    return;
  }
  const sub = router.events.subscribe((e) => {
    if (!(e instanceof NavigationEnd || e instanceof NavigationCancel || e instanceof NavigationError)) return;
    sub.unsubscribe();
    void router.navigateByUrl(route);
  });
  destroy?.onDestroy(() => sub.unsubscribe());
}

/** Pitwall's main window: the pages frame (ADR 0012) from Pitwall's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const bridge = tauriBridge();
  return bootstrapApplication(PitwallShell, {
    providers: [
      provideBrowserGlobalErrorListeners(),
      provideIcons(),
      // Material's behaviour defaults (outline fields, no ripples, plain toggles, tooltip delay): rule 47, every window.
      providePaddockMaterial(),
      { provide: BACKEND, useFactory: createBackend },
      providePitwallAppearance(),
      // Pitwall's toasts go to the frame's one toast host.
      { provide: NOTIFICATION_SOURCES, multi: true, useExisting: Toasts },
      {
        provide: LANGUAGE_CONFIG,
        useValue: { storage: new LocalStorageLanguageStorage(), systemLanguages: () => navigator.languages, shipped: [], reload: () => location.reload() },
      },
      provideContributions<PitwallContext>(CONTRIBUTIONS),
      // Quit in the app menu (shell-contributed) runs Pitwall's quit flow.
      PITWALL_APP_QUIT,
      // The desktop build asks the shell plugin for its version once (ADR 0012 §3): a different one is a plain error.
      ...(ipc ? [provideShellVersionCheck(ipc.invoke)] : []),
      provideContributions(ABOUT_CONTRIBUTIONS),
      provideContributions(DIAGNOSTICS_CONTRIBUTIONS),
      // The page's own errors reach the log (the command is part of every window's contract); the browser build has no log.
      ...(ipc ? [provideWebviewLog({ ipc: () => ({ invoke: (command, args) => ipc.invoke(command, args) }) })] : []),
      provideShellAbout({ about: aboutBackend, updates: updatesBackend, main: true, ready: windowReady }),
      provideContributions(SETTINGS_CONTRIBUTIONS),
      provideShellSettings({ backend: settingsBackend, appearance: { gate: windowReady } }),
      provideSubmenus(),
      // Mod+Shift+P lists the commands, Mod+P goes to a page (the shared palette on the pages frame).
      providePagesPalette({ commonCommands: ['pd.settings.open', 'pw.pause-serving', 'pw.resume-serving', 'pw.release-memory'] }),
      // The app's part of the context is a lazy chunk, loaded when the window is idle.
      provideShellContext<PitwallContext>(() => import('./context').then((m) => m.pitwallContext)),
      provideAppInitializer(startMainWindow),
      // The shell's frame provides the native feel (browser keys, context menu, the Copy menu).
      provideShellPages({
        appName: 'Pitwall',
        icon: PitwallIcon,
        notify: notifyBackends(),
        startup: { backend: () => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend()) },
        ...(ipc ? { nativeMenu: tauriNativeMenuPort(ipc) } : {}),
        ...(bridge ? { dialogs: { io: () => tauriDialogIo(bridge) } } : {}),
      }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
