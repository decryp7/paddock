import { ChangeDetectionStrategy, Component } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { Tray } from '../features/tray/tray';
import { provideIcons } from '../shared/icons';
import { provideDynoAppearance } from './appearance';

@Component({
  selector: 'dy-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Tray],
  template: `<dy-tray />`,
})
class TrayRoot {}

/**
 * The tray popover window (`tray` and `tray-<item>`, rule 34: its own bootstrap with a small provider set): no pages, no
 * start-up contract (the watchdog does not watch popovers) and only commands open to the tray class. The shell places and
 * fits the window.
 */
export function bootTrayWindow(): Promise<unknown> {
  document.documentElement.classList.add('pd-tray');
  return bootstrapApplication(TrayRoot, { providers: [provideIcons(), provideDynoAppearance(), { provide: BACKEND, useFactory: createBackend }] });
}
