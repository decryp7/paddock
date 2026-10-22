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
      <div class="depth" role="list" aria-label="Waiting by priority">
        @for (p of priorities; track p) {
          <div class="d" role="listitem">
            <span class="n num">{{ q.depth[p] }}</span>
            <span class="k"><i [attr.data-p]="p"></i>{{ label[p] }}</span>
          </div>
        }
      </div>
      @if (jobs().length === 0) {
        <p class="none">Nothing running or waiting.</p>
      }
      <ul class="jobs" aria-label="Jobs">
        @for (j of jobs().slice(0, max()); track j.id) {
          <li>
            <span class="status" [attr.data-state]="j.state" [attr.aria-label]="j.state === 'running' ? 'Running' : 'Waiting'"></span>
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
    .depth {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 1px;
      margin-bottom: var(--sp-3);
      background: var(--border-soft);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
      overflow: hidden;
      .d {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: var(--sp-2) var(--sp-3);
        background: var(--bg-card);
      }
      .n {
        font-size: var(--fs-xl);
        font-weight: var(--fw-semibold);
        line-height: 1.2;
      }
      .k {
        display: flex;
        align-items: center;
        gap: 5px;
        font-size: var(--fs-xs);
        color: var(--text-3);
      }
      i {
        width: 6px;
        height: 6px;
        border-radius: 2px;
        background: var(--accent);
        &[data-p='background'] {
          background: color-mix(in srgb, var(--accent) 55%, var(--text-3));
        }
        &[data-p='batch'] {
          background: var(--text-3);
        }
      }
    }
    .none {
      padding: var(--sp-3) 0;
      color: var(--text-3);
      font-size: var(--fs-sm);
    }
    .jobs {
      list-style: none;
      margin: 0;
      padding: 0;
      li {
        display: grid;
        grid-template-columns: auto minmax(0, 1fr) auto;
        align-items: center;
        gap: var(--sp-3);
        padding: 8px 0;
        border-top: 1px solid var(--border-soft);
        &:first-child {
          border-top: none;
        }
      }
    }
    /* Cancel is a quiet icon that appears with the pointer or keyboard focus, not a row of text buttons. */
    .cancel {
      opacity: 0;
      transition: opacity var(--dur-fast) var(--ease);
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
    .status {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      border: 1.5px solid var(--text-3);
      &[data-state='running'] {
        border-color: var(--accent);
        background: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-soft);
      }
    }
    .line1 {
      display: flex;
      gap: var(--sp-2);
      align-items: baseline;
      .app {
        font-weight: var(--fw-medium);
      }
      .kind {
        font-size: var(--fs-xs);
        color: var(--text-3);
      }
    }
    .line2 {
      display: flex;
      gap: var(--sp-2);
      font-size: var(--fs-xs);
      color: var(--text-3);
      span + span::before {
        content: '·';
        margin-right: var(--sp-2);
      }
    }
    .more {
      padding-top: var(--sp-2);
      font-size: var(--fs-xs);
      color: var(--text-3);
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
