import { ChangeDetectionStrategy, Component, inject, provideAppInitializer } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { LiveStore } from '../core/live.store';
import { TrayPopover } from '../features/tray/tray-popover';
import { provideIcons } from '../shared/icons';
import { providePitwallAppearance } from './appearance';

@Component({
  selector: 'pw-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TrayPopover],
  template: `<pw-tray-popover />`,
})
class TrayRoot {}

/**
 * The tray popover window (`#/tray`, rule 34: its own bootstrap with a small provider set): no pages, no start-up
 * contract (the watchdog does not watch popovers) and only commands open to the tray class (`overview`, `set_serving`,
 * `release_memory`, the pairings and the update notices), so it reads the overview alone and follows the live feed.
 */
export function bootTrayWindow(): Promise<unknown> {
  document.documentElement.classList.add('pd-tray');
  return bootstrapApplication(TrayRoot, {
    providers: [
      provideIcons(),
      providePaddockMaterial(),
      providePitwallAppearance(),
      { provide: BACKEND, useFactory: createBackend },
      provideAppInitializer(() => {
        const live = inject(LiveStore);
        void live.loadOverview();
        live.connect();
      }),
    ],
  });
}
