import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { CommandError } from '../../core/api.types';

/**
 * A panel's error boundary: a skeleton while the first request runs, an inline error with Retry when it fails, and the
 * content otherwise. A failed refresh keeps the last good content and shows the error above it.
 */
@Component({
  selector: 'pc-load-state',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (error(); as e) {
      <div class="callout danger" [class.compact]="hasValue()" role="alert">
        <mat-icon svgIcon="pc:alert" aria-hidden="true" />
        <div class="callout-body">
          <strong>{{ what() }} could not be loaded</strong>
          <span class="muted">{{ e.message }}</span>
        </div>
        <button type="button" class="btn sm" (click)="retry.emit()">
          <mat-icon svgIcon="pc:refresh" aria-hidden="true" />
          Retry
        </button>
      </div>
    }
    @if (hasValue()) {
      <ng-content />
    } @else if (!error()) {
      <div class="skeletons" aria-busy="true" [attr.aria-label]="'Loading ' + what().toLowerCase()">
        @for (h of bars(); track $index) {
          <span class="skeleton" [style.height.px]="h" [style.width]="$last && bars().length > 1 ? '60%' : '100%'"></span>
        }
      </div>
    }
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-3);
      min-width: 0;
    }
    .skeletons {
      display: grid;
      gap: var(--space-2);
    }
    .compact {
      padding: var(--space-2) var(--space-3);
    }
  `,
})
export class LoadState {
  readonly hasValue = input.required<boolean>();
  readonly error = input<CommandError | null>(null);
  /** What is loading, for the messages ("Activity"). */
  readonly what = input('This');
  /** Skeleton bar heights. */
  readonly skeleton = input<number[]>([16, 16, 16]);
  readonly retry = output<void>();

  protected readonly bars = computed(() => this.skeleton());
}
