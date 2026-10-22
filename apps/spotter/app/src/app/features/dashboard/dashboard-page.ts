import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { SuCard, SuEmptyState, SuSegmentedControl, SuSparkline, type SuSegment } from '@decryp7/paddock-ui';
import type { DashboardRange, WatchNumbers } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { Chart } from '../../shared/charts/chart';
import { releaseChartSummary, releaseSeries, releasesOption } from '../../shared/charts/chart-options';
import { formatDuration, spanText } from '../../shared/format';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { KindIcon } from '../../shared/ui/kind-icon';
import { Panel } from '../../shared/ui/panel';
import { WatchStatus } from '../../shared/ui/watch-status';
import { DashboardStore } from '../../state/dashboard.store';
import { SettingsStore } from '../../state/settings.store';
import { WatchesStore } from '../../state/watches.store';
import { EventsFeed } from './events-feed';
import { WhatsNew } from './whats-new';

const RANGES: SuSegment[] = [
  { value: '30d', label: '30 days' },
  { value: '90d', label: '90 days' },
  { value: '1y', label: '1 year' },
];

/** The home page: what is new, every watch at a glance, the releases chart and the events feed. */
@Component({
  selector: 'sp-dashboard-page',
  imports: [
    RouterLink, MatIcon, CdkMenu, CdkMenuItem, CdkMenuTrigger, SuCard, SuEmptyState, SuSegmentedControl, SuSparkline,
    Panel, Chart, KindIcon, WatchStatus, AgoPipe, WhatsNew, EventsFeed,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './dashboard-page.html',
  styleUrl: './dashboard-page.scss',
})
export class DashboardPage {
  protected readonly store = inject(DashboardStore);
  protected readonly watches = inject(WatchesStore);
  protected readonly settings = inject(SettingsStore);
  private readonly backend = inject(BACKEND);
  private readonly router = inject(Router);
  protected readonly ranges = RANGES;

  protected readonly dashboard = computed(() => this.store.dashboard().value);
  protected readonly numbers = computed(() => new Map((this.dashboard()?.watches ?? []).map((n) => [n.watchId, n])));
  protected readonly series = computed(() => {
    const d = this.dashboard();
    return d ? releaseSeries(d, this.watches.watches()) : [];
  });
  protected readonly chartOption = computed(() => {
    const d = this.dashboard();
    return d ? releasesOption(d, this.series()) : {};
  });
  protected readonly chartLabel = computed(() => {
    const d = this.dashboard();
    return d ? releaseChartSummary(d, this.series()) : '';
  });
  protected readonly computed = computed(() => {
    const d = this.dashboard();
    return d ? `Computed in ${formatDuration(d.computeMicros / 1000)}` : '';
  });
  protected readonly nextCheck = computed(() => {
    const times = this.watches.watches().map((w) => w.nextCheckMs).filter((t): t is number => t !== null);
    return times.length ? Math.min(...times) : null;
  });
  protected readonly paused = computed(() => {
    const until = this.settings.settings().value?.notificationsPausedUntilMs ?? null;
    return until !== null && until > Date.now();
  });

  constructor() {
    void this.store.load();
    // Recompute shortly after the watch list changes (a check ended, a release was marked seen).
    let first = true;
    effect(() => {
      this.watches.watches();
      if (first) first = false;
      else untracked(() => this.store.scheduleReload());
    });
  }

  protected setRange(range: string): void {
    if (range !== this.store.range()) void this.store.setRange(range as DashboardRange);
  }

  protected pause(minutes: number | null): void {
    void this.settings.pauseNotifications(minutes);
  }

  protected pauseUntilTomorrow(): void {
    const t = new Date();
    t.setHours(24, 0, 0, 0);
    this.pause(Math.max(1, Math.round((t.getTime() - Date.now()) / 60_000)));
  }

  protected openDownloads(): void {
    void this.backend.open('downloads');
  }

  protected goto(id: string): void {
    void this.router.navigate(['/watches', id]);
  }

  protected span(ms: number | null): string {
    return ms === null ? '—' : spanText(ms);
  }

  protected numbersOf(id: string): WatchNumbers | undefined {
    return this.numbers().get(id);
  }
}
