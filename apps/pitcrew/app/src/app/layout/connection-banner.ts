import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { ServiceNotices } from '../shared/ui/service-notices';
import { ServiceStore } from '../state/service.store';
import { StatusStore } from '../state/status.store';

/** Shown when the app cannot talk to the service: not installed, not running, or a different version. */
@Component({
  selector: 'pc-connection-banner',
  imports: [MatIcon, ServiceNotices],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (status.offline(); as off) {
      <div class="banner" role="alert" [class.mismatch]="off.kind === 'mismatch'">
        <mat-icon [svgIcon]="off.kind === 'mismatch' ? 'pc:alert' : 'pc:cloud-off'" aria-hidden="true" />
        <div class="text">
          <strong>{{ title() }}</strong>
          <span>{{ off.message }} {{ hint() }}</span>
          <pc-service-notices />
        </div>
        <div class="btn-row">
          @if (canInstall()) {
            <button type="button" class="btn primary sm" (click)="service.install()" [disabled]="service.working() !== null">
              <mat-icon svgIcon="pc:shield" aria-hidden="true" />
              {{ service.working() === 'install' ? 'Waiting for permission…' : 'Install service' }}
            </button>
          }
          <button type="button" class="btn sm" (click)="retry()" [disabled]="status.loading()">
            <mat-icon svgIcon="pc:refresh" aria-hidden="true" [class.spin]="status.loading()" />
            Retry
          </button>
        </div>
      </div>
    }
  `,
  styles: `
    .banner {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-5);
      border-bottom: 1px solid color-mix(in srgb, var(--warning) 30%, transparent);
      background: var(--warning-soft);
    }
    .banner.mismatch {
      border-color: color-mix(in srgb, var(--danger) 30%, transparent);
      background: var(--danger-soft);
    }
    .banner > mat-icon {
      flex: none;
      color: var(--warning);
    }
    .mismatch > mat-icon {
      color: var(--danger);
    }
    .text {
      flex: 1;
      display: grid;
      min-width: 0;
    }
    .text span {
      color: var(--text-2);
    }
  `,
})
export class ConnectionBanner {
  protected readonly status = inject(StatusStore);
  protected readonly service = inject(ServiceStore);

  protected readonly canInstall = computed(() => {
    const s = this.service.registration().value;
    return this.status.offline()?.kind === 'unavailable' && s !== null && !s.installed && s.canInstall && !s.disabledInLoginItems && !s.repair;
  });
  protected readonly title = computed(() => {
    const off = this.status.offline();
    if (off?.kind === 'mismatch') return 'The app and the service are different versions';
    const s = this.service.registration().value;
    return s && !s.installed ? 'The Pitcrew service is not installed' : 'Pitcrew cannot reach its service';
  });
  protected readonly hint = computed(() => {
    const s = this.service.registration().value;
    if (!this.canInstall() || !s) return '';
    return s.platform === 'macos'
      ? 'Installing it adds Pitcrew to Login Items; macOS asks you to approve it in System Settings.'
      : 'Installing it asks for administrator permission (a Windows UAC prompt).';
  });

  constructor() {
    // Learn whether the service is installed whenever the connection is lost.
    effect(() => {
      if (this.status.offline()) untracked(() => void this.service.load());
    });
  }

  protected retry(): void {
    void this.status.load();
    void this.service.load();
  }
}
