import { ChangeDetectionStrategy, Component } from '@angular/core';

/** The diagnostics bundle (a stub the page's owner replaces). */
@Component({
  selector: 'dy-diagnostics-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><header class="page-head"><div class="titles"><h1>Diagnostics bundle</h1><p>A redacted zip for a bug report.</p></div></header></div>`,
})
export class DiagnosticsPage {}
