import { ChangeDetectionStrategy, Component } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { tauriIpc } from './app/demo-ipc';
import { TRAY_INVOKE, TrayPopover, type TrayInvoke } from './app/tray-popover';
import { APP_NAME } from './app/demo-name';

/** The browser build's stand-in (e2e and screenshots): one window, the shell's version, every action accepted. */
const browserInvoke: TrayInvoke = async <T,>(command: string) => {
  const replies: Record<string, unknown> = { shell_version: { shell: 'browser', app: { id: 'shell-demo', name: APP_NAME, version: 'browser' } }, windows_list: [{ label: 'main' }] };
  return (replies[command] ?? null) as T;
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
  return bootstrapApplication(TrayRoot, {
    providers: [{ provide: TRAY_INVOKE, useValue: ipc ? ipc.invoke : browserInvoke }],
  });
}
