import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { ClockService } from '../../core/clock.service';
import { RegisterLauncher } from '../../layout/register-launcher';
import type { RunnerSnapshot } from '../../core/api.types';
import { formatBytes, formatDuration, formatElapsed } from '../../shared/format';
import { RunnerActions } from '../../shared/runner-actions';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { StatusStore } from '../../state/status.store';
import { RunnerStatusText } from './runner-status';

/** Every runner in a table. */
@Component({
  selector: 'pc-runners-page',
  imports: [MatIcon, RouterLink, RunnerStatusText, RunnerActions, EmptyState, LoadState],
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
                  <th scope="col" class="num">Disk</th>
                  <th scope="col"><span class="visually-hidden">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                @for (r of status.runners(); track r.name) {
                  <tr>
                    <td class="who">
                      <div class="line">
                        <a class="name" [routerLink]="['/runners', r.name]">{{ r.name }}</a>
                        @if (r.leftovers > 0) {
                          <a class="leftover" [routerLink]="['/runners', r.name]" [queryParams]="{ tab: 'processes' }" [title]="leftoverText(r)" [attr.aria-label]="leftoverText(r)">
                            <mat-icon svgIcon="pc:alert" aria-hidden="true" />
                          </a>
                        }
                      </div>
                      <div class="sub faint truncate" [title]="allLabels(r)">{{ subLine(r) }}</div>
                    </td>
                    <td class="nowrap"><pc-runner-status [status]="r.status" /></td>
                    <td class="job">
                      @if (r.currentJob; as j) {
                        <span class="truncate" [title]="j.info.repository ?? ''">{{ j.info.repository ?? '—' }}</span>
                        <span class="sub faint truncate">{{ j.info.job }} · {{ elapsed(j.startedAt) }}</span>
                      } @else if (r.problem) {
                        <span class="sub tone-danger truncate" [title]="r.problem">{{ r.problem }}</span>
                        <a class="sub logs-link" routerLink="/logs" [queryParams]="{ runner: r.name }">View logs</a>
                      } @else {
                        <span class="faint">—</span>
                      }
                    </td>
                    <td class="nowrap">
                      <span class="num ver">{{ r.currentVersion ?? '—' }}</span>
                      @if (r.updateTarget) {
                        <span class="badge update small" [title]="r.updateTarget + ' is available'">{{ r.updateTarget }}</span>
                      }
                    </td>
                    <td class="num">{{ r.runningSince ? uptime(r.runningSince) : '—' }}</td>
                    <td class="num" [class.tone-danger]="r.disk?.low">{{ bytes(r.disk?.folderBytes) }}</td>
                    <td class="actions">
                      <div class="row-actions">
                        <a class="btn icon sm ghost" routerLink="/logs" [queryParams]="{ runner: r.name }" [attr.aria-label]="'Logs of ' + r.name" title="Logs">
                          <mat-icon svgIcon="pc:logs" aria-hidden="true" />
                        </a>
                        <pc-runner-actions [runner]="r" [small]="true" [allowRemove]="true" />
                      </div>
                    </td>
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
    tbody tr {
      height: 48px;
    }
    td {
      padding-top: 4px;
      padding-bottom: 4px;
    }
    .nowrap {
      white-space: nowrap;
    }
    .who {
      max-width: 300px;
    }
    .line {
      display: flex;
      align-items: center;
      gap: 6px;
    }
    .name {
      color: var(--text);
      font-weight: var(--weight-semibold);
    }
    .leftover {
      display: inline-flex;
      color: var(--warning);
      mat-icon {
        width: 12px;
        height: 12px;
      }
    }
    .sub {
      display: block;
      font-size: var(--text-sm);
    }
    .logs-link {
      display: inline;
      color: var(--text-2);
      text-decoration: underline;
    }
    .job {
      max-width: 280px;
    }
    .job .truncate {
      display: block;
    }
    .ver {
      font-weight: var(--weight-semibold);
    }
    .small {
      height: 18px;
      margin-left: 6px;
      font-size: var(--text-xs);
      font-variant-numeric: tabular-nums;
    }
    .actions {
      width: 80px;
      text-align: right;
    }
    .row-actions {
      display: inline-flex;
      align-items: center;
      justify-content: flex-end;
      gap: 2px;
    }
    .row-actions a {
      opacity: 0;
    }
    pc-runner-actions {
      opacity: 0.4;
    }
    tr:hover .row-actions a,
    tr:focus-within .row-actions a,
    tr:hover pc-runner-actions,
    tr:focus-within pc-runner-actions {
      opacity: 1;
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

  /** The labels shown inline: the first two, then how many more. */
  protected subLine(r: RunnerSnapshot): string {
    const shown = r.labels.slice(0, 2).join(', ');
    const more = r.labels.length > 2 ? ` +${r.labels.length - 2}` : '';
    return [r.kind, shown].filter((p) => p !== '').join(' · ') + more;
  }
  protected allLabels(r: RunnerSnapshot): string {
    return r.labels.length ? r.labels.join(', ') : 'No labels';
  }
  protected leftoverText(r: RunnerSnapshot): string {
    return `${r.leftovers} leftover ${r.leftovers === 1 ? 'process' : 'processes'}`;
  }
}
