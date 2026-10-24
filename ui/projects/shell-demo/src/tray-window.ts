import { ChangeDetectionStrategy, Component } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { tauriIpc } from './app/demo-ipc';
import { TRAY_IPC, TrayPopover, type TrayIpc, type TrayState } from './app/tray-popover';

/** The browser build's stand-in (e2e and screenshots): a calm app, every action accepted, no events. */
const BROWSER_STATE: TrayState = { problems: [], job: null };
const browserIpc: TrayIpc = {
  invoke: async <T,>(command: string) => (command === 'demo_tray_state' ? BROWSER_STATE : null) as T,
  listen: async () => () => undefined,
};

@Component({
  selector: 'sd-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TrayPopover],
  template: `<sd-tray-popover />`,
})
class TrayRoot {}

/**
 * The tray popover window (`tray`, rule 34: its own bootstrap with a small provider set): no pages, no start-up contract
 * (the watchdog does not watch popovers) and only commands open to the tray class.
 */
export function bootTrayWindow(): Promise<unknown> {
  const ipc = tauriIpc();
  const tray: TrayIpc = ipc ? { invoke: ipc.invoke, listen: (event, handler) => ipc.listen(event, () => handler()) } : browserIpc;
  return bootstrapApplication(TrayRoot, { providers: [{ provide: TRAY_IPC, useValue: tray }] });
}
