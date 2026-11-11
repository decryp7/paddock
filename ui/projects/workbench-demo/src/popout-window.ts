import { demoAccent } from './app/params';
import { bootstrapApplication } from '@angular/platform-browser';
import { inTauri } from '@decryp7/paddock-ui/ipc';
import { provideAppearance } from '@decryp7/paddock-ui/shell';
import { provideShellContext, provideShellCore } from '@decryp7/paddock-ui/shell/contributions';
import { MockStartupBackend, provideStartupBackend, provideStartupContract, reportBootstrapFailure, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { provideWorkbenchContributions } from '@decryp7/paddock-ui/workbench';
import { browserWindowChrome, mountPopoutWindowHost, parsePopoutRoute, PdPopoutShell, providePopoutWindow, tauriPanelsIo, tauriWindowChrome } from '@decryp7/paddock-ui/workbench/panels';
import type { WorkbenchDemoContext } from './app/context';
import { desktopIpc, onMac, panelsBridge, windowBridge } from './app/desktop';
import { DEMO_CONTRIBUTIONS } from './app/demo.contributions';
import { InPageDesktop } from '@decryp7/paddock-ui/workbench/panels';

/**
 * A pop-out window of the demo (rule 34: its own bootstrap and small provider set): the pop-out shell, the demo's
 * contributions (so the registry knows the view and the shared state) and the window's own start-up contract. None of the
 * host-only services (the dock, the Explorer, the palette).
 */
export function bootPopoutWindow(): Promise<unknown> {
  const route = parsePopoutRoute(location.hash);
  // index.html carries the main window's root; a pop-out window boots the pop-out shell in its place.
  mountPopoutWindowHost();
  const desktop = inTauri();
  const ipc = desktop ? desktopIpc() : null;
  const bridge = desktop ? panelsBridge() : null;
  const win = desktop ? windowBridge() : null;
  const io = bridge ? tauriPanelsIo(bridge) : new InPageDesktop().window(route?.label ?? 'p-main-1');
  return bootstrapApplication(PdPopoutShell, {
    providers: [
      provideShellCore(),
      provideAppearance({ accent: demoAccent() }),
      provideWorkbenchContributions<WorkbenchDemoContext>(DEMO_CONTRIBUTIONS),
      provideShellContext<WorkbenchDemoContext>(() => import('./app/context').then((m) => m.workbenchDemoContext)),
      provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())),
      provideStartupContract(),
      providePopoutWindow(io, { chrome: win ? tauriWindowChrome(win, onMac()) : browserWindowChrome() }),
    ],
  }).catch((e: unknown) => {
    console.error(e);
    reportBootstrapFailure(e);
  });
}

