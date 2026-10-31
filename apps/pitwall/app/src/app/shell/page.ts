import { ChangeDetectionStrategy, Component, inject, signal, type Type } from '@angular/core';
import { NgComponentOutlet } from '@angular/common';
import { ActivatedRoute } from '@angular/router';
import { PAGE_DATA } from '@decryp7/paddock-ui/shell/pages';
import { JobsPanel } from '../features/jobs/jobs-panel';
import { ServiceNotices } from '../features/service/service-notices';
import { NAV, type NavItem } from './nav';

/** The nav entry of page id `pw.<id>`, if it is one of Pitwall's. */
export function navItemOf(pageId: unknown): NavItem | undefined {
  return NAV.find((n) => `pw.${n.id}` === pageId);
}

/**
 * Every Pitwall page as the pages frame shows it: what the user must know whatever page is open (the background service's
 * notices, the running jobs with their progress and Cancel) above the page itself. The page's own chunk is already loaded
 * when this is created (the page's `load` waits for it), so the frame's skeleton and Try again cover its loading.
 */
@Component({
  selector: 'pw-page',
  imports: [NgComponentOutlet, ServiceNotices, JobsPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="chrome">
      <pw-service-notices />
      <pw-jobs-panel />
    </div>
    @if (page(); as p) {
      <ng-container *ngComponentOutlet="p" />
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .chrome { display: grid; gap: var(--pd-space-2); padding: var(--pd-space-4) var(--pd-space-6) 0; }
    /* Nothing to say: no space either. */
    .chrome:not(:has(.callout, section)) { display: none; }
  `,
})
export class PitwallPage {
  protected readonly page = signal<Type<unknown> | null>(null);

  constructor() {
    const item = navItemOf(inject(ActivatedRoute).snapshot.data[PAGE_DATA]);
    void item?.load().then((t) => this.page.set(t));
  }
}
