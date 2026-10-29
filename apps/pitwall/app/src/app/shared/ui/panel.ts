import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { CommandError } from '../../core/api.types';
import { ServiceStore } from '../../features/service/service.store';
import { Skeleton } from './skeleton';

/**
 * A card with a header and an error boundary: while `loading` with no data it shows a skeleton; on `error` an inline
 * message with Retry (content stays when there is data to keep); otherwise the projected content.
 * Header actions are projected with the `panel-actions` attribute.
 */
@Component({
  selector: 'pw-panel',
  imports: [MatIconModule, Skeleton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class.flush]': 'flush()', '[attr.aria-busy]': 'loading()' },
  template: `
    @if (heading()) {
      <header>
        <div class="titles">
          <h2>
            @if (icon()) {
              <mat-icon [svgIcon]="'pw:' + icon()" aria-hidden="true" />
            }
            {{ heading() }}
          </h2>
          @if (sub()) {
            <span class="sub">{{ sub() }}</span>
          }
        </div>
        <div class="actions"><ng-content select="[panel-actions]" /></div>
      </header>
    }
    @if (error(); as e) {
      <div class="error" role="alert">
        <mat-icon svgIcon="pw:alert" aria-hidden="true" />
        <span class="msg">{{ e.message }}</span>
        @if (e.code === 'serverUnavailable' && service.canRepair()) {
          <button type="button" class="btn sm primary" data-testid="repair-service" [disabled]="service.isBusy()('repair')" (click)="service.repair()">Repair service</button>
        }
        <button type="button" class="btn sm" (click)="retry.emit()">Retry</button>
      </div>
    }
    @if (loading() && !ready()) {
      <pw-skeleton [lines]="skeletonLines()" />
    } @else if (ready()) {
      <ng-content />
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      min-width: 0;
      padding: var(--pd-space-4);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-1);
    }
    :host(.flush) {
      padding: 0;
      header {
        padding: var(--pd-space-3) var(--pd-space-4) 0;
      }
      .error {
        margin: 0 var(--pd-space-4) var(--pd-space-3);
      }
      pw-skeleton {
        padding: var(--pd-space-2) var(--pd-space-4) var(--pd-space-4);
      }
    }
    header {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      justify-content: space-between;
      gap: var(--pd-space-1) var(--pd-space-3);
      min-height: 24px;
      margin-bottom: var(--pd-space-3);
    }
    .titles {
      display: flex;
      align-items: baseline;
      gap: var(--pd-space-2);
      min-width: 0;
    }
    h2 {
      display: flex;
      align-items: center;
      gap: 7px;
      font-size: var(--pd-text-body);
      font-weight: var(--pd-weight-semibold);
      letter-spacing: 0;
      white-space: nowrap;
      mat-icon {
        width: 15px;
        height: 15px;
        color: var(--pd-text-subtle);
      }
    }
    .sub {
      color: var(--pd-text-subtle);
      font-size: var(--pd-text-label);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .actions {
      display: flex;
      align-items: center;
      gap: var(--pd-space-1);
      flex: 0 1 auto;
      min-width: 0;
    }
    .error {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      margin-bottom: var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-2) var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text);
      background: var(--pd-danger-tint);
      border-radius: var(--pd-radius-control);
      mat-icon {
        color: var(--pd-danger);
        flex: none;
      }
      .msg {
        flex: 1;
      }
    }
  `,
})
export class Panel {
  protected readonly service = inject(ServiceStore);
  readonly heading = input<string>('');
  readonly sub = input<string>('');
  readonly icon = input<string>('');
  readonly loading = input(false);
  readonly error = input<CommandError | null>(null);
  /** There is data to show (keeps content visible during reloads and errors). */
  readonly ready = input(true);
  readonly skeletonLines = input(4);
  readonly flush = input(false);
  readonly retry = output<void>();
}
