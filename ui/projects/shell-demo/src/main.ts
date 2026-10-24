import { inject, signal } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { provideContributions, provideShellContext, SHELL_PORTS, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { JOBS_CONTRIBUTIONS, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import {
  AFTER_READY,
  ipcBootErrorSender,
  MockStartupBackend,
  provideStartupBackend,
  provideStartupContract,
  provideUiCheckpoint,
  reportBootstrapFailure,
  startBootErrorForwarding,
  TauriStartupBackend,
  WindowRecovery,
  type ReadyReply,
} from '@decryp7/paddock-ui/shell/startup';
import { provideKeyboardShortcuts, provideMenus, provideNativeMenu, provideSubmenus, tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { DEMO_CONTRIBUTIONS, DEMO_SUBMENUS } from './app/demo.contributions';
import type { DemoContext } from './app/demo-context';
import { tauriIpc } from './app/demo-ipc';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoNote } from './app/demo-note';
import { DemoNotices } from './app/demo-notices';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
import type * as shellDialogs from '@decryp7/paddock-ui/shell/dialogs';
import type * as shellI18n from '@decryp7/paddock-ui/shell/i18n';
import type * as shellNotify from '@decryp7/paddock-ui/shell/notify';
import type * as shellPages from '@decryp7/paddock-ui/shell/pages';
import type * as shellSettings from '@decryp7/paddock-ui/shell/settings';
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

// Errors before the window is ready reach the app's log (ADR 0058): a window that never starts leaves its cause there.
const ipc = tauriIpc();
if (ipc) startBootErrorForwarding(ipcBootErrorSender(ipc));

// The window has reported ready (the start-up contract's reply): the native menu bar is built after it (plan S13).
const windowReady = signal(false);

applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
bootstrapApplication(ShellDemo, {
  providers: [
    provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
    provideContributions(JOBS_CONTRIBUTIONS),
    // Menus and keys (plan S13): one model for the in-app menu bar, context menus and (macOS app) the native menu bar.
    provideMenus({ appName: 'Shell demo' }),
    provideSubmenus<DemoContext>(...DEMO_SUBMENUS),
    provideKeyboardShortcuts(),
    provideNativeMenu(ipc ? { port: tauriNativeMenuPort(ipc), gate: () => windowReady.asReadonly() } : {}),
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
    {
      provide: SHELL_PORTS,
      useFactory: (): ShellPorts => {
        const notices = inject(DemoNotices);
        return {
          showPage: (id) => notices.add({ level: 'info', message: `Page ${id} opens in the pages frame (plan S14)` }),
          openDialog: (id) => (notices.add({ level: 'info', message: `Dialog ${id} opens as a window (plan S6)` }), Promise.resolve(undefined)),
          notify: (n) => notices.add(n),
        };
      },
    },
  ],
}).catch((e: unknown) => {
  console.error(e);
  reportBootstrapFailure(e);
});
