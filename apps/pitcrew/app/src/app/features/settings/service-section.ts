import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MatIcon } from '@angular/material/icon';
import { ClockService } from '../../core/clock.service';
import { formatDuration } from '../../shared/format';
import { confirmAction } from '../../shared/confirm';
import { ServiceNotices } from '../../shared/ui/service-notices';
import { LoadState } from '../../shared/ui/load-state';
import { ServiceStore } from '../../state/service.store';
import { StatusStore } from '../../state/status.store';

/** The Pitcrew service: whether it is installed and running, and installing or removing it. */
@Component({
  selector: 'pc-service-section',
  imports: [MatIcon, LoadState, ServiceNotices],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header"><h2>Pitcrew service</h2></header>
      <div class="card-body body">
        <pc-load-state what="The service state" [hasValue]="!!service.registration().value" [error]="service.registration().error" [skeleton]="[40, 20]" (retry)="service.load()">
          @if (service.registration().value; as s) {
            <div class="state">
              <span class="badge" [class]="s.running ? 'success' : s.installed ? 'warning' : 'neutral'">
                <mat-icon [svgIcon]="s.running ? 'pc:check-circle' : s.installed ? 'pc:alert' : 'pc:power'" aria-hidden="true" />
                {{ s.running ? 'Installed and running' : s.installed ? 'Installed, not running' : 'Not installed' }}
              </span>
              @if (status.service(); as svc) {
                <span class="faint small num">version {{ svc.version }} · up {{ up() }} · data in {{ svc.dataDir }}</span>
              }
            </div>
            <pc-service-notices />
            @if (s.message) {
              <p class="muted">{{ s.message }}</p>
            }
            <p class="muted">
              The service supervises the runners whether or not this window is open, and survives log-off.
              {{ isMac() ? 'On macOS it is your own launch agent: installing it adds Pitcrew to Login Items, and macOS asks you to approve it in System Settings ▸ General ▸ Login Items.' : 'On Windows it is a service started by Windows; installing or removing it asks for administrator permission (a UAC prompt). Pitcrew never asks for or stores your password.' }}
            </p>
            <div class="btn-row">
              @if (service.canRepair()) {
                <button type="button" class="btn" data-testid="repair-service-settings" (click)="service.repair()" [disabled]="service.working() !== null">
                  <mat-icon svgIcon="pc:refresh" aria-hidden="true" />{{ service.working() === 'repair' ? 'Repairing…' : 'Repair service' }}
                </button>
              }
              @if (!s.installed && !s.disabledInLoginItems) {
                <button type="button" class="btn primary" (click)="service.install()" [disabled]="!s.canInstall || service.working() !== null">
                  <mat-icon svgIcon="pc:shield" aria-hidden="true" />{{ service.working() === 'install' ? 'Waiting for permission…' : 'Install service' }}
                </button>
              } @else {
                <button type="button" class="btn danger" (click)="uninstall()" [disabled]="service.working() !== null">
                  <mat-icon svgIcon="pc:trash" aria-hidden="true" />{{ service.working() === 'uninstall' ? 'Waiting for permission…' : 'Uninstall service' }}
                </button>
              }
            </div>
            @if (!s.canInstall && !s.installed && !s.disabledInLoginItems) {
              <p class="faint small">Installing is not possible here{{ s.platform === 'browser' ? ' (browser build)' : '' }}.</p>
            }
          }
        </pc-load-state>
      </div>
    </section>
  `,
  styles: `
    h2 {
      font-size: var(--text-md);
    }
    .body,
    pc-load-state {
      display: grid;
      gap: var(--space-3);
    }
    .state {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-3);
    }
    .small {
      font-size: var(--text-sm);
    }
    p {
      max-width: 72ch;
    }
  `,
})
export class ServiceSection {
  protected readonly service = inject(ServiceStore);
  protected readonly status = inject(StatusStore);
  private readonly clock = inject(ClockService);
  private readonly ports = inject(SHELL_PORTS);

  protected readonly isMac = computed(() => this.service.registration().value?.platform === 'macos');
  protected readonly up = computed(() => {
    const svc = this.status.service();
    return svc ? formatDuration(this.clock.now() - svc.startedAt) : '—';
  });

  constructor() {
    void this.service.load();
  }

  protected async uninstall(): Promise<void> {
    const ok = await confirmAction(this.ports, {
      title: 'Uninstall the Pitcrew service?',
      body: 'The runners stop after their current jobs and are no longer restarted or updated. Their folders, registrations and pitcrew.toml stay on disk; installing the service again picks them up.',
      confirm: 'Uninstall service',
      danger: true,
    });
    if (ok) void this.service.uninstall();
  }
}
