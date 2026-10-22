import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-processes-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>ProcessesPage</h1></div>`,
})
export class ProcessesPage {}
