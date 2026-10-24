import { inject } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { defineCommand, msg, provideAppearance, provideShellContext } from '@decryp7/paddock-ui/shell';
import { inTauri } from '@decryp7/paddock-ui/ipc';
import { tauriDialogIo } from '@decryp7/paddock-ui/shell/dialogs';
import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { tauriNativeMenuPort } from '@decryp7/paddock-ui/shell/menu';
import { TauriNotificationsBackend, TauriServiceHealthBackend } from '@decryp7/paddock-ui/shell/notify';
import { MemoryPitwallBackend, providePitwall, TauriPitwallBackend } from '@decryp7/paddock-ui/shell/pitwall';
import { MockStartupBackend, reportBootstrapFailure, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { provideLiveContributions, provideWorkbenchContributions, WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import { ASSISTANT_CONTRIBUTIONS, MemoryAssistantBackend, provideAssistant, TauriAssistantBackend } from '@decryp7/paddock-ui/workbench/assistant';
import { BrowserDockLayoutStore, IpcDockLayoutStore, provideWorkbenchDock } from '@decryp7/paddock-ui/workbench/dock';
import { provideShellWorkbench } from '@decryp7/paddock-ui/workbench/frame';
import { InPageDesktop, providePanelWindows, tauriPanelsIo } from '@decryp7/paddock-ui/workbench/panels';
import type { WorkbenchDemoContext } from './app/context';
import { assistantIpc, desktopIpc, dialogBridge, panelsBridge, pitwallIpc, rawInvoke } from './app/desktop';
import { DemoHealth } from './app/demo-health';
import { DemoJobs } from './app/demo-jobs';
import { DEMO_CONTRIBUTIONS } from './app/demo.contributions';
import { WorkbenchDemo } from './app/root';

/** The demo's main window. In the desktop app every part is the plugins'; in the browser, the demo's mock stand-ins. */
export function bootMainWindow(): Promise<unknown> {
  const desktop = inTauri();
  const ipc = desktop ? desktopIpc() : null;
  const dialogs = desktop ? dialogBridge() : null;
  const panels = desktop ? panelsBridge() : null;
  const raw = desktop ? rawInvoke() : null;
  const assistant = desktop ? assistantIpc() : null;
  const pitwall = desktop ? pitwallIpc() : null;
  return bootstrapApplication(WorkbenchDemo, {
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
        notify: ipc
          ? { health: () => new TauriServiceHealthBackend(ipc), notifications: () => new TauriNotificationsBackend(ipc) }
          : { health: () => inject(DemoHealth).health, notifications: () => inject(DemoHealth).centre },
        jobs: { backend: () => (ipc ? new TauriJobsBackend(ipc) : inject(DemoJobs).backend), stillWorkingAfterMs: 1_500 },
        startup: { backend: () => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend()) },
        palette: { commonCommands: ['wd.open-readme', 'wd.reindex'] },
        ...(ipc ? { nativeMenu: tauriNativeMenuPort(ipc) } : {}),
        ...(dialogs ? { dialogs: { io: () => tauriDialogIo(dialogs) } } : {}),
      }),
      // The dock's layouts: in the engine (`paddock-workbench`, the config folder) on the desktop, the browser's storage otherwise.
      provideWorkbenchDock({ store: raw ? new IpcDockLayoutStore(raw) : new BrowserDockLayoutStore(), storageKey: 'wd' }),
      // Pop-outs: OS windows (`paddock-panels`) on the desktop, simulated windows in the browser (no OS window opens).
      providePanelWindows(panels ? tauriPanelsIo(panels) : new InPageDesktop().window('main')),
      // The assistant pane on Pitwall. Without Pitwall (the browser, or a desktop without it) the pane shows the calm needs-AI state with its fix.
      provideWorkbenchContributions(ASSISTANT_CONTRIBUTIONS),
      provideAssistant({ backend: () => (assistant ? new TauriAssistantBackend(assistant) : new MemoryAssistantBackend()) }),
      providePitwall({
        backend: () =>
          pitwall
            ? new TauriPitwallBackend(pitwall)
            : new MemoryPitwallBackend({ state: { kind: 'notRunning' }, app: 'Workbench demo', builtInAvailable: false }),
      }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}
