import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { ServiceStore } from '../../state/service.store';

/** What the user needs to know about the background service on macOS: the automatic repair, or the item being switched off. */
@Component({
  selector: 'pc-service-notices',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (service.registration().value; as s) {
      @if (s.repair; as r) {
        <p class="notice" [class.danger]="r.state === 'failed'" role="status" data-testid="service-repair">
          <mat-icon [svgIcon]="r.state === 'working' ? 'pc:refresh' : r.state === 'succeeded' ? 'pc:check-circle' : 'pc:alert'" aria-hidden="true" [class.spin]="r.state === 'working'" />
          <span>{{ r.message }}</span>
        </p>
      }
      @if ((s.startFailed || s.notInLoginItems) && s.repair?.state !== 'working') {
        <div class="notice" role="status" data-testid="service-start-failed">
          <mat-icon svgIcon="pc:alert" aria-hidden="true" />
          <span>
            {{ s.startFailed ? 'The background service could not start after the update.' : 'Pitcrew is not listed in System Settings ▸ General ▸ Login Items, so macOS does not start the service.' }}
            Repair registers it again.
          </span>
          <button type="button" class="btn sm primary" data-testid="repair-service" [disabled]="service.working() !== null" (click)="service.repair()">
            {{ service.working() === 'repair' ? 'Repairing…' : 'Repair' }}
          </button>
        </div>
      }
      @if (s.disabledInLoginItems) {
        <div class="notice" role="status" data-testid="service-disabled">
          <mat-icon svgIcon="pc:alert" aria-hidden="true" />
          <span>Pitcrew's background service is turned off in System Settings ▸ General ▸ Login Items</span>
          <button type="button" class="btn sm" (click)="service.openLoginItems()">Open Login Items</button>
        </div>
      }
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-2);
    }
    .notice {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
      margin: 0;
    }
    .notice.danger mat-icon {
      color: var(--danger);
    }
    .notice span {
      flex: 1;
      min-width: 0;
    }
  `,
})
export class ServiceNotices {
  protected readonly service = inject(ServiceStore);
}
