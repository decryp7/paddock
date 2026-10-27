import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { SuErrorState, SuSkeleton } from '@decryp7/paddock-ui';
import type { Loadable } from '../../state/loadable';

/**
 * The error boundary of a panel: while nothing has loaded it shows a skeleton, a failure shows the cause with Retry
 * (never a blank area), and a failed refresh keeps the last good content with an inline notice.
 */
@Component({
  selector: 'sp-panel',
  imports: [SuErrorState, SuSkeleton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let s = state();
    @if (s.error && s.value === null) {
      <pd-error-state [heading]="'Could not load ' + label()" [cause]="s.error.message" [busy]="s.loading" (retry)="retry.emit()" />
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
