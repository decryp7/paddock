import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { ServiceStore } from './service.store';

/**
 * What the user needs to know about the background service on macOS: the automatic repair of a registration launchd
 * refused (with its progress), or the item being switched off in System Settings > General > Login Items, with the
 * button that opens that pane. The engine decides both; this only shows them.
 */
@Component({
  selector: 'pw-service-notices',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (service.service(); as s) {
      @if (s.repair; as r) {
        <div class="callout" [attr.data-tone]="r.state === 'failed' ? 'bad' : r.state === 'succeeded' ? 'ok' : 'info'" role="status" data-testid="service-repair">
          <mat-icon [svgIcon]="r.state === 'working' ? 'pw:refresh' : r.state === 'succeeded' ? 'pw:success' : 'pw:alert'" aria-hidden="true" />
          <span>{{ r.message }}</span>
        </div>
      }
      @if (s.disabledInLoginItems) {
        <div class="callout" data-tone="warn" role="status" data-testid="service-disabled">
          <mat-icon svgIcon="pw:alert" aria-hidden="true" />
          <span class="msg">Pitwall's background service is turned off in System Settings > General > Login Items. Switch it on there; Pitwall never does it for you.</span>
          <button type="button" class="btn sm" [disabled]="service.isBusy()('loginItems')" (click)="service.openLoginItems()">Open Login Items</button>
        </div>
      }
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-2);
    }
    :host:empty {
      display: none;
    }
    .callout {
      align-items: center;
    }
    span {
      flex: 1;
      min-width: 0;
    }
  `,
})
export class ServiceNotices {
  protected readonly service = inject(ServiceStore);
}
