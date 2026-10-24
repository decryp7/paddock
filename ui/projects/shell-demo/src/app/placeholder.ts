import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Stands in for the settings page of the demo until plan S10 renders it. */
@Component({
  selector: 'sd-placeholder',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p>Rendered by a later work package.</p>`,
})
export class Placeholder {}
