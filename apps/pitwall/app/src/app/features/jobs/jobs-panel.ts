import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { JobsService, type JobRow } from '../../core/jobs';
import { bytes, int, percent } from '../../shared/format';

/**
 * The background jobs that are running (ER-18, ER-19): title, the phase in words, its own count (a growing count when the
 * total is unknown, never a static 0), Cancel in every phase and "Stop waiting" when Cancel did not take effect. A job that
 * goes quiet says "Still working"; the UI never times it out.
 */
@Component({
  selector: 'pw-jobs-panel',
  imports: [MatButtonModule, MatProgressBarModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (jobs.any()) {
      <section class="jobs" aria-label="Running operations">
        @for (job of jobs.jobs(); track job.id) {
          <div class="job" [attr.data-job]="job.id">
            <div class="head">
              <strong class="title">{{ job.title }}</strong>
              <span class="phase">{{ job.cancelling ? 'Cancelling' : job.phase }}</span>
              <span class="count">{{ count(job) }}</span>
            </div>
            <mat-progress-bar
              [mode]="job.total ? 'determinate' : 'indeterminate'"
              [value]="percent(job)"
              [attr.aria-label]="job.title + ': ' + job.phase"
            />
            @if (jobs.stalled(job)) {
              <p class="note" role="status">Still working. This can take a while; you can keep using Pitwall.</p>
            }
            @if (!job.cancellable && job.notCancellableReason) {
              <p class="note">{{ job.notCancellableReason }}</p>
            }
            <div class="actions">
              <button type="button" mat-button [disabled]="job.cancelling" (click)="jobs.cancel(job.id)" [attr.aria-label]="'Cancel ' + job.title">Cancel</button>
              @if (job.canStopWaiting) {
                <button type="button" mat-button (click)="jobs.stopWaiting(job.id)" [attr.aria-label]="'Stop waiting for ' + job.title">Stop waiting</button>
                <span class="note">Cancel did not take effect; the server may still be working.</span>
              }
            </div>
          </div>
        }
      </section>
    }
  `,
  styles: `
    .jobs { display: flex; flex-direction: column; gap: var(--sp-2); }
    .job { display: flex; flex-direction: column; gap: var(--sp-1); padding: var(--sp-3); background: var(--bg-raised); border: 1px solid var(--border); border-radius: var(--r-md); }
    .head { display: flex; align-items: baseline; gap: var(--sp-2); min-width: 0; }
    .title { font-weight: 600; }
    .phase, .count, .note { color: var(--text-muted); font-size: var(--fs-sm); margin: 0; }
    .count { margin-left: auto; font-variant-numeric: tabular-nums; }
    .actions { display: flex; align-items: center; gap: var(--sp-2); }
  `,
})
export class JobsPanel {
  protected readonly jobs = inject(JobsService);

  protected percent(job: JobRow): number {
    return job.total ? Math.min(100, percent(job.done, job.total)) : 0;
  }

  /** "42 of 118 MB", "3 of 40", "12,345 so far" or, with nothing to count, the time spent: never a static 0. */
  protected count(job: JobRow): string {
    const fmt = (n: number) => (job.unit === 'bytes' ? bytes(n) : int(n));
    if (job.total) return `${fmt(Math.min(job.done, job.total))} of ${fmt(job.total)}`;
    if (job.done > 0 && job.unit === 'bytes') return `${fmt(job.done)} so far`;
    return `${job.elapsedSecs} s`;
  }
}
