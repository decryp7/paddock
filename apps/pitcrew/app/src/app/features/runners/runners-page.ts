import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { ClockService } from '../../core/clock.service';
import { RegisterLauncher } from '../../layout/register-launcher';
import { formatBytes, formatDuration, formatElapsed } from '../../shared/format';
import { RunnerActions } from '../../shared/runner-actions';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { StatusBadge } from '../../shared/ui/status-badge';
import { StatusStore } from '../../state/status.store';

/** Every runner in a table. */
@Component({
  selector: 'pc-runners-page',
  imports: [MatIcon, RouterLink, StatusBadge, RunnerActions, EmptyState, LoadState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-header">
        <div>
          <h1>Runners</h1>
          <p class="subtitle">The runners Pitcrew supervises on this machine.</p>
        </div>
        <button type="button" class="btn primary" (click)="register.open()"><mat-icon svgIcon="pc:plus" aria-hidden="true" />Register runner</button>
      </header>

      <section class="card">
        <pc-load-state what="Runners" [hasValue]="!!status.status()" [error]="status.offline() ? null : status.error()" [skeleton]="[36, 36, 36, 36]" (retry)="status.load()">
          @if (status.runners().length === 0) {
            <pc-empty-state [brand]="true" title="No runners yet" text="Register a runner, or adopt a runner folder that is registered already.">
              <button type="button" class="btn primary" (click)="register.open()"><mat-icon svgIcon="pc:plus" aria-hidden="true" />Register a runner</button>
            </pc-empty-state>
          } @else {
            <table class="table">
              <thead>
                <tr>
                  <th scope="col">Runner</th>
                  <th scope="col">Status</th>
                  <th scope="col">Current job</th>
                  <th scope="col">Version</th>
                  <th scope="col" class="num">Up for</th>
                  <th scope="col" class="num">Folder</th>
                  <th scope="col"><span class="visually-hidden">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                @for (r of status.runners(); track r.name) {
                  <tr>
                    <td>
                      <a class="name" [routerLink]="['/runners', r.name]">{{ r.name }}</a>
                      <div class="sub faint">{{ r.kind }} · {{ r.labels.join(', ') }}</div>
                    </td>
                    <td>
                      <pc-status-badge [status]="r.status" />
                    </td>
                    <td class="job">
                      @if (r.currentJob; as j) {
                        <span class="truncate">{{ j.info.repository ?? '—' }}</span>
                        <span class="sub faint">{{ j.info.job }} · {{ elapsed(j.startedAt) }}</span>
                      } @else if (r.problem) {
                        <span class="sub tone-danger truncate" [title]="r.problem">{{ r.problem }}</span>
                      } @else {
                        <span class="faint">—</span>
                      }
                    </td>
                    <td>
                      <span class="num">{{ r.currentVersion ?? '—' }}</span>
                      @if (r.updateTarget) {
                        <span class="badge accent small">{{ r.updateTarget }} available</span>
                      }
                    </td>
                    <td class="num">{{ r.runningSince ? uptime(r.runningSince) : '—' }}</td>
                    <td class="num" [class.tone-danger]="r.disk?.low">{{ bytes(r.disk?.folderBytes) }}</td>
                    <td class="actions"><pc-runner-actions [runner]="r" [small]="true" [allowRemove]="true" /></td>
                  </tr>
                }
              </tbody>
            </table>
          }
        </pc-load-state>
      </section>
    </div>
  `,
  styles: `
    section {
      padding: var(--space-2);
    }
    .name {
      color: var(--text);
      font-weight: var(--weight-semibold);
    }
    .sub {
      display: block;
      font-size: var(--text-sm);
    }
    .job {
      max-width: 280px;
    }
    .job .truncate {
      display: block;
    }
    .small {
      height: 18px;
      margin-left: 6px;
      font-size: var(--text-xs);
    }
    .actions {
      width: 40px;
      text-align: right;
    }
  `,
})
export class RunnersPage {
  protected readonly status = inject(StatusStore);
  protected readonly register = inject(RegisterLauncher);
  private readonly clock = inject(ClockService);

  protected elapsed(since: number): string {
    return formatElapsed(this.clock.now() - since);
  }
  protected uptime(since: number): string {
    return formatDuration(this.clock.now() - since);
  }
  protected bytes(n: number | undefined): string {
    return formatBytes(n);
  }
}
