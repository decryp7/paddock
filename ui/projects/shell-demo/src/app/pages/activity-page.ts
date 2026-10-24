import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdJobsList } from '@decryp7/paddock-ui/shell/jobs';
import { DemoPage, SECTION_STYLES } from './demo-page';
import { ActivityVisit } from './activity.providers';

/** A page with providers of its own (loaded with it, ended with it) that hosts the shell's jobs list as page content. */
@Component({
  selector: 'sd-activity-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdJobsList],
  template: `
    <sd-page heading="Activity" subtitle="This page brings its own providers: a new visit service each time it opens.">
      <p class="context" data-testid="activity-visit">Visit {{ visit.number }} of this page</p>
      <section aria-labelledby="sd-jobs">
        <h2 id="sd-jobs">Jobs</h2>
        <pd-jobs-list />
      </section>
    </sd-page>
  `,
  styles: SECTION_STYLES,
})
export class ActivityPage {
  protected readonly visit = inject(ActivityVisit);
}
