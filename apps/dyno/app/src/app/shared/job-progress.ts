import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { SuJob } from '@decryp7/paddock-ui';
import type { JobProgress } from '../core/job-starter';

/**
 * The progress of one of the engine's long operations (ER-18): the shared job card with the phase's own count, an ETA only when
 * the engine has a trustworthy one, Cancel, and "Still working" when nothing has changed for a while. It never shows a timeout.
 */
@Component({
  selector: 'dy-job-progress',
  imports: [SuJob],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (progress(); as p) {
      <pd-job [job]="p.snapshot" [stalled]="p.stalled" (cancel)="cancel.emit()" />
    }
  `,
  styles: `
    :host { display: block; }
  `,
})
export class JobProgressCard {
  /** The latest status; nothing is drawn until the first one arrives. */
  readonly progress = input<JobProgress | null>(null);
  /** Cancel was pressed. */
  // eslint-disable-next-line @angular-eslint/no-output-native -- the shared job card's own API is `(cancel)`
  readonly cancel = output<void>();
}
