import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Stands in for the pages, status items, settings pages and dialogs of the demo until their packages render them. */
@Component({
  selector: 'sd-placeholder',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p>Rendered by a later work package.</p>`,
})
export class Placeholder {}
