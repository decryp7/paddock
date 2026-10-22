import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { ViewPrefs } from '../../core/view-prefs';
import { WidgetBoard } from '../widgets/widget-board';
import { isNotBuilt } from '../../core/command-error';
import { formatDuration } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';
import { AnomaliesCallout } from './anomalies-callout';
import { DiagnosisCard } from './diagnosis-card';
import { EventsFeed } from './events-feed';
import { HealthHero } from './health-hero';
import { ConsumersPanel, DiskTrendPanel, HistoryPanel } from './history-panels';
import { BatteryTile, CpuTile, DisksTile, GpuTile, MemoryTile, NetworkTile, SensorsTile } from './tiles';
import { TopApps } from './top-apps';

/**
 * The home screen: how the PC is doing right now (live tiles), what is using it (top apps), how it got here
 * (history with the events marked), and what Dyno did about it. Every panel owns its loading, empty and error state, so
 * one failed request never blanks the page.
 */
@Component({
  selector: 'dy-dashboard',
  imports: [
    RouterLink,
    Icon,
    AnomaliesCallout,
    WidgetBoard,
    HealthHero,
    DiagnosisCard,
    CpuTile,
    MemoryTile,
    DisksTile,
    NetworkTile,
    GpuTile,
    BatteryTile,
    SensorsTile,
    TopApps,
    EventsFeed,
    HistoryPanel,
    DiskTrendPanel,
    ConsumersPanel,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Dashboard</h1>
          <p class="num">{{ subtitle() }}</p>
        </div>
        <div class="seg" role="group" aria-label="Dashboard layout">
          <button type="button" [attr.aria-pressed]="prefs.dashboardView() === 'standard'" (click)="prefs.setDashboardView('standard')">Standard</button>
          <button type="button" [attr.aria-pressed]="prefs.dashboardView() === 'widgets'" (click)="prefs.setDashboardView('widgets')">Widgets</button>
        </div>
        <a class="btn" routerLink="/processes"><dy-icon name="processes" /> Open task manager</a>
        <button type="button" class="btn" [disabled]="!rules.loaded() || rules.saving()" (click)="rules.pause(!rules.paused())">
          <dy-icon [name]="rules.paused() ? 'play' : 'pause'" /> {{ rules.paused() ? 'Resume rules' : 'Pause rules' }}
        </button>
      </header>

      @if (prefs.dashboardView() === 'widgets') {
        <dy-widget-board />
      } @else {
      <dy-anomalies-callout />

      <div class="grid" [class.no-hero]="!showHero()">
        <dy-cpu-tile />
        <dy-memory-tile />
        <dy-disks-tile />

        @if (showHero()) {
          <dy-health-hero class="health" />
        }
        <dy-diagnosis-card class="diag" />

        <dy-network-tile />
        <dy-gpu-tile />
        <dy-battery-tile />
        <dy-sensors-tile />

        <dy-top-apps class="s4" metric="cpu" />
        <dy-top-apps class="s4" metric="memory" />
        <dy-events-feed class="s4" />

        <dy-history-panel class="hist" />
        <div class="side">
          <dy-consumers-panel />
          <dy-disk-trend-panel />
        </div>
      </div>
      }
    </div>
  `,
  styles: `
    :host { display: block; container-type: inline-size; container-name: dash; }
    .page-head { align-items: center; margin-bottom: var(--sp-4); }
    .page-head p { color: var(--text-3); }
    .grid { display: grid; grid-template-columns: repeat(12, minmax(0, 1fr)); gap: var(--sp-4); margin-top: var(--sp-4); align-items: stretch; }
    .grid > * { grid-column: span 4; min-width: 0; }
    .grid > .health { grid-column: span 7; }
    .grid > .diag { grid-column: span 5; }
    .grid.no-hero > .diag { grid-column: span 12; }
    .grid > dy-network-tile, .grid > dy-gpu-tile, .grid > dy-battery-tile, .grid > dy-sensors-tile { grid-column: span 3; }
    .grid > .hist { grid-column: span 8; }
    .grid > .side { grid-column: span 4; display: grid; gap: var(--sp-4); align-content: start; }
    @container dash (max-width: 1060px) {
      .grid > .health, .grid > .diag, .grid > .hist, .grid > .side { grid-column: span 12; }
      .grid > dy-cpu-tile, .grid > dy-memory-tile, .grid > dy-disks-tile { grid-column: span 6; }
      .grid > dy-disks-tile { grid-column: span 12; }
      .grid > dy-network-tile, .grid > dy-gpu-tile, .grid > dy-battery-tile, .grid > dy-sensors-tile { grid-column: span 6; }
      .grid > .s4 { grid-column: span 6; }
      .grid > dy-events-feed { grid-column: span 12; }
      .grid > .side { grid-template-columns: 1fr 1fr; }
    }
    @container dash (max-width: 720px) {
      .page { padding-inline: var(--sp-4); }
      .grid > * { grid-column: span 12 !important; }
      .grid > .side { grid-template-columns: 1fr; }
    }
  `,
})
export class Dashboard implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly rules = inject(RulesStore);
  protected readonly prefs = inject(ViewPrefs);
  private readonly health = inject(HealthStore);

  /** The health score is shown only when the engine provides it: a "not built yet" answer hides it instead of faking a number. */
  protected readonly showHero = computed(() => !isNotBuilt(this.health.reportError()));
  protected readonly subtitle = computed(() => {
    const s = this.live.system();
    const profile = this.rules.activeProfile()?.name;
    if (!s) return 'Connecting to the Dyno agent…';
    return `${profile ? `${profile} profile · ` : ''}up ${formatDuration(s.uptimeSecs)}${this.rules.paused() ? ' · rules paused' : ''}`;
  });

  ngOnInit(): void {
    void this.rules.ensure();
  }
}
