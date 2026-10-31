import { ChangeDetectionStrategy, Component } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { TrayPage } from '../features/tray/tray-page';
import { provideIcons } from '../shared/icons';
import { provideSpotterAppearance } from './appearance';

@Component({
  selector: 'sp-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [TrayPage],
  template: `<sp-tray-page />`,
})
class TrayRoot {}

/**
 * The tray popover window (`tray`, rule 34: its own bootstrap with a small provider set): no pages, no start-up contract
 * (the watchdog does not watch popovers) and only commands open to the tray class. The shell places and fits the window.
 */
export function bootTrayWindow(): Promise<unknown> {
  document.documentElement.classList.add('pd-tray');
  return bootstrapApplication(TrayRoot, { providers: [provideIcons(), providePaddockMaterial(), provideSpotterAppearance(), { provide: BACKEND, useFactory: createBackend }] });
}
