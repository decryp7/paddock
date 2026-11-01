import { menuModeProviders } from './menu-mode';
import { ChangeDetectionStrategy, Component, provideAppInitializer, inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import type { Routes } from '@angular/router';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { PdCommandPalette, providePagesPalette } from '@decryp7/paddock-ui/workbench/palette';
import { PdShellFrame, provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideContributions, provideShellContext, provideShellVersionCheck } from '@decryp7/paddock-ui/shell';
import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { provideWebviewLog } from '@decryp7/paddock-ui/shell/diagnostics';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend, windowReady } from '@decryp7/paddock-ui/shell/startup';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage } from '@decryp7/paddock-ui/shell/i18n';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { provideIcons } from '../shared/icons';
import { provideNotifierActions } from '../shared/notifier';
import { provideDynoAppearance } from './appearance';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS, DIAGNOSTICS_CONTRIBUTIONS } from './contributions';
import { aboutBackend, jobsBackend, notifyBackends, settingsBackend, updatesBackend } from './engines';
import { DynoIcon } from './icon';
import { tauriBridge, tauriIpc } from './ipc';
import { provideWidgetNavigation } from './navigate';
import { SettingsStore } from '../state/settings.store';
import type { DynoContext } from './context';

/** The routes of the main window that are not sidebar pages (the CPU detail opens from the dashboard). */
export const DETAIL_ROUTES: Routes = [
  { path: 'cpu', title: 'CPU', loadComponent: () => import('../features/cpu/cpu-detail').then((m) => m.CpuDetail) },
];

/** The whole window is the pages frame: pages, status items, menus and dialogs come from the contributions. */
@Component({
  selector: 'dy-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame, PdCommandPalette],
  template: `<pd-shell-frame /><pd-command-palette />`,
  styles: `:host { display: block; height: 100vh; }`,
})
export class DynoShell {}

/** Dyno's main window: the pages frame (ADR 0012) from Dyno's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const bridge = tauriBridge();
  return bootstrapApplication(DynoShell, {
    providers: [
      provideIcons(),
      // Material's behaviour defaults (outline fields, no icon in the toggle's handle, no ripples); the look is css/material.css.
      providePaddockMaterial(),
      { provide: BACKEND, useFactory: createBackend },
      provideDynoAppearance(),
      provideNotifierActions(),
      {
        provide: LANGUAGE_CONFIG,
        useValue: { storage: new LocalStorageLanguageStorage(), systemLanguages: () => navigator.languages, shipped: [], reload: () => location.reload() },
      },
      provideContributions<DynoContext>(CONTRIBUTIONS),
      // The desktop build asks the shell plugin for its version once (ADR 0012 §3): a different one is a plain error.
      ...(ipc ? [provideShellVersionCheck(ipc.invoke)] : []),
      provideContributions(ABOUT_CONTRIBUTIONS),
      provideContributions(DIAGNOSTICS_CONTRIBUTIONS),
      // The page's own errors reach the log (the command is part of every window's contract); the browser build has no log.
      ...(ipc ? [provideWebviewLog({ ipc: () => ({ invoke: (command, args) => ipc.invoke(command, args) }) })] : []),
      provideShellAbout({ about: aboutBackend, updates: updatesBackend, main: true, ready: windowReady }),
      provideWidgetNavigation(),
      provideContributions(SETTINGS_CONTRIBUTIONS),
      provideShellSettings({ backend: settingsBackend, appearance: { gate: windowReady } }),
      ...menuModeProviders(!!ipc, window.location.search),
      provideSubmenus(),
      // Mod+Shift+P lists the commands, Mod+P goes to a page (the shared palette on the pages frame).
      providePagesPalette({ commonCommands: ['pd.settings.open', 'dy.refresh-history', 'dy.rescan-issues', 'dy.capture'] }),
      // The app's part of the context is a lazy chunk, loaded when the window is idle.
      provideShellContext<DynoContext>(() => import('./context').then((m) => m.dynoContext)),
      // Everything starts in the background: the first paint never waits for the backend.
      provideAppInitializer(() => {
        void inject(SettingsStore).load();
      }),
      // The shell's frame already provides the native feel (browser keys, context menu, the Copy menu).
      provideShellPages({
        appName: 'Dyno',
        icon: DynoIcon,
        routes: DETAIL_ROUTES,
        notify: notifyBackends(),
        jobs: { backend: jobsBackend, stillWorkingAfterMs: 10_000 },
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
