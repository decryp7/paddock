import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';

import type { DashboardRange } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ThemeService } from '../../core/theme.service';
import { Chart } from '../../shared/charts/chart';
import {
  downloadCounts,
  downloadsOption,
  downloadsSummary,
  releaseChartSummary,
  releaseSeries,
  releasesOption,
  reliabilityCounts,
  reliabilityOption,
  reliabilitySummary,
} from '../../shared/charts/chart-options';
import { formatDuration } from '../../shared/format';
import { AgoPipe, AgoShortPipe } from '../../shared/ui/ago.pipe';
import { Avatar } from '../../shared/ui/avatar';
import { Panel } from '../../shared/ui/panel';
import { MenuButton, type MenuEntry } from '../../shared/ui/row-menu';
import { StatusDot } from '../../shared/ui/status-dot';
import { DashboardStore } from '../../state/dashboard.store';
import { SettingsStore } from '../../state/settings.store';
import { WatchesStore } from '../../state/watches.store';
import { CadenceCards } from './cadence-cards';
import { EventsFeed } from './events-feed';
import { WhatsNew } from './whats-new';

const RANGES: { value: DashboardRange; label: string }[] = [
  { value: '30d', label: '30 days' },
  { value: '90d', label: '90 days' },
  { value: '1y', label: '1 year' },
];

/** The home page: what is new, every watch at a glance, the releases chart and the events feed. */
@Component({
  selector: 'sp-dashboard-page',
  imports: [RouterLink, MatIcon, Panel, Chart, Avatar, StatusDot, MenuButton, AgoPipe, AgoShortPipe, WhatsNew, EventsFeed, CadenceCards],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './dashboard-page.html',
  styleUrl: './dashboard-page.scss',
})
export class DashboardPage {
  protected readonly store = inject(DashboardStore);
  protected readonly watches = inject(WatchesStore);
  protected readonly settings = inject(SettingsStore);
  private readonly backend = inject(BACKEND);
  private readonly theme = inject(ThemeService);
  protected readonly ranges = RANGES;

  protected readonly dashboard = computed(() => this.store.dashboard().value);
  protected readonly hasCadence = computed(() => (this.dashboard()?.watches ?? []).some((n) => n.releases.some((c) => c > 0)));
  protected readonly rangeLabel = computed(() => RANGES.find((r) => r.value === this.store.range())?.label.toLowerCase() ?? '');
  protected readonly headEntries = computed<MenuEntry[]>(() => [
    { id: 'check', label: 'Check all now', icon: 'refresh', disabled: this.watches.checking() || !this.watches.watches().length },
    ...(this.paused()
      ? [{ id: 'resume', label: 'Resume notifications', icon: 'bell' }]
      : [
          { id: 'hour', label: 'Pause notifications for 1 hour', icon: 'bell-off', separated: true },
          { id: 'tomorrow', label: 'Pause notifications until tomorrow', icon: 'bell-off' },
        ]),
  ]);
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
  /** Checks in the range; the chart is only drawn when some failed (a chart of all-good bars says nothing). */
  protected readonly reliability = computed(() => {
    const d = this.dashboard();
    return d ? reliabilityCounts(d) : { total: 0, failed: 0 };
  });
  protected readonly reliabilityOption = computed(() => {
    const d = this.dashboard();
    return d ? reliabilityOption(d, this.theme.chartTokens()) : {};
  });
  protected readonly reliabilityLabel = computed(() => {
    const d = this.dashboard();
    return d ? reliabilitySummary(d) : '';
  });
  protected readonly downloads = computed(() => {
    const d = this.dashboard();
    return d ? downloadCounts(d) : { total: 0, failed: 0 };
  });
  protected readonly downloadsOption = computed(() => {
    const d = this.dashboard();
    return d ? downloadsOption(d, this.theme.chartTokens()) : {};
  });
  protected readonly downloadsLabel = computed(() => {
    const d = this.dashboard();
    return d ? downloadsSummary(d) : '';
  });
  protected readonly computed = computed(() => {
    const d = this.dashboard();
    if (!d) return '';
    return d.computeMicros < 1000 ? 'Computed in under 1 ms' : `Computed in ${formatDuration(d.computeMicros / 1000)}`;
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

  protected onHead(id: string): void {
    if (id === 'check') void this.watches.checkNow(null);
    else if (id === 'hour') this.pause(60);
    else if (id === 'tomorrow') this.pauseUntilTomorrow();
    else if (id === 'resume') this.pause(null);
  }
}
