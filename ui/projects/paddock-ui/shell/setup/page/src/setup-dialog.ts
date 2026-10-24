import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdSetupChecklist } from './setup-checklist';

/**
 * The Set-up window (a modeless `tool` dialog): the checklist beside the work, so the user can answer an OS prompt or open
 * Pitwall and come back. The window's own Close (Escape) leaves the steps as they are; they stay in Settings > Set-up and
 * Help > Set-up….
 */
@Component({
  selector: 'pd-setup-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSetupChecklist],
  template: `<pd-setup-checklist />`,
  styles: `:host { display: block; min-width: 0; }`,
})
export class PdSetupDialog {}
