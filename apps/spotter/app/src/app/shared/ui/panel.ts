import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { SuSkeleton } from '@decryp7/paddock-ui';
import type { Loadable } from '../../state/loadable';

/**
 * The error boundary of a panel: while nothing has loaded it shows a skeleton, a failure shows the cause with Retry
 * (never a blank area), and a failed refresh keeps the last good content with an inline notice.
 */
@Component({
  selector: 'sp-panel',
  imports: [SuSkeleton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let s = state();
    @if (s.error && s.value === null) {
      <!-- Not the shared error state: its Retry is a filled primary, and a page with several failed panels has one primary at most. -->
      <div class="failed" role="alert">
        <h3>Could not load {{ label() }}</h3>
        <p>{{ s.error.message }}</p>
        <button type="button" class="btn" [disabled]="s.loading" (click)="retry.emit()">Retry</button>
      </div>
    } @else if (s.value === null) {
      <div class="loading" role="status" aria-live="polite">
        <span class="pd-visually-hidden">Loading {{ label() }}…</span>
        <pd-skeleton shape="block" height="72" />
        <pd-skeleton shape="text" width="60%" />
        <pd-skeleton shape="text" width="40%" />
      </div>
    } @else {
      @if (s.error) {
        <div class="stale" role="alert">
          <span>Could not refresh {{ label() }}: {{ s.error.message }}</span>
          <button type="button" class="btn sm" (click)="retry.emit()">Retry</button>
        </div>
      }
      <ng-content />
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .failed {
      display: grid;
      justify-items: start;
      gap: var(--pd-space-2);
      padding: var(--pd-space-4);
      border: 1px solid var(--pd-border);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-1);
    }
    .failed h3 {
      margin: 0;
      font-size: var(--pd-text-body);
      font-weight: var(--pd-weight-semibold);
    }
    .failed p {
      margin: 0;
      color: var(--pd-text-muted);
    }
    .loading {
      display: grid;
      gap: var(--pd-space-3);
    }
    .stale {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-3);
      margin-bottom: var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-3);
      border-radius: var(--pd-radius-control);
      background: var(--pd-warning-tint);
      color: var(--pd-text);
    }
  `,
})
export class Panel {
  readonly state = input.required<Loadable<unknown>>();
  /** What the panel shows, for messages ("the dashboard"). */
  readonly label = input.required<string>();
  readonly retry = output<void>();
}
