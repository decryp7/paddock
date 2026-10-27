import { ChangeDetectionStrategy, Component, DestroyRef, InjectionToken, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { jobFraction, jobNumbers } from '../shared/job-text';
import { JobsStore, type JobView } from '../state/jobs.store';

/** How the panel treats time. */
export interface OperationsTiming {
  /** A job is shown once it has run this long, so quick ones do not flash a panel. */
  showAfterMs: number;
  /** The engine reports about ten times a second while it works; this long without a word and the panel says it is still working. */
  quietAfterMs: number;
  /** How often the panel looks at the clock. */
  tickMs: number;
}

export const OPERATIONS_TIMING = new InjectionToken<OperationsTiming>('OperationsTiming', {
  factory: () => ({ showAfterMs: 600, quietAfterMs: 10_000, tickMs: 250 }),
});

/**
 * The running long operations with what each is doing now: the phase in words, the numbers on that phase's own scale,
 * a bar (indeterminate while the total is not known) and Cancel. The UI never times a job out; when the engine is
 * quiet for a while the panel says it is still working.
 */
@Component({
  selector: 'sp-operations',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (visible().length) {
      <section class="ops" aria-label="Running operations">
        @for (j of visible(); track j.id) {
          <div class="op">
            <div class="head">
              <strong class="title">{{ j.title }}</strong>
              <button type="button" class="btn ghost sm" (click)="jobs.cancel(j.id)" [disabled]="j.cancelling" [attr.aria-label]="'Cancel: ' + j.title">
                <mat-icon svgIcon="sp:x" aria-hidden="true" />{{ j.cancelling ? 'Cancelling…' : 'Cancel' }}
              </button>
            </div>
            @if (j.progress; as p) {
              <div class="line">
                <span class="phase">{{ p.label }}</span>
                <span class="numbers">{{ numbers(j) }}</span>
              </div>
              @if (fraction(j); as f) {
                <progress max="1" [value]="f" [attr.aria-label]="p.label"></progress>
              } @else {
                <progress [attr.aria-label]="p.label"></progress>
              }
            } @else {
              <div class="line"><span class="phase">Starting…</span></div>
              <progress aria-label="Starting"></progress>
            }
            @if (quiet(j)) {
              <p class="quiet" role="status">Still working. This can take a while; you can keep using Spotter or cancel.</p>
            }
          </div>
        }
      </section>
    }
  `,
  styles: `
    .ops {
      position: fixed;
      left: calc(var(--pd-sidebar-width) + var(--pd-space-4));
      bottom: var(--pd-space-4);
      z-index: var(--pd-z-toast);
      display: grid;
      gap: var(--pd-space-2);
      width: min(420px, calc(100vw - 32px));
    }
    .op {
      display: grid;
      gap: var(--pd-space-2);
      padding: var(--pd-space-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-popover);
      background: var(--pd-surface-3);
      box-shadow: var(--pd-shadow-3);
    }
    .head,
    .line {
      display: flex;
      gap: var(--pd-space-3);
      align-items: center;
      justify-content: space-between;
      min-width: 0;
    }
    .title {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .phase {
      font-weight: 600;
    }
    .numbers {
      color: var(--pd-text-muted);
      font-variant-numeric: tabular-nums;
    }
    progress {
      width: 100%;
      height: 6px;
      accent-color: var(--pd-accent);
    }
    .quiet {
      margin: 0;
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
  `,
})
export class Operations {
  protected readonly jobs = inject(JobsStore);
  private readonly timing = inject(OPERATIONS_TIMING);
  private readonly now = signal(Date.now());

  constructor() {
    const timer = setInterval(() => this.now.set(Date.now()), this.timing.tickMs);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  protected visible(): JobView[] {
    const now = this.now();
    return this.jobs.jobs().filter((j) => now - j.startedAt >= this.timing.showAfterMs);
  }

  protected numbers(j: JobView): string {
    return j.progress ? jobNumbers(j.progress) : '';
  }

  protected fraction(j: JobView): number | null {
    return jobFraction(j.progress);
  }

  protected quiet(j: JobView): boolean {
    return this.now() - j.lastEventAt >= this.timing.quietAfterMs;
  }
}
