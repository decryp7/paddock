import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { SuEmptyState, SuErrorState, SuJob, SuSkeleton, type SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobsStore } from './jobs.store';

/**
 * The Jobs list (plan §5.8: the pages frame shows it in a status-bar popover, the workbench as a view): every job with its
 * phase, count and Cancel, "Still working" for a job that went quiet, an inline error with Retry when the list could not
 * be read, and an empty state. Presentational over `JobsStore`.
 */
@Component({
  selector: 'pd-jobs-list',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuJob, SuEmptyState, SuErrorState, SuSkeleton],
  template: `
    @if (store.requestError(); as cause) {
      <pd-error-state heading="The jobs could not be read" [cause]="cause" [busy]="store.isLoading()" (retry)="store.load()" />
    }
    @if (store.list().length) {
      <ul class="jobs" [attr.aria-label]="label()">
        @for (job of store.list(); track job.id) {
          <li>
            <pd-job [job]="job" [stalled]="store.stalledIds().has(job.id)" [elapsedMs]="elapsed(job)" (cancel)="store.cancel($event)" />
          </li>
        }
      </ul>
    } @else if (store.isLoading()) {
      <div class="loading" aria-busy="true" [attr.aria-label]="label()">
        <pd-skeleton shape="block" />
      </div>
    } @else if (!store.requestError()) {
      <pd-empty-state heading="No jobs" description="Long operations show their progress here." />
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .jobs { display: flex; flex-direction: column; gap: var(--pd-space-2); margin: 0; padding: 0; list-style: none; }
    .jobs li { min-width: 0; }
    .loading { padding: var(--pd-space-2) 0; }
  `,
})
export class PdJobsList {
  protected readonly store = inject(JobsStore);
  /** The list's accessible name. */
  readonly label = input('Jobs');

  /** A running job's time now: its last snapshot's plus the time since it arrived (so a quiet step still counts up). */
  protected elapsed(job: SuJobSnapshot): number {
    if (job.state !== 'running' && job.state !== 'queued') return job.elapsedMs;
    const heard = this.store.heardAt()[job.id];
    return job.elapsedMs + (heard === undefined ? 0 : Math.max(0, this.store.now() - heard));
  }
}
