import { inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { provideContributions, provideShellContext } from '@decryp7/paddock-ui/shell';
import { provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { PAGES_WINDOW_READY, provideShellPages } from '@decryp7/paddock-ui/shell/pages';
import { provideShellSettings, SETTINGS_CONTRIBUTIONS } from '@decryp7/paddock-ui/shell/settings';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from './app/demo.contributions';
import type { DemoContext } from './app/demo-context';
import { DemoIcon } from './app/demo-icon';
import { tauriIpc } from './app/demo-ipc';
import { DemoHealthEngine } from './app/demo-health';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoNote } from './app/demo-note';
import { demoSettingsBackend } from './app/demo-settings';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
import type * as shellI18n from '@decryp7/paddock-ui/shell/i18n';
// shell/store is used through shell/jobs and shell/notify (their stores take its withRequestStatus).
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

/** The demo's main window: the pages frame (plan S14) from the demo's contributions. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
  return bootstrapApplication(ShellDemo, {
    providers: [
      provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
      // Settings (plan S10): Settings… (Mod+,) and the shell's Appearance page, Open Settings for notices and repairs, and
      // the window's look on the Appearance settings once the window is ready (start-up sends only the contract's calls).
      provideContributions(SETTINGS_CONTRIBUTIONS),
      provideShellSettings({ backend: demoSettingsBackend, appearance: { gate: () => inject(PAGES_WINDOW_READY).asReadonly() } }),
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
