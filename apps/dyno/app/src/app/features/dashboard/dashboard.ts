import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-dashboard',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>Dashboard</h1></div>`,
})
export class Dashboard {}
