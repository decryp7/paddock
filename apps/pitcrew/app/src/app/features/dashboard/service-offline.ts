import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { EmptyState } from '../../shared/ui/empty-state';
import { ServiceNotices } from '../../shared/ui/service-notices';
import { ServiceStore } from '../../state/service.store';
import { StatusStore } from '../../state/status.store';

/**
 * What the dashboard shows while the app cannot talk to the service: not installed, not running, or a different version, and
 * what to do about it (Install service when it never was, and Retry). The status-bar chip says the same in one line and
 * repairs with Restart; this is the page's explanation.
 */
@Component({
  selector: 'pc-service-offline',
  imports: [MatIcon, EmptyState, ServiceNotices],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pc-empty-state [brand]="true" [title]="title()" [text]="text()">
      @if (canInstall()) {
        <button type="button" class="btn primary" (click)="service.install()" [disabled]="service.working() !== null">
          <mat-icon svgIcon="pc:shield" aria-hidden="true" />
          {{ service.working() === 'install' ? 'Waiting for permission…' : 'Install service' }}
        </button>
      }
      <button type="button" class="btn" (click)="retry()" [disabled]="status.loading()">
        <mat-icon svgIcon="pc:refresh" aria-hidden="true" [class.spin]="status.loading()" />
        Retry
      </button>
    </pc-empty-state>
    <pc-service-notices />
  `,
})
export class ServiceOffline {
  protected readonly status = inject(StatusStore);
  protected readonly service = inject(ServiceStore);

  protected readonly canInstall = computed(() => {
    const s = this.service.registration().value;
    return this.status.offline()?.kind === 'unavailable' && s !== null && !s.installed && s.canInstall && !s.disabledInLoginItems && !s.repair;
  });
  protected readonly title = computed(() => {
    if (this.status.offline()?.kind === 'mismatch') return 'The app and the service are different versions';
    const s = this.service.registration().value;
    return s && !s.installed ? 'The Pitcrew service is not installed' : 'Pitcrew cannot reach its service';
  });
  protected readonly text = computed(() => {
    const off = this.status.offline();
    const s = this.service.registration().value;
    const hint =
      !this.canInstall() || !s
        ? ''
        : s.platform === 'macos'
          ? ' Installing it adds Pitcrew to Login Items; macOS asks you to approve it in System Settings.'
          : ' Installing it asks for administrator permission (a Windows UAC prompt).';
    return `${off?.message ?? 'The runners, their jobs and their history appear here once Pitcrew can reach its service.'}${hint}`;
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
