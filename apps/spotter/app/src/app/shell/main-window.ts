import { ChangeDetectionStrategy, Component, inject, provideAppInitializer } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { PdShellFrame, provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideContributions, provideShellContext, provideShellVersionCheck } from '@decryp7/paddock-ui/shell';
import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend, windowReady } from '@decryp7/paddock-ui/shell/startup';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage } from '@decryp7/paddock-ui/shell/i18n';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { applyWindowChrome } from '../core/window-chrome';
import { platformFromUserAgent } from '../core/tauri-backend';
import { provideNativeFeel } from '../core/native-feel';
import { provideIcons } from '../shared/icons';
import { provideSpotterAppearance } from './appearance';
import { ABOUT_CONTRIBUTIONS, CONTRIBUTIONS } from './contributions';
import { aboutBackend, jobsBackend, notifyBackends, settingsBackend, updatesBackend } from './engines';
import { SpotterIcon } from './icon';
import { tauriBridge, tauriIpc } from './ipc';
import { WatchesStore } from '../state/watches.store';
import { SettingsStore } from '../state/settings.store';
import type { SpotterContext } from './context';
import { Routes } from '@angular/router';

/** The detail routes of the Watches page, matched before the pages. */
export const DETAIL_ROUTES: Routes = [
  { path: 'watches/new', title: 'Add watch', loadComponent: () => import('../features/editor/editor-page').then((m) => m.EditorPage) },
  { path: 'watches/:id/edit', title: 'Edit watch', loadComponent: () => import('../features/editor/editor-page').then((m) => m.EditorPage) },
  { path: 'watches/:id', title: 'Watch', loadComponent: () => import('../features/watch-detail/watch-detail-page').then((m) => m.WatchDetailPage) },
];

/** The whole window is the pages frame: pages, status items, menus and dialogs come from the contributions. */
@Component({
  selector: 'sp-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame],
  template: `<pd-shell-frame />`,
  styles: `:host { display: block; height: 100vh; }`,
})
export class SpotterShell {}

/** Spotter's main window: the pages frame (ADR 0012) from Spotter's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const bridge = tauriBridge();
  return bootstrapApplication(SpotterShell, {
    providers: [
      provideIcons(),
      provideNativeFeel(),
      { provide: BACKEND, useFactory: createBackend },
      provideSpotterAppearance(),
      {
        provide: LANGUAGE_CONFIG,
        useValue: { storage: new LocalStorageLanguageStorage(), systemLanguages: () => navigator.languages, shipped: [], reload: () => location.reload() },
      },
      provideContributions<SpotterContext>(CONTRIBUTIONS),
      // The desktop build asks the shell plugin for its version once (ADR 0012 §3): a different one is a plain error.
      ...(ipc ? [provideShellVersionCheck(ipc.invoke)] : []),
      provideContributions(ABOUT_CONTRIBUTIONS),
      provideShellAbout({ about: aboutBackend, updates: updatesBackend, main: true, ready: windowReady }),
      provideContributions(SETTINGS_CONTRIBUTIONS),
      provideShellSettings({ backend: settingsBackend, appearance: { gate: windowReady } }),
      provideSubmenus(),
      // The app's part of the context is a lazy chunk, loaded when the window is idle.
      provideShellContext<SpotterContext>(() => import('./context').then((m) => m.spotterContext)),
      // Everything starts in the background: the first paint never waits for the backend.
      provideAppInitializer(() => {
        applyWindowChrome(document.documentElement, platformFromUserAgent(navigator.userAgent));
        inject(WatchesStore).start();
        void inject(SettingsStore).loadSettings();
      }),
      provideShellPages({
        appName: 'Spotter',
        icon: SpotterIcon,
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

