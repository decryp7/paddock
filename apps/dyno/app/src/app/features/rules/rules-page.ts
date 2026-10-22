import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-rules-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>RulesPage</h1></div>`,
})
export class RulesPage {}
