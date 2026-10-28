import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { LiveStore } from '../../core/live.store';
import { bytes, gb, int, percent, rate } from '../../shared/format';
import { loadTone, pressureLook } from '../../shared/labels';
import { Skeleton } from '../../shared/ui/skeleton';

/** The four live tiles at the top of the dashboard: memory, GPU memory, processors and throughput. */
@Component({
  selector: 'pw-stat-tiles',
  imports: [MatIconModule, MatTooltipModule, Skeleton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="tile" aria-labelledby="t-ram">
      <div class="head">
        <span id="t-ram" class="label"><mat-icon svgIcon="pw:memory" aria-hidden="true" />System memory</span>
        @if (pressure(); as p) {
          @if (p.tone !== 'ok') {
            <span class="status" [attr.data-tone]="p.tone">{{ p.label }}</span>
          }
        }
      </div>
      @if (live.memory(); as m) {
        <div class="value num">{{ gbText(m.ramTotal - m.ramAvailable, 'memory') }}<span class="unit">GB</span><span class="of">of {{ gbText(m.ramTotal, 'memory', 0) }} GB</span></div>
        <div class="stack" role="img" [attr.aria-label]="ramPct() + ' percent used, ' + ownPct() + ' percent by Pitwall'">
          <span class="seg own" [style.width.%]="ownPct()"></span>
          <span class="seg other" [style.width.%]="ramPct() - ownPct()"></span>
        </div>
        <div class="foot">
          <span><i class="key own"></i>Pitwall <b class="num">{{ bytesText(m.pitwallRss) }}</b></span>
          <span class="faint num">{{ bytesText(m.ramAvailable) }} free</span>
        </div>
      } @else {
        <pw-skeleton [lines]="3" />
      }
    </section>

    <section class="tile" aria-labelledby="t-vram">
      <div class="head">
        <span id="t-vram" class="label"><mat-icon svgIcon="pw:gpu" aria-hidden="true" />GPU memory</span>
        @if (gpu(); as g) {
          <span class="gpu-name" [matTooltip]="g.name">{{ shortGpu(g.name) }}</span>
        }
      </div>
      @if (gpu(); as g) {
        @if (g.vramTotal !== null) {
          <div class="value num">{{ gbText(g.vramUsed, 'memory') }}<span class="unit">GB</span><span class="of">of {{ gbText(g.vramTotal, 'memory', 0) }} GB</span></div>
          <div class="meter" [attr.data-tone]="tone(vramPct())"><span [style.width.%]="vramPct()"></span></div>
          <div class="foot">
            <span>Models <b class="num">{{ bytesText(modelVram()) }}</b></span>
            <span class="faint num">{{ bytesText(g.vramTotal - (g.vramUsed ?? 0)) }} free</span>
          </div>
        } @else {
          <div class="value small">{{ g.unified ? 'Unified memory' : 'Not reported' }}</div>
          <p class="faint">{{ g.unified ? 'The GPU shares system memory.' : 'The driver does not report VRAM.' }}</p>
        }
      } @else if (live.memory()) {
        <div class="value small">No GPU</div>
        <p class="faint">Models run on the CPU.</p>
      } @else {
        <pw-skeleton [lines]="3" />
      }
    </section>

    <section class="tile" aria-labelledby="t-proc">
      <div class="head">
        <span id="t-proc" class="label"><mat-icon svgIcon="pw:cpu" aria-hidden="true" />Processors</span>
        @if (live.memory()?.foregroundBusy) {
          <span class="status" data-tone="warn" matTooltip="A full-screen app is in front: Pitwall yields">Yielding</span>
        }
      </div>
      @if (live.memory(); as m) {
        <div class="value num">{{ heroLoad() }}<span class="unit">%</span><span class="of">{{ gpuLoad() === null ? 'CPU' : 'GPU' }}</span></div>
        @if (gpuLoad() !== null) {
          <div class="bar-row">
            <span class="bar-label">CPU</span>
            <div class="meter" [attr.data-tone]="tone(m.cpuPercent)"><span [style.width.%]="m.cpuPercent"></span></div>
            <span class="bar-val num">{{ round(m.cpuPercent) }}%</span>
          </div>
        } @else {
          <div class="meter" [attr.data-tone]="tone(m.cpuPercent)"><span [style.width.%]="m.cpuPercent"></span></div>
        }
        <div class="foot">
          <span>Commit <b class="num">{{ commit() }}</b></span>
          <span class="faint num">swap {{ bytesText(m.swapUsed) }}</span>
        </div>
      } @else {
        <pw-skeleton [lines]="3" />
      }
    </section>

    <section class="tile" aria-labelledby="t-tps">
      <div class="head">
        <span id="t-tps" class="label"><mat-icon svgIcon="pw:bolt" aria-hidden="true" />Throughput</span>
        @if (live.paused()) {
          <span class="status" data-tone="warn">Paused</span>
        } @else if (live.throughput(); as t) {
          @if (t.activeRequests) {
            <span class="gpu-name num">{{ t.activeRequests }} active</span>
          }
        }
      </div>
      @if (live.throughput(); as t) {
        <div class="value num">{{ rateText(t.genTps) }}<span class="unit">tok/s</span><span class="of">generation</span></div>
        <div class="trio">
          <div><span class="k">Prompt</span><b class="num">{{ intText(t.promptTps) }}</b><span class="u">tok/s</span></div>
          <div><span class="k">TTFT p50</span><b class="num">{{ intText(t.ttftP50Ms) }}</b><span class="u">ms</span></div>
          <div><span class="k">Requests</span><b class="num">{{ intText(t.requestsPerMin) }}</b><span class="u">/min</span></div>
        </div>
      } @else {
        <pw-skeleton [lines]="3" />
      }
    </section>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: var(--sp-3);
    }
    .tile {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
      min-width: 0;
      padding: var(--sp-4);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-1), var(--card-highlight);
    }
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-2);
      min-height: 20px;
    }
    .label {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      font-size: var(--fs-sm);
      font-weight: var(--fw-medium);
      color: var(--text-2);
      white-space: nowrap;
      mat-icon {
        width: 15px;
        height: 15px;
        color: var(--text-3);
      }
    }
    .gpu-name {
      font-size: var(--fs-sm);
      color: var(--text-2);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .value {
      display: flex;
      align-items: baseline;
      gap: 3px;
      font-size: var(--fs-3xl);
      font-weight: var(--fw-semibold);
      letter-spacing: -0.02em;
      line-height: 1;
      font-variant-numeric: tabular-nums;
      .unit {
        font-size: var(--fs-base);
        font-weight: var(--fw-medium);
        color: var(--text-3);
        letter-spacing: 0;
      }
      .of {
        margin-left: var(--sp-2);
        font-size: var(--fs-base);
        font-weight: var(--fw-regular);
        color: var(--text-3);
        letter-spacing: 0;
      }
      &.small {
        font-size: var(--fs-xl);
      }
    }
    .stack {
      display: flex;
      gap: 2px;
      height: 6px;
      border-radius: var(--r-full);
      background: var(--track);
      overflow: hidden;
      .seg {
        height: 100%;
        transition: width var(--dur-slow) var(--ease);
      }
      .own {
        background: var(--accent);
        border-radius: var(--r-full) 0 0 var(--r-full);
      }
      .other {
        background: color-mix(in srgb, var(--text-3) 55%, transparent);
      }
    }
    .foot {
      display: flex;
      justify-content: space-between;
      gap: var(--sp-2);
      font-size: var(--fs-sm);
      color: var(--text-2);
      white-space: nowrap;
      b {
        font-weight: var(--fw-medium);
        color: var(--text);
      }
    }
    .key {
      display: inline-block;
      width: 8px;
      height: 8px;
      margin-right: 6px;
      border-radius: 2px;
      &.own {
        background: var(--accent);
      }
    }
    .bars {
      display: grid;
      gap: 10px;
      padding: 3px 0 2px;
    }
    .bar-row {
      display: grid;
      grid-template-columns: 34px 1fr 40px;
      align-items: center;
      gap: var(--sp-2);
    }
    .bar-label {
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .bar-val {
      font-size: var(--fs-base);
      font-weight: var(--fw-medium);
      text-align: right;
    }
    .trio {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: var(--sp-2);
      > div {
        display: grid;
        grid-template-columns: auto 1fr;
        align-items: baseline;
        column-gap: 3px;
        min-width: 0;
      }
      .k {
        grid-column: 1 / -1;
        font-size: var(--fs-xs);
        color: var(--text-3);
      }
      b {
        font-size: var(--fs-md);
        font-weight: var(--fw-semibold);
      }
      .u {
        font-size: var(--fs-xs);
        color: var(--text-3);
        white-space: nowrap;
      }
    }
    @media (max-width: 1100px) {
      :host {
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
    }
  `,
})
export class StatTiles {
  protected readonly live = inject(LiveStore);
  protected readonly gbText = gb;
  protected readonly bytesText = (n: number | null | undefined): string => bytes(n, 'memory');
  protected readonly rateText = rate;
  protected readonly intText = int;
  protected readonly round = Math.round;
  protected readonly tone = loadTone;

  protected readonly pressure = computed(() => {
    const m = this.live.memory();
    return m ? pressureLook(m.pressure) : null;
  });
  protected readonly ramPct = computed(() => {
    const m = this.live.memory();
    return m ? percent(m.ramTotal - m.ramAvailable, m.ramTotal) : 0;
  });
  protected readonly ownPct = computed(() => {
    const m = this.live.memory();
    return m ? Math.min(this.ramPct(), percent(m.pitwallRss, m.ramTotal)) : 0;
  });
  protected readonly gpu = computed(() => this.live.memory()?.gpus[0] ?? null);
  /** The processors tile's hero: the GPU's load when the GPU reports it, else the CPU's. */
  protected readonly gpuLoad = computed(() => this.gpu()?.utilisation ?? null);
  protected readonly heroLoad = computed(() => {
    const g = this.gpuLoad();
    return Math.round(g ?? this.live.memory()?.cpuPercent ?? 0);
  });
  protected readonly vramPct = computed(() => {
    const g = this.gpu();
    return g && g.vramTotal ? percent(g.vramUsed ?? 0, g.vramTotal) : 0;
  });
  protected readonly modelVram = computed(() => this.live.models().reduce((s, m) => s + m.vramBytes, 0));
  protected readonly commit = computed(() => {
    const m = this.live.memory();
    return m && m.commitUsed !== null && m.commitLimit !== null ? `${percent(m.commitUsed, m.commitLimit)}%` : '—';
  });

  protected shortGpu(name: string): string {
    return name.replace(/^NVIDIA GeForce |^AMD Radeon |^Intel\(R\) /, '');
  }
}
