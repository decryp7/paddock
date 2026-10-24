import { inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { provideContributions, provideShellContext, provideShellVersionCheck } from '@decryp7/paddock-ui/shell';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend, windowReady } from '@decryp7/paddock-ui/shell/startup';
import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { OS_NOTIFICATIONS_CONTRIBUTIONS, TauriNotificationsBackend, TauriServiceHealthBackend } from '@decryp7/paddock-ui/shell/notify';
import { provideWebviewLog } from '@decryp7/paddock-ui/shell/diagnostics';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS, SHELL_ABOUT_CONTRIBUTIONS, SHELL_DIAGNOSTICS_CONTRIBUTIONS } from './app/demo.contributions';
import { demoAboutBackend, demoUpdatesBackend } from './app/demo-about';
import type { DemoContext } from './app/demo-context';
import { DemoIcon } from './app/demo-icon';
import { tauriBridge, tauriIpc } from './app/demo-ipc';
import { APP_NAME } from './app/demo-name';
import { DemoHealthEngine } from './app/demo-health';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoNote } from './app/demo-note';
import { provideDemoAppearance } from './app/demo-appearance';
import { relaunchApp } from './app/language.providers';
import { LANGUAGE_CONFIG, LocalStorageLanguageStorage, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import { demoSettingsBackend } from './app/demo-settings';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
// shell/store is used through shell/jobs and shell/notify (their stores take its withRequestStatus).
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

/** The demo's main window: the pages frame (plan S14) from the demo's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const bridge = tauriBridge();
  return bootstrapApplication(ShellDemo, {
    providers: [
      // Theme and language (plan S15): the appearance store, over the Appearance settings (provideShellSettings below);
      // the language applies after a reload.
      provideDemoAppearance(),
      {
        provide: LANGUAGE_CONFIG,
        useValue: {
          storage: new LocalStorageLanguageStorage(),
          systemLanguages: () => navigator.languages,
          shipped: [PSEUDO_LOCALE],
          reload: relaunchApp,
        },
      },
      provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
      // The desktop build asks the shell plugin for its version once (ADR 0012 §3): a different one is a plain error.
      ...(ipc ? [provideShellVersionCheck(ipc.invoke)] : []),
      // About, Updates and What's New (plan S11): the Help menu's items; the main window announces a found update and
      // opens What's New once after an update.
      provideContributions(SHELL_ABOUT_CONTRIBUTIONS),
      provideShellAbout({ about: demoAboutBackend, updates: demoUpdatesBackend, main: true, ready: windowReady }),
      // Report a problem (plan X2): Help > Report a problem…, the Logging settings page, and the page's own errors in the
      // engine's log (desktop build only: the browser build has no log to send them to).
      provideContributions(SHELL_DIAGNOSTICS_CONTRIBUTIONS),
      provideContributions(OS_NOTIFICATIONS_CONTRIBUTIONS),
      ...(ipc ? [provideWebviewLog({ ipc: () => ({ invoke: (command, args) => ipc.invoke(command, args) }) })] : []),
      // Settings (plan S10): Settings… (Mod+,) and the shell's Appearance page, Open Settings for notices and repairs, and
      // the Appearance settings as the source of the window's look, read once the window is ready (start-up sends only
      // the contract's calls).
      provideContributions(SETTINGS_CONTRIBUTIONS),
      // The window's look: its appearance store (the one writer) over the Appearance settings (the one source), read
      // once the window reported ready (start-up sends only the contract's calls).
      provideShellSettings({ backend: demoSettingsBackend, appearance: { gate: windowReady } }),
      provideSubmenus<DemoContext>(...DEMO_SUBMENUS),
      // The app's part of the context is a lazy chunk, loaded when the window is idle (plan §9).
      provideShellContext<DemoContext>(() => import('./app/demo-context').then((m) => m.demoContext)),
      // The whole window from the contributions: pages, status bar, menus and keys, notifications and service health,
      // jobs ("Still working" after 1.5 s without events), the start-up contract keeping the note across a recovery
      // reload, and dialogs. In the desktop app every part is the shell plugin's (dialogs are real windows); in the
      // browser, the demo's in-memory stand-ins (dialogs in the page).
      provideShellPages({
        appName: APP_NAME,
        icon: DemoIcon,
        notify: ipc
          ? { health: () => new TauriServiceHealthBackend(ipc), notifications: () => new TauriNotificationsBackend(ipc) }
          : { health: () => inject(DemoHealthEngine).health, notifications: () => inject(DemoHealthEngine).centre },
        jobs: { backend: () => (ipc ? new TauriJobsBackend(ipc) : inject(DemoJobsEngine).backend), stillWorkingAfterMs: 1_500 },
        startup: {
          backend: () => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend()),
          checkpoint: () => inject(DemoNote).checkpoint,
        },
        ...(ipc ? { nativeMenu: tauriNativeMenuPort(ipc) } : {}),
        ...(bridge ? { dialogs: { io: () => tauriDialogIo(bridge) } } : {}),
      }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
