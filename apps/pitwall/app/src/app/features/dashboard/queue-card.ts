import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { Job, Priority } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { compact, duration } from '../../shared/format';
import { PRIORITY_LABEL } from '../../shared/labels';

const PRIORITIES: Priority[] = ['interactive', 'background', 'batch'];

/** Queue depth by priority and the running and waiting jobs, each cancellable. */
@Component({
  selector: 'pw-queue-card',
  imports: [MatIconModule, MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.queue(); as q) {
      <p class="depth num" aria-label="Waiting by priority">
        @for (p of priorities; track p) {
          <span>{{ q.depth[p] }} {{ label[p].toLowerCase() }}</span>
        }
      </p>
      @if (jobs().length === 0) {
        <p class="none">Nothing running or waiting.</p>
      }
      <ul class="jobs" aria-label="Jobs">
        @for (j of jobs().slice(0, max()); track j.id) {
          <li>
            <span class="job-dot" [attr.data-state]="j.state" [attr.aria-label]="j.state === 'running' ? 'Running' : 'Waiting'"></span>
            <div class="main">
              <div class="line1">
                <span class="app">{{ appName(j.appId) }}</span>
                <span class="kind">{{ j.kind }}</span>
              </div>
              <div class="line2 num">
                <span>{{ label[j.priority] }}</span>
                @if (j.state === 'running') {
                  <span>{{ dur(now() - (j.startedAt ?? j.submittedAt)) }}</span>
                  @if (j.tokens) {
                    <span>{{ count(j.tokens) }} tok</span>
                  }
                } @else {
                  <span>waiting {{ dur(now() - j.submittedAt) }}</span>
                }
              </div>
              @if (j.progress !== null && j.state === 'running') {
                <div class="meter prog" role="progressbar" [attr.aria-valuenow]="pct(j)" aria-valuemin="0" aria-valuemax="100" [attr.aria-label]="'Progress of ' + appName(j.appId) + ' ' + j.kind">
                  <span [style.width.%]="pct(j)"></span>
                </div>
              }
            </div>
            <button type="button" class="btn ghost icon sm cancel" [attr.aria-label]="'Cancel ' + appName(j.appId) + ' ' + j.kind" matTooltip="Cancel request" [disabled]="live.isBusy()('job:' + j.id)" (click)="live.cancelJob(j.id)">
              <mat-icon svgIcon="pw:close" aria-hidden="true" />
            </button>
          </li>
        }
      </ul>
      @if (jobs().length > max()) {
        <p class="more">and {{ jobs().length - max() }} more waiting</p>
      }
    }
  `,
  styles: `
    :host {
      display: block;
    }
    /* The priority split is one muted line: "1 interactive · 1 background · 1 batch". */
    .depth {
      display: flex;
      flex-wrap: wrap;
      gap: 0 var(--pd-space-2);
      margin: 0 0 var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      span + span::before {
        content: '·';
        margin-right: var(--pd-space-2);
      }
    }
    .none {
      padding: var(--pd-space-3) 0;
      color: var(--pd-text-subtle);
      font-size: var(--pd-text-label);
    }
    .jobs {
      list-style: none;
      margin: 0;
      padding: 0;
      li {
        display: grid;
        grid-template-columns: auto minmax(0, 1fr) auto;
        align-items: center;
        gap: var(--pd-space-3);
        padding: 8px 0;
        border-top: 1px solid var(--pd-hairline);
        &:first-child {
          border-top: none;
        }
      }
    }
    /* Cancel is a quiet icon that appears with the pointer or keyboard focus, not a row of text buttons. */
    .cancel {
      opacity: 0;
      transition: opacity var(--pd-duration-micro) var(--pd-ease-move);
    }
    li:hover .cancel,
    li:focus-within .cancel,
    .cancel:focus-visible {
      opacity: 1;
    }
    @media (hover: none) {
      .cancel {
        opacity: 1;
      }
    }
    .job-dot {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      border: 1.5px solid var(--pd-text-subtle);
      &[data-state='running'] {
        border-color: var(--pd-accent);
        background: var(--pd-accent);
        box-shadow: 0 0 0 3px var(--pd-accent-tint);
      }
    }
    .line1 {
      display: flex;
      gap: var(--pd-space-2);
      align-items: baseline;
      .app {
        font-weight: var(--pd-weight-medium);
      }
      .kind {
        font-size: var(--pd-text-label);
        color: var(--pd-text-subtle);
      }
    }
    .line2 {
      display: flex;
      gap: var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-subtle);
      span + span::before {
        content: '·';
        margin-right: var(--pd-space-2);
      }
    }
    .more {
      padding-top: var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-subtle);
    }
    .prog {
      height: 3px;
      margin-top: 5px;
    }
  `,
})
export class QueueCard {
  readonly now = input.required<number>();
  /** Jobs listed; the rest are counted. */
  readonly max = input(4);
  protected readonly live = inject(LiveStore);
  protected readonly priorities = PRIORITIES;
  protected readonly label = PRIORITY_LABEL;
  protected readonly dur = duration;
  protected readonly count = compact;
  protected readonly jobs = computed<Job[]>(() => {
    const q = this.live.queue();
    return q ? [...q.running, ...q.waiting] : [];
  });
  private readonly names = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));

  protected appName(id: string): string {
    return this.names().get(id) ?? id;
  }

  protected pct(j: Job): number {
    return Math.round((j.progress ?? 0) * 100);
  }
}
