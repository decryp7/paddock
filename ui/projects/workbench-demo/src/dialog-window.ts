import { demoAccent } from './app/params';
import { bootstrapApplication } from '@angular/platform-browser';
import { inTauri } from '@decryp7/paddock-ui/ipc';
import { provideAppearance } from '@decryp7/paddock-ui/shell';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { MockStartupBackend, provideStartupBackend, provideStartupContract, TauriStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { InPageDesktop, provideDialogUiSync, tauriPanelsIo } from '@decryp7/paddock-ui/workbench/panels';
import { desktopIpc, dialogBridge, panelsBridge, rawInvoke } from './app/desktop';
import { DIALOG_BASE } from './app/generated/dialog-contracts';
import { BrowserDockLayoutStore, dockLayoutDialogs, IpcDockLayoutStore } from '@decryp7/paddock-ui/workbench/dock';

/**
 * A dialog window of the demo: the dialog shell, the start-up contract (a dialog window reports ready and answers liveness
 * pings, ADR 0058) and the display state it shares with its host (theme, formats). The demo has no dialogs of its own yet;
 * an app adds its dialogs to `dialogs` and its contract file to the shell Builder.
 */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  const desktop = inTauri();
  const ipc = desktop ? desktopIpc() : null;
  const raw = desktop ? rawInvoke() : null;
  const bridge = desktop ? dialogBridge() : null;
  const panels = desktop ? panelsBridge() : null;
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      // The dock's Save layout and Manage layouts (the same store and key as the main window's dock).
      dialogs: dockLayoutDialogs({ store: raw ? new IpcDockLayoutStore(raw) : new BrowserDockLayoutStore(), storageKey: 'wd' }),
      providers: [
        provideAppearance({ accent: demoAccent() }),
        provideStartupBackend(() => (ipc ? new TauriStartupBackend(ipc) : new MockStartupBackend())),
        provideStartupContract(),
        provideDialogUiSync(panels ? tauriPanelsIo(panels) : new InPageDesktop().window(route.label)),
      ],
      route,
      baseCommands: DIALOG_BASE,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
