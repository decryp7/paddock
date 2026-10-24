import { inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { applyStoredAppearance, defineCommand, msg, provideAppearance, provideShellContext } from '@decryp7/paddock-ui/shell';
import { MockStartupBackend, reportBootstrapFailure } from '@decryp7/paddock-ui/shell/startup';
import { provideLiveContributions, provideWorkbenchContributions, WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import { BrowserDockLayoutStore, provideWorkbenchDock } from '@decryp7/paddock-ui/workbench/dock';
import { provideShellWorkbench } from '@decryp7/paddock-ui/workbench/frame';
import { DEMO_CONTRIBUTIONS } from './app/demo.contributions';
import type { WorkbenchDemoContext } from './app/context';
import { DemoHealth } from './app/demo-health';
import { DemoJobs } from './app/demo-jobs';
import { WorkbenchDemo } from './app/root';

/*
 * The workbench reference app (ADR 0013 §4; plan W4, W8): every workbench contribution kind against mock backends, in the
 * browser. It is the first consumer of every workbench change and the starter for Pacenotes and Sighter.
 */
applyStoredAppearance(document.documentElement, 'graphite');

bootstrapApplication(WorkbenchDemo, {
  providers: [
    provideAppearance({ accent: 'graphite' }),
    provideWorkbenchContributions<WorkbenchDemoContext>(DEMO_CONTRIBUTIONS),
    provideLiveContributions((injector) => ({
      commands: [
        defineCommand({ id: 'wd.p.understand', title: msg('wd.cmd.p.understand', 'Perspective: Understand'), run: () => void injector.get(WORKBENCH_PORTS).applyPerspective('wd.understand') }),
        defineCommand({ id: 'wd.p.review', title: msg('wd.cmd.p.review', 'Perspective: Review'), run: () => void injector.get(WORKBENCH_PORTS).applyPerspective('wd.review') }),
      ],
    })),
    provideShellContext<WorkbenchDemoContext>(() => import('./app/context').then((m) => m.workbenchDemoContext)),
    provideShellWorkbench({
      appName: 'Workbench demo',
      notify: { health: () => inject(DemoHealth).health, notifications: () => inject(DemoHealth).centre },
      jobs: { backend: () => inject(DemoJobs).backend, stillWorkingAfterMs: 1_500 },
      startup: { backend: () => new MockStartupBackend() },
      palette: { commonCommands: ['wd.open-readme', 'wd.reindex'] },
    }),
    provideWorkbenchDock({ store: new BrowserDockLayoutStore(), storageKey: 'wd' }),
  ],
}).catch((e: unknown) => {
  console.error(e);
  reportBootstrapFailure(e);
});
