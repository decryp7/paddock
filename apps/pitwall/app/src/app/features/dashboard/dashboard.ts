import { ChangeDetectionStrategy, Component, OnInit, computed, inject, isDevMode } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatIconModule } from '@angular/material/icon';
import { MatMenuTrigger } from '@angular/material/menu';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { EChartsCoreOption } from 'echarts/core';
import type { HistoryRange, Preset } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { latencyChart, memoryChart, stackedLegend, stackedRequestsChart, throughputChart } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { duration } from '../../shared/format';
import { PRESET_HINT, PRESET_LABEL } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { ItemsMenu, type MenuItem } from '../../shared/ui/menu';
import { Panel } from '../../shared/ui/panel';
import { UsageCard } from '../usage/usage-card';
import { AppsCard } from './apps-card';
import { DashboardStore } from './dashboard.store';
import { EngineSetupBanner } from '../engines/engine-setup-banner';
import { EngineIssues } from './engine-issues';
import { EnginesCard } from './engines-card';
import { EventsFeed } from './events-feed';
import { HotModels } from './hot-models';
import { PairingCard } from './pairing-card';
import { UpdateBanner } from '../updates/update-banner';
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
  imports: [MatIconModule, MatTooltipModule, MatButton, MatIconButton, MatButtonToggleGroup, MatButtonToggle, MatMenuTrigger, SuSegmentedControl, ItemsMenu, Panel, EChart, StatTiles, HotModels, QueueCard, EngineIssues, EngineSetupBanner, EnginesCard, EventsFeed, AppsCard, PairingCard, UsageCard, UpdateBanner],
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
          <mat-button-toggle-group hideSingleSelectionIndicator aria-label="Memory preset" [value]="live.overview()?.preset" [disabled]="live.isBusy()('preset') || !live.overview()" (change)="live.applyPreset($event.value)">
            @for (p of presets; track p) {
              <mat-button-toggle [value]="p" [matTooltip]="presetHint[p]">{{ presetLabel[p] }}</mat-button-toggle>
            }
          </mat-button-toggle-group>
          @if (live.paused()) {
            <button type="button" matButton="filled" [disabled]="live.isBusy()('serving')" (click)="live.setServing(false)"><mat-icon svgIcon="pw:play" aria-hidden="true" />Resume serving</button>
          }
          <button type="button" matIconButton aria-label="More actions" matTooltip="More actions" [matMenuTriggerFor]="moreMenu.menu()" [matMenuTriggerData]="{ items: moreItems() }"><mat-icon svgIcon="pw:more" aria-hidden="true" /></button>
          <pw-items-menu #moreMenu label="Dashboard actions" />
        </div>
      </header>

      @if (live.error() && !live.overview()) {
        <pw-panel heading="Server" icon="server" [error]="live.error()" [ready]="false" (retry)="live.load()" />
      }

      @if (live.paused()) {
        <div class="callout" data-tone="warn" role="status">
          <mat-icon svgIcon="pw:pause" aria-hidden="true" />
          <span><b>Serving is paused.</b> Apps can still connect; new requests wait in the queue until you resume.</span>
        </div>
      }

      <pw-engine-setup />
      <pw-engine-issues [now]="now()" />

      @for (p of live.pairings(); track p.id) {
        <pw-pairing-card [request]="p" [now]="now()" />
      }

      <pw-update-banner mode="summary" />

      @if (!live.error() || live.overview()) {
        <pw-stat-tiles />

        <div class="grid now">
          <pw-panel class="c4" heading="Loaded models" icon="models" [sub]="hotSummary()" [loading]="live.loading()" [ready]="!!live.overview()">
            <pw-hot-models [now]="now()" />
          </pw-panel>
          <pw-panel class="c4" heading="Queue" icon="queue" [sub]="queueSummary()" [loading]="live.loading()" [ready]="!!live.overview()">
            <pw-queue-card [now]="now()" />
          </pw-panel>
          <pw-panel class="c4" heading="Engines" icon="engines" [loading]="live.loading()" [ready]="!!live.overview()">
            <pw-engines-card />
          </pw-panel>
        </div>
      }

      <div class="history-head">
        <h2 class="section">History</h2>
        <pd-segmented-control label="History range" [options]="rangeSegs" [value]="store.range()" (valueChange)="store.select($any($event))" />
      </div>

      <div class="grid">
        <pw-panel class="c8" heading="Memory" icon="memory" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" [skeletonLines]="8" (retry)="store.select(store.range())">
          <div panel-actions class="legend" aria-hidden="true">
            <span><i class="sw" [style.background]="tokens().history[0]"></i>RAM</span>
            <span><i class="sw" [style.background]="tokens().history[1]"></i>VRAM</span>
            <span><i class="sw dash" [style.border-color]="tokens().history[2]"></i>Pitwall</span>
            <span class="sep"></span>
            <span><i class="mk tri" [style.background]="tokens().text2"></i>Loaded</span>
            <span><i class="mk circ" [style.background]="tokens().text2"></i>Released</span>
            <span><i class="mk dia" [style.background]="tokens().bad"></i>Memory pressure</span>
            <span><i class="mk sq" [style.background]="tokens().bad"></i>Engine restart</span>
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
          <pw-echart [option]="byAppOption()" [themeKey]="chartKey()" height="196px" label="Requests per interval by app" />
          <ul class="series-legend" aria-label="Apps in the chart">
            @for (e of byAppLegend(); track e.name) {
              <li><i [style.background]="e.color"></i>{{ e.name }}</li>
            }
          </ul>
        </pw-panel>

        <pw-panel class="c4" heading="Tokens per second" icon="bolt" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="tpsOption()" [themeKey]="chartKey()" height="220px" label="Generation and prompt tokens per second over time" />
        </pw-panel>
        <pw-panel class="c4" heading="Time to first token" icon="clock" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="latencyOption()" [themeKey]="chartKey()" height="220px" label="Time to first token, p50 and p95, over time" />
        </pw-panel>
        <pw-panel class="c4" heading="Requests by model" icon="models" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.history()" (retry)="store.select(store.range())">
          <pw-echart [option]="byModelOption()" [themeKey]="chartKey()" height="190px" label="Requests per interval by model" />
          <ul class="series-legend" aria-label="Models in the chart">
            @for (e of byModelLegend(); track e.name) {
              <li><i [style.background]="e.color"></i>{{ e.name }}</li>
            }
          </ul>
        </pw-panel>

        <pw-usage-card class="c12" />
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
      gap: var(--pd-space-3);
    }
    .page-head {
      margin-bottom: var(--pd-space-2);
    }
    .offline {
      color: var(--pd-warning);
      font-weight: var(--pd-weight-medium);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(12, minmax(0, 1fr));
      gap: var(--pd-space-3);
    }
    /* The three live cards size to their content; they do not stretch to match the tallest. */
    .now {
      align-items: start;
    }
    .c3 {
      grid-column: span 3;
    }
    .c12 {
      grid-column: span 12;
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
    /* A section break of 24 px (the page's section step) above History: its heading and its range control read as one row,
       centred on each other, and at 1440x900 the row starts below the window's fold instead of peeking under the status bar. */
    .history-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      min-height: 32px;
      margin-top: var(--pd-space-6);
      .section {
        font-size: var(--pd-text-body-lg);
      }
    }
    .legend {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      span {
        display: inline-flex;
        align-items: center;
        gap: 5px;
      }
      /* The lane's markers, drawn in CSS so the legend's shapes are the chart's shapes. */
      .mk {
        width: 9px;
        height: 9px;
        &.tri {
          clip-path: polygon(50% 0, 100% 100%, 0 100%);
        }
        &.circ {
          border-radius: 50%;
        }
        &.dia {
          clip-path: polygon(50% 0, 100% 50%, 50% 100%, 0 50%);
        }
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
        background: var(--pd-hairline);
      }
    }
    /* A chart's series, under it: wraps with the card and never runs into the time axis. */
    .series-legend {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-1) var(--pd-space-3);
      margin: var(--pd-space-2) 0 0;
      padding: 0;
      list-style: none;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      li {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        white-space: nowrap;
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
      }
    }
    .perf {
      position: fixed;
      right: var(--pd-space-3);
      bottom: var(--pd-space-3);
      padding: 2px var(--pd-space-2);
      font: 11px var(--pd-font-mono);
      color: var(--pd-text-muted);
      background: var(--pd-surface-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-tag);
    }
    @media (max-width: 1200px) {
      .c3,
      .c4,
      .c5 {
        grid-column: span 6;
      }
      /* The wide ones (memory history, apps table) and the last live card take the full row rather than leave a hole. */
      .c7,
      .c8,
      .c12,
      .events,
      .now > :last-child {
        grid-column: span 12;
      }
    }
  `,
})
export class Dashboard implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly store = inject(DashboardStore);
  private readonly theme = inject(ThemeService);
  protected readonly now = injectNow();
  protected readonly rangeSegs: SuSegment[] = RANGES.map((r) => ({ value: r, label: r }));
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
    return stackedRequestsChart(h?.points ?? [], h?.requestsByApp ?? [], this.store.range(), this.tokens(), this.appName);
  });
  protected readonly byAppLegend = computed(() => stackedLegend(this.store.history()?.requestsByApp ?? [], this.tokens(), this.appName));
  private readonly appName = (k: string) => this.appNames().get(k) ?? k;
  /** Models by their display name, from the library (a model need not be loaded now to have served in the range). */
  private readonly modelNames = computed(() => new Map([...Object.entries(this.store.modelNames()), ...this.live.models().map((m) => [m.modelId, m.name] as const)]));
  private readonly modelName = (k: string) => this.modelNames().get(k) ?? shortModel(k);
  protected readonly byModelOption = computed(() => {
    const h = this.store.history();
    return stackedRequestsChart(h?.points ?? [], h?.requestsByModel ?? [], this.store.range(), this.tokens(), this.modelName);
  });
  protected readonly byModelLegend = computed(() => stackedLegend(this.store.history()?.requestsByModel ?? [], this.tokens(), this.modelName));

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

  /** Release memory and Pause serving: occasional actions, kept out of the header's way. */
  protected readonly moreItems = computed<MenuItem[]>(() => {
    const busy = !this.live.overview();
    return [
      { label: 'Release memory', run: () => void this.live.releaseMemory(), disabled: busy || this.live.isBusy()('release') },
      { label: this.live.paused() ? 'Resume serving' : 'Pause serving', run: () => void this.live.setServing(!this.live.paused()), disabled: busy || this.live.isBusy()('serving') },
    ];
  });
}

/** A readable name for a model id the library does not know any more (`gemma-4-12b-it-q4_k_m` → `gemma-4-12b-it`). */
export function shortModel(id: string): string {
  return id.replace(/[-.](q\d\w*|f16|bf16|f32)$/i, '');
}
