import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-tray',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>Tray</h1></div>`,
})
export class Tray {}
