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
      <pd-error-state [heading]="t.readFailed" [cause]="cause" [busy]="store.isLoading()" (retry)="store.load()" />
    }
    @if (store.list().length) {
      <ul class="pd-jobs" [class.pd-flat]="flat()" [attr.aria-label]="label()">
        @for (job of store.list(); track job.id) {
          <li>
            <pd-job [flat]="flat()" [job]="job" [stalled]="store.stalledIds().has(job.id)" [elapsedMs]="elapsed(job)" (cancel)="store.cancel($event)" />
          </li>
        }
      </ul>
    } @else if (store.isLoading()) {
      <div class="pd-loading" aria-busy="true" [attr.aria-label]="label()">
        <pd-skeleton shape="block" />
      </div>
    } @else if (!store.requestError()) {
      <pd-empty-state [heading]="t.emptyHeading" [description]="t.emptyDescription" />
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
    .pd-jobs { display: flex; flex-direction: column; gap: var(--pd-space-2); margin: 0; padding: 0; list-style: none; }
    .pd-jobs li { min-width: 0; }
    /* Flat rows (a popover is the card): a hairline between jobs, no box around each. */
    .pd-jobs.pd-flat { gap: 0; }
    .pd-jobs.pd-flat li + li { border-top: 1px solid var(--pd-hairline); }
    .pd-loading { padding: var(--pd-space-2) 0; }
  `,
})
export class PdJobsList {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    readFailed: $localize`:@@pd.jobs.read-failed:The jobs could not be read`,
    emptyHeading: $localize`:@@pd.jobs.empty.heading:No jobs`,
    emptyDescription: $localize`:@@pd.jobs.empty.description:Long operations show their progress here.`,
  };
  protected readonly store = inject(JobsStore);
  /** The list's accessible name. */
  readonly label = input($localize`:@@pd.jobs.list:Jobs`);
  /** Flat rows instead of one card per job: for a list inside a bordered container (UI rule 17). */
  readonly flat = input(false);

  /** A running job's time now: its last snapshot's plus the time since it arrived (so a quiet step still counts up). */
  protected elapsed(job: SuJobSnapshot): number {
    if (job.state !== 'running' && job.state !== 'queued') return job.elapsedMs;
    const heard = this.store.heardAt()[job.id];
    return job.elapsedMs + (heard === undefined ? 0 : Math.max(0, this.store.now() - heard));
  }
}
