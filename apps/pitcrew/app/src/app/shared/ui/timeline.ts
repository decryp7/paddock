import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import type { Mark, MarkKind, Span } from '../../core/api.types';
import { formatDateTime, formatPermille } from '../format';
import { spanBars, timeTicks, timeX } from '../geometry';
import { markLook } from '../labels';

/** One runner's row: when it was up, when it was busy, and what happened. */
export interface TimelineRow {
  name: string;
  up: Span[];
  busy: Span[];
  marks: Mark[];
  uptimePermille: number | null;
}

type MarkShape = 'restart' | 'update' | 'problem';

function shape(kind: MarkKind): MarkShape {
  if (kind === 'updated' || kind === 'rolledBack' || kind === 'updateFailed' || kind === 'updateSkipped') return 'update';
  if (kind === 'crashLoop' || kind === 'launchFailed') return 'problem';
  return 'restart';
}

/**
 * A compact uptime timeline per runner: up spans, busy spans on top, and marks for restarts, stalls and updates.
 * Positions are percentages, so it scales with its container. Each row has a text summary for screen readers.
 */
@Component({
  selector: 'pc-timeline',
  imports: [MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="rows">
      @for (row of layout(); track row.name) {
        <div class="row">
          <div class="name truncate" [title]="row.name">{{ row.name }}</div>
          <div class="track" role="img" [attr.aria-label]="row.summary">
            @for (b of row.up; track $index) {
              <span class="up" [style.left.%]="b.x" [style.width.%]="b.w"></span>
            }
            @for (b of row.busy; track $index) {
              <span class="busy" [style.left.%]="b.x" [style.width.%]="b.w"></span>
            }
            @for (m of row.marks; track $index) {
              <span class="mark" [class]="m.shape + ' ' + m.tone" [style.left.%]="m.x" [matTooltip]="m.label" aria-hidden="true"></span>
            }
          </div>
          <div class="uptime num">{{ row.uptime }}</div>
        </div>
      }
      <div class="row axis" aria-hidden="true">
        <div></div>
        <div class="ticks">
          @for (t of ticks(); track t.x) {
            <span [style.left.%]="t.x">{{ t.label }}</span>
          }
        </div>
        <div></div>
      </div>
    </div>
    <ul class="legend" aria-label="Legend">
      <li><span class="key up"></span>Up</li>
      <li><span class="key down"></span>Down</li>
      <li><span class="key busy"></span>Running a job</li>
      <li><span class="key-mark restart warning"></span>Restart (stall or crash)</li>
      <li><span class="key-mark problem danger"></span>Crash loop</li>
      <li><span class="key-mark update success"></span>Update</li>
    </ul>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-3);
    }
    .rows {
      display: grid;
      gap: var(--space-2);
    }
    .row {
      display: grid;
      grid-template-columns: 96px 1fr 52px;
      align-items: center;
      gap: var(--space-3);
    }
    .name {
      font-size: var(--text-sm);
      color: var(--text-2);
    }
    .uptime {
      font-size: var(--text-sm);
      color: var(--text-2);
      text-align: right;
    }
    .track {
      position: relative;
      height: 18px;
      border-radius: var(--radius-xs);
      background: color-mix(in srgb, var(--danger) 9%, var(--surface-2));
    }
    .up,
    .busy {
      position: absolute;
      top: 0;
      bottom: 0;
    }
    .up {
      background: var(--surface-3);
    }
    .busy {
      top: 4px;
      bottom: 4px;
      background: var(--chart-1);
      opacity: 0.85;
    }
    .mark {
      position: absolute;
      top: -3px;
      width: 8px;
      height: 8px;
      margin-left: -4px;
      background: currentColor;
      box-shadow: 0 0 0 2px var(--surface);
    }
    .mark.restart {
      clip-path: polygon(50% 100%, 0 0, 100% 0);
    }
    .mark.problem {
      border-radius: 1px;
      top: -4px;
      width: 9px;
      height: 9px;
      margin-left: -4.5px;
    }
    .mark.update {
      top: auto;
      bottom: -3px;
      border-radius: 50%;
    }
    .warning {
      color: var(--warning);
    }
    .danger {
      color: var(--danger);
    }
    .success {
      color: var(--success);
    }
    .neutral {
      color: var(--neutral);
    }
    .info {
      color: var(--info);
    }
    .axis .ticks {
      position: relative;
      height: 14px;
    }
    .ticks span {
      position: absolute;
      transform: translateX(-50%);
      font-size: var(--text-xs);
      color: var(--text-3);
      white-space: nowrap;
    }
    .ticks span:first-child {
      transform: none;
    }
    .legend {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-4);
      margin: 0;
      padding: 0;
      list-style: none;
      font-size: var(--text-sm);
      color: var(--text-2);
    }
    .legend li {
      display: inline-flex;
      align-items: center;
      gap: 6px;
    }
    .key {
      display: inline-block;
      width: 14px;
      height: 10px;
      border-radius: 2px;
    }
    .key.up {
      position: static;
      background: var(--surface-3);
    }
    .key.down {
      background: color-mix(in srgb, var(--danger) 9%, var(--surface-2));
      box-shadow: inset 0 0 0 1px var(--border);
    }
    .key.busy {
      position: static;
      background: var(--chart-1);
    }
    .key-mark {
      display: inline-block;
      width: 9px;
      height: 9px;
      background: currentColor;
    }
    .key-mark.restart {
      clip-path: polygon(50% 100%, 0 0, 100% 0);
    }
    .key-mark.update {
      border-radius: 50%;
    }
  `,
})
export class Timeline {
  readonly rows = input.required<TimelineRow[]>();
  readonly from = input.required<number>();
  readonly to = input.required<number>();

  protected readonly layout = computed(() => {
    const from = this.from();
    const to = this.to();
    return this.rows().map((r) => {
      const marks = r.marks
        .filter((m) => m.kind !== 'cleanup')
        .map((m) => {
          const look = markLook(m.kind);
          return { x: timeX(m.at, from, to, 100), shape: shape(m.kind), tone: look.tone, label: `${look.label} · ${formatDateTime(m.at)}` };
        })
        .filter((m): m is typeof m & { x: number } => m.x !== null);
      const restarts = marks.filter((m) => m.shape !== 'update').length;
      const updates = marks.length - restarts;
      const uptime = r.uptimePermille === null ? '—' : formatPermille(r.uptimePermille);
      return {
        name: r.name,
        up: spanBars(r.up, from, to, 100),
        busy: spanBars(r.busy, from, to, 100),
        marks,
        uptime,
        summary: `${r.name}: up ${uptime} of the time, ${restarts} restarts or problems, ${updates} updates.`,
      };
    });
  });

  protected readonly ticks = computed(() => timeTicks(this.from(), this.to(), 100, 7));
}
