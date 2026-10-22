import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-cleanup-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>CleanupPage</h1></div>`,
})
export class CleanupPage {}
