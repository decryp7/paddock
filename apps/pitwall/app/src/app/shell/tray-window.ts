import { ChangeDetectionStrategy, Component } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { TrayPopover } from '../features/tray/tray-popover';
import { provideIcons } from '../shared/icons';
import { providePitwallAppearance } from './appearance';

/**
 * The backend calls the tray popover may make: each is a command open to the TRAY class in `src-tauri/src/lib.rs`
 * (`popover_view`, `popover_values`, `popover_header`, `popover_shape`, `set_serving`, `release_memory`, `cancel_download`) or the
 * app's popup event (`onPopover`, no command). `tray-window.spec.ts` holds the popover to it.
 */
export const TRAY_BACKEND_CALLS = ['popoverView', 'popoverValues', 'popoverHeader', 'popoverShape', 'onPopover', 'setServing', 'releaseMemory', 'cancelDownload'] as const;

@Component({
  selector: 'pw-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TrayPopover],
  template: `<pw-tray-popover />`,
})
class TrayRoot {}

/**
 * The tray popover window (`#/tray`, rule 34: its own bootstrap with a small provider set): no pages, no start-up
 * contract (the watchdog does not watch popovers) and only commands open to the tray class (the popup's layout, values and
 * header, `set_serving`, `release_memory`, `cancel_download`), so it draws what the app pushes and has no feed of its own.
 */
export function bootTrayWindow(): Promise<unknown> {
  document.documentElement.classList.add('pd-tray');
  return bootstrapApplication(TrayRoot, {
    providers: [
      provideIcons(),
      providePaddockMaterial(),
      providePitwallAppearance(),
      { provide: BACKEND, useFactory: createBackend },
    ],
  });
}
