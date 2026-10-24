import { inject, provideEnvironmentInitializer } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { provideContributions, provideShellContext } from '@decryp7/paddock-ui/shell';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { PAGES_WINDOW_READY, provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideShellSettings, settingsAppearanceBackend, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from './app/demo.contributions';
import type { DemoContext } from './app/demo-context';
import { DemoIcon } from './app/demo-icon';
import { tauriIpc } from './app/demo-ipc';
import { DemoHealthEngine } from './app/demo-health';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoNote } from './app/demo-note';
import { demoAppearanceLocal, provideDemoAppearance } from './app/demo-appearance';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import { DemoSettingsEngine, demoSettingsBackend } from './app/demo-settings';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
// shell/store is used through shell/jobs and shell/notify (their stores take its withRequestStatus).
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

/** The demo's main window: the pages frame (plan S14) from the demo's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  return bootstrapApplication(ShellDemo, {
    providers: [
      // Theme and language (plan S15): the appearance follows every other window and keeps theme and density in the
      // Appearance settings once the window is ready; the language applies after a reload.
      provideDemoAppearance(settingsAppearanceBackend({ local: demoAppearanceLocal, gate: () => inject(PAGES_WINDOW_READY).asReadonly() })),
      {
        provide: LANGUAGE_CONFIG,
        useValue: {
          storage: new LocalStorageLanguageStorage(),
          systemLanguages: () => navigator.languages,
          shipped: [PSEUDO_LOCALE],
          reload: () => location.reload(),
        },
      },
      // Plays paddock-settings' start-up in the browser build: the settings are read (and a damaged file reported) before
      // any window reads its notifications, as `paddock_settings::tauri::manage` does in the app's setup.
      ...(ipc ? [] : [provideEnvironmentInitializer(() => void inject(DemoSettingsEngine))]),
      provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
      // Settings (plan S10): Settings… (Mod+,) and the shell's Appearance page, Open Settings for notices and repairs, and
      // the window's look on the Appearance settings once the window is ready (start-up sends only the contract's calls),
      // through the appearance store's backend (above).
      provideContributions(SETTINGS_CONTRIBUTIONS),
      // The look is the appearance store's (plan S15), kept in the Appearance settings by its backend: no second follower.
      provideShellSettings({ backend: demoSettingsBackend, appearance: false }),
      provideSubmenus<DemoContext>(...DEMO_SUBMENUS),
      // The app's part of the context is a lazy chunk, loaded when the window is idle (plan §9).
      provideShellContext<DemoContext>(() => import('./app/demo-context').then((m) => m.demoContext)),
      // The whole window from the contributions: pages, status bar, menus and keys, notifications over the demo's
      // in-memory paddock-health, jobs over its simulated engine ("Still working" after 1.5 s without events), the
      // start-up contract keeping the note across a recovery reload, and dialogs in the page (the desktop build passes
      // `dialogs: { io: () => tauriDialogIo(bridge) }`).
      provideShellPages({
        appName: 'Paddock demo',
        icon: DemoIcon,
        notify: { health: () => inject(DemoHealthEngine).health, notifications: () => inject(DemoHealthEngine).centre },
        jobs: { backend: () => inject(DemoJobsEngine).backend, stillWorkingAfterMs: 1_500 },
        startup: {
          backend: () => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend()),
          checkpoint: () => inject(DemoNote).checkpoint,
        },
        ...(ipc ? { nativeMenu: tauriNativeMenuPort(ipc) } : {}),
      }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
