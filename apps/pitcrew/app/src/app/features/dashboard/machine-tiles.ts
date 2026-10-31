import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { MachineSample, ServiceInfo } from '../../core/api.types';
import { formatBytes, formatDuration, formatPermille } from '../../shared/format';
import { SuSparkline } from '@decryp7/paddock-ui';

interface Tile {
  key: string;
  icon: string;
  label: string;
  value: string;
  sub: string;
  values: number[];
  max: number | undefined;
  tone: 'chart-1' | 'chart-3' | 'chart-4' | 'warning';
}

/**
 * What the runners use: CPU and memory of the runner processes and everything they started (ADR 0007), with trends;
 * the machine's free disk; what Pitcrew itself costs. A muted line gives the machine's totals for context.
 */
@Component({
  selector: 'pc-machine-tiles',
  imports: [MatIcon, SuSparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (t of tiles(); track t.key) {
      <section class="card tile" [attr.aria-label]="t.label + ': ' + t.value + ', ' + t.sub">
        <div class="top">
          <span class="label"><mat-icon [svgIcon]="'pc:' + t.icon" aria-hidden="true" />{{ t.label }}</span>
        </div>
        <div class="value num">{{ t.value }}</div>
        <div class="sub faint">{{ t.sub }}</div>
        <pd-sparkline class="spark" [style.color]="'var(--' + t.tone + ')'" [label]="t.label" [values]="t.values" [min]="0" [max]="t.max" [area]="true" [height]="34" />
      </section>
    }
    <section class="card tile own" aria-label="Pitcrew's own cost">
      <div class="top">
        <span class="label"><mat-icon svgIcon="pc:gauge" aria-hidden="true" />Pitcrew itself</span>
      </div>
      <div class="value num">{{ own().cpu }} <span class="unit">CPU</span></div>
      <div class="sub faint">{{ own().mem }} memory · up {{ own().up }}</div>
      <div class="note faint">The service and this window together.</div>
    </section>
    <p class="context faint" data-testid="machine-context">{{ context() }}</p>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: var(--space-4);
    }
    .tile {
      display: grid;
      grid-template-rows: auto auto auto 1fr;
      gap: 2px;
      padding: var(--space-3) var(--space-4) var(--space-3);
      overflow: hidden;
    }
    .label {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      color: var(--text-2);
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
    }
    .label mat-icon {
      width: 15px;
      height: 15px;
      color: var(--text-3);
    }
    .value {
      font-family: var(--font-display);
      font-size: var(--text-2xl);
      font-weight: var(--weight-semibold);
      font-variant-numeric: tabular-nums;
      letter-spacing: -0.01em;
      margin-top: var(--space-1);
    }
    .unit {
      font-size: var(--text-md);
      color: var(--text-3);
      font-weight: var(--weight-medium);
    }
    .sub {
      font-size: var(--text-sm);
    }
    .spark {
      height: 34px;
      margin-top: var(--space-2);
      align-self: end;
    }
    .context {
      grid-column: 1 / -1;
      margin: calc(var(--space-1) * -1) 0 0;
      font-size: var(--text-sm);
    }
    .note {
      align-self: end;
      font-size: var(--text-xs);
      margin-top: var(--space-2);
    }
  `,
})
export class MachineTiles {
  readonly samples = input.required<MachineSample[]>();
  readonly service = input.required<ServiceInfo | null>();
  readonly now = input.required<number>();

  protected readonly tiles = computed<Tile[]>(() => {
    const s = this.samples();
    const last = s.at(-1) ?? this.service()?.machine ?? null;
    const run = s.map((x) => x.runners);
    const peak = run.reduce((a, x) => Math.max(a, x.cpuPeakPermille), 0);
    const avg = run.length ? run.reduce((a, x) => a + x.cpuPermille, 0) / run.length : 0;
    const memPeak = run.reduce((a, x) => Math.max(a, x.memPeakBytes), 0);
    const memShare = last ? Math.round((last.runners.memBytes / Math.max(1, last.memTotalBytes)) * 1000) : null;
    const freeShare = last ? Math.round((last.diskFreeBytes / Math.max(1, last.diskTotalBytes)) * 1000) : null;
    return [
      {
        key: 'cpu',
        icon: 'cpu',
        label: 'Runners CPU',
        value: last ? formatPermille(last.runners.cpuPermille) : '—',
        sub: s.length ? `avg ${formatPermille(avg)} · peak ${formatPermille(peak)}` : 'No samples yet',
        values: run.map((x) => x.cpuPermille),
        max: 1000,
        tone: 'chart-1',
      },
      {
        key: 'mem',
        icon: 'memory',
        label: 'Runners memory',
        value: last ? formatBytes(last.runners.memBytes) : '—',
        sub: last ? `${formatPermille(memShare)} of ${formatBytes(last.memTotalBytes, 0)} · peak ${formatBytes(memPeak)}` : 'No samples yet',
        values: run.map((x) => x.memBytes),
        max: last?.memTotalBytes,
        tone: 'chart-3',
      },
      {
        key: 'disk',
        icon: 'disk',
        label: 'Free disk',
        value: last ? formatBytes(last.diskFreeBytes) : '—',
        sub: last ? `of ${formatBytes(last.diskTotalBytes, 0)} · ${formatPermille(freeShare)} free` : 'No samples yet',
        values: s.map((x) => x.diskFreeBytes),
        max: undefined,
        tone: freeShare !== null && freeShare < 100 ? 'warning' : 'chart-4',
      },
    ];
  });

  /** The whole machine, for context: the runners are a part of it. */
  protected readonly context = computed(() => {
    const last = this.samples().at(-1) ?? this.service()?.machine ?? null;
    if (!last) return 'The machine’s own totals appear once samples arrive.';
    return `This machine: ${formatPermille(last.cpuPermille)} CPU · ${formatBytes(last.memUsedBytes)} of ${formatBytes(last.memTotalBytes, 0)} memory in use, runners included`;
  });

  protected readonly own = computed(() => {
    const svc = this.service();
    return {
      cpu: svc ? formatPermille(svc.ownCpuPermille) : '—',
      mem: svc ? formatBytes(svc.ownMemBytes) : '—',
      up: svc ? formatDuration(this.now() - svc.startedAt) : '—',
    };
  });
}
