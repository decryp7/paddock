import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { DemoRepo } from './repo';

/** The demo's status item at the start of the bar: what the index holds (an app puts its own state here). */
@Component({
  selector: 'wd-index-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="item" data-testid="index-status">{{ count }} files indexed</span>`,
  styles: `.item { display: inline-flex; align-items: center; padding: 0 var(--pd-space-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); white-space: nowrap; }`,
})
export class IndexStatus {
  protected readonly count = inject(DemoRepo).files.length;
}
