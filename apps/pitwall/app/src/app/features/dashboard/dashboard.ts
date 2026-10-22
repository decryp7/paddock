import { ChangeDetectionStrategy, Component, OnInit, computed, inject, isDevMode } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { EChartsCoreOption } from 'echarts/core';
import type { HistoryRange, Preset } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { latencyChart, memoryChart, stackedRequestsChart, throughputChart } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { duration } from '../../shared/format';
import { PRESET_HINT, PRESET_LABEL } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { Panel } from '../../shared/ui/panel';
import { AppsCard } from './apps-card';
import { DashboardStore } from './dashboard.store';
import { EnginesCard } from './engines-card';
import { EventsFeed } from './events-feed';
import { HotModels } from './hot-models';
import { PairingCard } from './pairing-card';
import { QueueCard } from './queue-card';
import { StatTiles } from './stat-tiles';

const RANGES: HistoryRange[] = ['1h', '24h', '7d', '30d'];
const PRESETS: Exclude<Preset, 'custom'>[] = ['lowMemory', 'balanced', 'performance'];

/** Rolling cost of building chart options per update (dev readout, `?perf`). Plain numbers: it runs inside computeds. */
export class PerfMeter {
  private samples: number[] = [];
  last = 0;
  count = 0;

  measure<T>(fn: () => T): T {
    const t0 = performance.now();
    const v = fn();
    this.last = performance.now() - t0;
    this.count++;
    this.samples = [...this.samples.slice(-59), this.last];
    return v;
  }

  get avg(): number {
    return this.samples.length ? this.samples.reduce((a, b) => a + b, 0) / this.samples.length : 0;
  }
}

/** Home: everything Pitwall is doing right now, how it got here, and the actions you need most. */
@Component({
  selector: 'pw-dashboard',
  imports: [MatIconModule, MatTooltipModule, Panel, EChart, StatTiles, HotModels, QueueCard, EnginesCard, EventsFeed, AppsCard, PairingCard],
  providers: [DashboardStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Dashboard</h1>
          <p class="sub">
            @if (live.server(); as s) {
              Pitwall {{ s.version }} · up {{ uptime() }} · {{ connectedApps() }} apps connected
              @if (s.offline) {
                · <span class="offline">Offline mode</span>
              }
            } @else {
              Connecting to the server…
            }
          </p>
        </div>
        <div class="actions">
          <div class="seg" role="radiogroup" aria-label="Memory preset">
            @for (p of presets; track p) {
              <button type="button" role="radio" [attr.aria-checked]="live.overview()?.preset === p" [matTooltip]="presetHint[p]" [disabled]="live.isBusy()('preset') || !live.overview()" (click)="live.applyPreset(p)">
                {{ presetLabel[p] }}
              </button>
            }
          </div>
          <button type="button" class="btn" [disabled]="live.isBusy()('release') || !live.overview()" (click)="live.releaseMemory()" matTooltip="Unload every model that is not pinned or busy">
            <mat-icon svgIcon="pw:release" aria-hidden="true" />Release memory
          </button>
          @if (live.paused()) {
            <button type="button" class="btn primary" [disabled]="live.isBusy()('serving')" (click)="live.setServing(false)"><mat-icon svgIcon="pw:play" aria-hidden="true" />Resume serving</button>
          } @else {
            <button type="button" class="btn" [disabled]="live.isBusy()('serving') || !live.overview()" (click)="live.setServing(true)"><mat-icon svgIcon="pw:pause" aria-hidden="true" />Pause serving</button>
          }
        </div>
      </header>

      @if (live.error() && !live.overview()) {
        <pw-panel heading="Server" icon="server" [error]="live.error()" [ready]="false" (retry)="live.load()" />
      }

      @if (live.paused()) {
        <div class="banner" role="status">
          <mat-icon svgIcon="pw:pause" aria-hidden="true" />
          <span><b>Serving is paused.</b> Apps can still connect; new requests wait in the queue until you resume.</span>
        </div>
      }

      @for (p of live.pairings(); track p.id) {
        <pw-pairing-card [request]="p" [now]="now()" />
      }

      <pw-stat-tiles />

      <div class="grid now">
        <pw-panel class="c5" heading="Loaded models" icon="models" [sub]="hotSummary()" [loading]="live.loading()" [ready]="!!live.overview()">
          <pw-hot-models [now]="now()" />
        </pw-panel>
        <pw-panel class="c4" heading="Queue" icon="queue" [sub]="queueSummary()" [loading]="live.loading()" [ready]="!!live.overview()">
          <pw-queue-card [now]="now()" />
        </pw-panel>
        <pw-panel class="c3" heading="Engines" icon="engines" [loading]="live.loading()" [ready]="!!live.overview()">
          <pw-engines-card />
        </pw-panel>
      </div>

      <div class="history-head">
        <h2 class="section">History</h2>
        <div class="seg small" role="radiogroup" aria-label="History range">
          @for (r of ranges; track r) {
            <button type="button" role="radio" [attr.aria-checked]="store.range() === r" (click)="store.select(r)">{{ r }}</button>
          }
        </div>
      </div>

      <div class="grid">
        <pw-panel class="c8" heading="Memory" icon="memory" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" [skeletonLines]="8" (retry)="store.select(store.range())">
          <div panel-actions class="legend" aria-hidden="true">
            <span><i class="sw" [style.background]="tokens().series[0]"></i>RAM</span>
            <span><i class="sw" [style.background]="tokens().series[1]"></i>VRAM</span>
            <span><i class="sw dash" [style.border-color]="tokens().series[2]"></i>Pitwall</span>
            <span class="sep"></span>
            <span><b [style.color]="tokens().accent">▲</b>load</span>
            <span><b [style.color]="tokens().warn">▼</b>idle</span>
            <span><b [style.color]="tokens().bad">▼</b>pressure</span>
            <span><b [style.color]="tokens().text2">▼</b>manual</span>
          </div>
          <pw-echart [option]="memoryOption()" [themeKey]="chartKey()" height="300px" label="RAM, VRAM and Pitwall memory over time, with model loads and releases marked" />
        </pw-panel>
        <pw-panel class="c4 events" heading="Events" icon="activity" [loading]="live.loading()" [ready]="!!live.overview()">
          <pw-events-feed [now]="now()" />
        </pw-panel>

        <pw-panel class="c7" heading="Apps" icon="apps" sub="today" [loading]="live.loading()" [ready]="!!live.overview()">
          <pw-apps-card [series]="store.history()?.requestsByApp ?? []" />
        </pw-panel>
        <pw-panel class="c5" heading="Requests by app" icon="requests" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="byAppOption()" [themeKey]="chartKey()" height="226px" label="Requests per interval by app" />
        </pw-panel>

        <pw-panel class="c4" heading="Tokens per second" icon="bolt" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="tpsOption()" [themeKey]="chartKey()" height="220px" label="Generation and prompt tokens per second over time" />
        </pw-panel>
        <pw-panel class="c4" heading="Time to first token" icon="clock" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="latencyOption()" [themeKey]="chartKey()" height="220px" label="Time to first token, p50 and p95, over time" />
        </pw-panel>
        <pw-panel class="c4" heading="Requests by model" icon="models" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="byModelOption()" [themeKey]="chartKey()" height="220px" label="Requests per interval by model" />
        </pw-panel>
      </div>

      @if (showPerf) {
        <div class="perf" aria-hidden="true">{{ perfText() }}</div>
      }
    </div>
  `,
  styles: `
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
    }
    .page-head {
      margin-bottom: var(--sp-2);
    }
    .offline {
      color: var(--warn);
      font-weight: var(--fw-medium);
    }
    .seg {
      display: inline-flex;
      padding: 2px;
      background: var(--bg-active);
      border-radius: var(--r-md);
      button {
        height: 24px;
        padding: 0 var(--sp-3);
        font: inherit;
        font-size: var(--fs-sm);
        font-weight: var(--fw-medium);
        color: var(--text-2);
        background: none;
        border: none;
        border-radius: var(--r-sm);
        cursor: pointer;
        transition: background var(--dur-fast) var(--ease);
        &:hover:not(:disabled) {
          color: var(--text);
        }
        &[aria-checked='true'] {
          color: var(--text);
          background: var(--bg-card);
          box-shadow: var(--shadow-1);
        }
        &:disabled {
          cursor: default;
        }
      }
      &.small button {
        height: 22px;
        min-width: 40px;
        padding: 0 var(--sp-2);
      }
    }
    .banner {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--warn-soft);
      border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--warn);
      }
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(12, minmax(0, 1fr));
      gap: var(--sp-3);
    }
    .c3 {
      grid-column: span 3;
    }
    .c4 {
      grid-column: span 4;
    }
    .c5 {
      grid-column: span 5;
    }
    .c7 {
      grid-column: span 7;
    }
    .c8 {
      grid-column: span 8;
    }
    .events {
      max-height: 383px;
    }
    .history-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-top: var(--sp-4);
      .section {
        font-size: var(--fs-md);
      }
    }
    .legend {
      display: flex;
      align-items: center;
      gap: var(--sp-3);
      font-size: var(--fs-xs);
      color: var(--text-2);
      span {
        display: inline-flex;
        align-items: center;
        gap: 5px;
      }
      b {
        font-size: 9px;
      }
      .sw {
        width: 10px;
        height: 3px;
        border-radius: 2px;
        &.dash {
          height: 0;
          border-top: 2px dashed;
        }
      }
      .sep {
        width: 1px;
        height: 12px;
        background: var(--border);
      }
    }
    .perf {
      position: fixed;
      right: var(--sp-3);
      bottom: var(--sp-3);
      padding: 2px var(--sp-2);
      font: 11px var(--font-mono);
      color: var(--text-2);
      background: var(--bg-raised);
      border: 1px solid var(--border);
      border-radius: var(--r-sm);
    }
    @media (max-width: 1200px) {
      .c3,
      .c4,
      .c5,
      .c7,
      .c8 {
        grid-column: span 6;
      }
      .events {
        grid-column: span 6;
      }
    }
  `,
})
export class Dashboard implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly store = inject(DashboardStore);
  private readonly theme = inject(ThemeService);
  protected readonly now = injectNow();
  protected readonly ranges = RANGES;
  protected readonly presets = PRESETS;
  protected readonly presetLabel = PRESET_LABEL;
  protected readonly presetHint = PRESET_HINT;
  protected readonly tokens = this.theme.tokens;
  protected readonly perf = new PerfMeter();
  protected readonly showPerf = isDevMode() && typeof location !== 'undefined' && new URLSearchParams(location.search).has('perf');

  /** Replaces the chart option on a theme or range switch; merges otherwise. */
  protected readonly chartKey = computed(() => `${this.theme.resolved()}:${this.store.range()}`);

  protected readonly memoryOption = computed<EChartsCoreOption>(() =>
    this.perf.measure(() => {
      const h = this.store.history();
      return memoryChart({ points: this.store.points(), events: this.store.chartEvents(), range: this.store.range(), stepMs: h?.stepMs ?? 60_000, zoom: true }, this.tokens());
    }),
  );
  protected readonly tpsOption = computed(() => throughputChart(this.store.points(), this.store.range(), this.tokens()));
  protected readonly latencyOption = computed(() => latencyChart(this.store.history()?.points ?? [], this.store.range(), this.tokens()));
  private readonly appNames = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));
  protected readonly byAppOption = computed(() => {
    const h = this.store.history();
    const names = this.appNames();
    return stackedRequestsChart(h?.points ?? [], h?.requestsByApp ?? [], this.store.range(), this.tokens(), (k) => names.get(k) ?? k);
  });
  protected readonly byModelOption = computed(() => {
    const h = this.store.history();
    const names = new Map(this.live.models().map((m) => [m.modelId, m.name]));
    return stackedRequestsChart(h?.points ?? [], h?.requestsByModel ?? [], this.store.range(), this.tokens(), (k) => names.get(k) ?? shortModel(k));
  });

  /** Refreshed with the clock tick, not by the measurement itself. */
  protected readonly perfText = computed(() => (this.now(), `charts ${this.perf.last.toFixed(2)} ms · avg ${this.perf.avg.toFixed(2)} ms · ${this.perf.count} builds · ${this.live.updates()} updates`));

  protected readonly uptime = computed(() => {
    const s = this.live.server();
    return s ? duration(this.now() - s.startedAt) : '';
  });
  protected readonly connectedApps = computed(() => this.live.apps().filter((a) => a.connected).length);
  protected readonly hotSummary = computed(() => {
    const ms = this.live.models();
    if (!ms.length) return '';
    const vram = ms.reduce((s, m) => s + m.vramBytes, 0);
    const ram = ms.reduce((s, m) => s + m.ramBytes, 0);
    return `${ms.length} hot · ${(vram / 1024 ** 3).toFixed(1)} GB VRAM · ${(ram / 1024 ** 3).toFixed(1)} GB RAM`;
  });
  protected readonly queueSummary = computed(() => {
    const q = this.live.queue();
    return q ? `${q.running.length} running · ${q.waiting.length} waiting` : '';
  });

  ngOnInit(): void {
    void this.store.select('1h');
  }
}

/** A readable name for a model id the library does not know any more (`gemma-3-12b-it-q4_k_m` → `gemma-3-12b-it`). */
export function shortModel(id: string): string {
  return id.replace(/[-.](q\d\w*|f16|bf16|f32)$/i, '');
}
