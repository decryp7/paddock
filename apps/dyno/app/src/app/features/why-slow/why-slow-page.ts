import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-why-slow-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>WhySlowPage</h1></div>`,
})
export class WhySlowPage {}
