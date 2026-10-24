import { inject, signal } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { provideContributions, provideShellContext, SHELL_PORTS, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { openDialogPort, provideDialogs } from '@decryp7/paddock-ui/shell/dialogs';
import { JOBS_CONTRIBUTIONS, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { provideKeyboardShortcuts, provideMenus, provideNativeMenu, provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { NOTIFY_CONTRIBUTIONS, notifyPort, provideShellNotify } from '@decryp7/paddock-ui/shell/notify';
import {
  AFTER_READY,
  MockStartupBackend,
  provideStartupBackend,
  provideStartupContract,
  provideUiCheckpoint,
  reportBootstrapFailure,
  TauriStartupBackend,
  WindowRecovery,
  type ReadyReply,
} from '@decryp7/paddock-ui/shell/startup';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from './app/demo.contributions';
import type { DemoContext } from './app/demo-context';
import { tauriIpc } from './app/demo-ipc';
import { DemoHealthEngine } from './app/demo-health';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoNote } from './app/demo-note';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
import type * as shellI18n from '@decryp7/paddock-ui/shell/i18n';
import type * as shellPages from '@decryp7/paddock-ui/shell/pages';
import type * as shellSettings from '@decryp7/paddock-ui/shell/settings';
// shell/store is used through shell/jobs and shell/notify (their stores take its withRequestStatus).
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

/** The demo's main window. */
export function bootMainWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  // The window has reported ready (the start-up contract's reply): the native menu bar is built after it (plan S13).
  const windowReady = signal(false);
  applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
  return bootstrapApplication(ShellDemo, {
    providers: [
      provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
      provideContributions(JOBS_CONTRIBUTIONS),
      provideContributions(NOTIFY_CONTRIBUTIONS),
      // Menus and keys (plan S13): one model for the in-app menu bar, context menus and (macOS app) the native menu bar.
      provideMenus({ appName: 'Shell demo' }),
      provideSubmenus<DemoContext>(...DEMO_SUBMENUS),
      provideKeyboardShortcuts(),
      provideNativeMenu(ipc ? { port: tauriNativeMenuPort(ipc), gate: () => windowReady.asReadonly() } : {}),
      // Notifications and service health of this window (plan S4) over the demo's in-memory paddock-health: the host is
      // mounted over the window; SHELL_PORTS below sends notices to it.
      provideShellNotify({ health: () => inject(DemoHealthEngine).health, notifications: () => inject(DemoHealthEngine).centre, ports: false }),
      // The jobs of this window (plan S9) over the demo's simulated engine; "Still working" after 1.5 s without events.
      provideShellJobs({ backend: () => inject(DemoJobsEngine).backend, stillWorkingAfterMs: 1_500 }),
      // The start-up contract (plan S7): ready after the first paint, liveness pings, the note kept across a recovery reload.
      provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())),
      provideStartupContract(),
      provideUiCheckpoint(() => inject(DemoNote).checkpoint),
      {
        provide: AFTER_READY,
        useFactory: () => {
          const recovery = inject(WindowRecovery);
          return (reply: ReadyReply | null) => {
            void recovery.afterReady(reply);
            windowReady.set(true);
          };
        },
      },
      // The app's part of the context is a lazy chunk, loaded when the window is idle (plan §9).
      provideShellContext<DemoContext>(() => import('./app/demo-context').then((m) => m.demoContext)),
      // The browser build shows dialogs in the page; the desktop build passes `{ io: () => tauriDialogIo(bridge) }` (plan S6).
      provideDialogs(),
      {
        provide: SHELL_PORTS,
        useFactory: (): ShellPorts => {
          const notify = notifyPort();
          return {
            showPage: (id) => notify({ level: 'info', message: `Page ${id} opens in the pages frame (plan S14)` }),
            openDialog: openDialogPort(),
            notify,
          };
        },
      },
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
