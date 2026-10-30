import { ChangeDetectionStrategy, Component, inject, provideAppInitializer } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { LiveStore } from '../core/live.store';
import { TrayPopover } from '../features/tray/tray-popover';
import { provideIcons } from '../shared/icons';
import { providePitwallAppearance } from './appearance';

/**
 * The backend calls the tray popover may make: each is a command open to the TRAY class in `src-tauri/src/lib.rs`
 * (`overview`, `set_serving`, `release_memory`, `update_check_status`, `update_notices`, `pairings`, `approve_pairing`,
 * `deny_pairing`) or the live event feed (`subscribe`, no command). `tray-window.spec.ts` holds the popover to it.
 */
export const TRAY_BACKEND_CALLS = ['overview', 'subscribe', 'setServing', 'releaseMemory', 'updateCheckStatus', 'updateNotices', 'pairings', 'approvePairing', 'denyPairing'] as const;

/** What the popover window starts with: the overview alone, then the live feed. */
export function startTrayFeed(live: LiveStore): void {
  void live.loadOverview();
  live.connect();
}

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
      provideAppInitializer(() => startTrayFeed(inject(LiveStore))),
    ],
  });
}
