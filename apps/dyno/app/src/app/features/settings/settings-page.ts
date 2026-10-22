import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-settings-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>SettingsPage</h1></div>`,
})
export class SettingsPage {}
