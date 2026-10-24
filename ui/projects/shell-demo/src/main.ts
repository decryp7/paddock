import { inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { provideContributions, provideShellContext, SHELL_PORTS, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { JOBS_CONTRIBUTIONS, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { NOTIFY_CONTRIBUTIONS, notifyPort, provideShellNotify } from '@decryp7/paddock-ui/shell/notify';
import { DEMO_CONTRIBUTIONS } from './app/demo.contributions';
import type { DemoContext } from './app/demo-context';
import { DemoJobsEngine } from './app/demo-jobs';
import { DemoHealthEngine } from './app/demo-health';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
import type * as shellDialogs from '@decryp7/paddock-ui/shell/dialogs';
import type * as shellI18n from '@decryp7/paddock-ui/shell/i18n';
import type * as shellMenu from '@decryp7/paddock-ui/shell/menu';
import type * as shellPages from '@decryp7/paddock-ui/shell/pages';
import type * as shellSettings from '@decryp7/paddock-ui/shell/settings';
import type * as shellStartup from '@decryp7/paddock-ui/shell/startup';
// shell/store is used through shell/jobs and shell/notify (their stores take its withRequestStatus).
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
bootstrapApplication(ShellDemo, {
  providers: [
    provideContributions<DemoContext>(DEMO_CONTRIBUTIONS),
    provideContributions(JOBS_CONTRIBUTIONS),
    provideContributions(NOTIFY_CONTRIBUTIONS),
    // Notifications and service health of this window (plan S4) over the demo's in-memory paddock-health: the host is
    // mounted over the window; SHELL_PORTS below sends notices to it.
    provideShellNotify({ health: () => inject(DemoHealthEngine).health, notifications: () => inject(DemoHealthEngine).centre, ports: false }),
    // The jobs of this window (plan S9) over the demo's simulated engine; "Still working" after 1.5 s without events.
    provideShellJobs({ backend: () => inject(DemoJobsEngine).backend, stillWorkingAfterMs: 1_500 }),
    // The app's part of the context is a lazy chunk, loaded when the window is idle (plan §9).
    provideShellContext<DemoContext>(() => import('./app/demo-context').then((m) => m.demoContext)),
    {
      provide: SHELL_PORTS,
      useFactory: (): ShellPorts => {
        const notify = notifyPort();
        return {
          showPage: (id) => notify({ level: 'info', message: `Page ${id} opens in the pages frame (plan S14)` }),
          openDialog: (id) => (notify({ level: 'info', message: `Dialog ${id} opens as a window (plan S6)` }), Promise.resolve(undefined)),
          notify,
        };
      },
    },
  ],
}).catch((e: unknown) => console.error(e));
