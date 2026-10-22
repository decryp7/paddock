import { ChangeDetectionStrategy, Component } from '@angular/core';

/** CPU taming and profiles (a stub the page's owner replaces). */
@Component({
  selector: 'dy-rules-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><header class="page-head"><div class="titles"><h1>CPU taming</h1><p>Rules that lower a runaway app, per profile.</p></div></header></div>`,
})
export class RulesPage {}
