import { ChangeDetectionStrategy, Component } from '@angular/core';
import { DemoPage, SECTION_STYLES } from './demo-page';

/** A page offered only while its `when` holds (after the first greeting). */
@Component({
  selector: 'sd-lab-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage],
  template: `
    <sd-page heading="Lab" subtitle="Offered once you said hello; it leaves the sidebar when the greetings are reset.">
      <p class="context" data-testid="lab">The page’s condition holds: there are greetings.</p>
    </sd-page>
  `,
  styles: SECTION_STYLES,
})
export class LabPage {}
