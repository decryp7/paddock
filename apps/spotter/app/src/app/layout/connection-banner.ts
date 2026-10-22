import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { WatchesStore } from '../state/watches.store';

/** Shown when the engine does not answer: what happened, and Retry. */
@Component({
  selector: 'sp-connection-banner',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.error(); as err) {
      <div class="banner" role="alert">
        <mat-icon svgIcon="sp:cloud-off" aria-hidden="true" />
        <div class="text">
          <strong>Spotter cannot reach its engine</strong>
          <span>{{ err.message }}</span>
        </div>
        <button type="button" class="btn sm" (click)="store.load()" [disabled]="store.loading()">
          <mat-icon svgIcon="sp:refresh" aria-hidden="true" [class.spin]="store.loading()" />
          Retry
        </button>
      </div>
    }
  `,
  styles: `
    .banner {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) var(--pd-space-6);
      background: var(--pd-warning-tint);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .banner > mat-icon {
      flex: none;
      color: var(--pd-warning);
    }
    .text {
      flex: 1;
      display: grid;
      min-width: 0;
    }
    .text span {
      color: var(--pd-text-muted);
    }
  `,
})
export class ConnectionBanner {
  protected readonly store = inject(WatchesStore);
}
