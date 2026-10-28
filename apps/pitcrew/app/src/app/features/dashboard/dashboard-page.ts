import { ChangeDetectionStrategy, Component, DestroyRef, computed, effect, inject, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import type { ActivityReply, FinishedJob } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { ThemeService } from '../../core/theme.service';
import { RegisterLauncher } from '../../shell/register-launcher';
import { SettingsLauncher } from '../../shell/settings-launcher';
import { Chart } from '../../shared/charts/chart';
import { diskOption, durationOption, jobsPerDayOption, loadOption, queueOption, sumJobDays } from '../../shared/charts/chart-options';
import { formatBytes, formatDateTime, formatPermille, plural } from '../../shared/format';
import { outcomeLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { Timeline, type TimelineRow } from '../../shared/ui/timeline';
import { StatusStore } from '../../state/status.store';
import { DashboardStore, RANGES, REFRESH_MS, type Range } from './dashboard.store';
import { EventsFeed } from './events-feed';
import { feedItems } from './feed';
import { MachineTiles } from './machine-tiles';
import { RunnerTile } from './runner-tile';
import { ServiceOffline } from './service-offline';

/** The home screen: live runner tiles, the machine, history charts and the events feed. */
@Component({
  selector: 'pc-dashboard-page',
  imports: [MatIcon, RouterLink, RunnerTile, MachineTiles, Chart, Timeline, EventsFeed, LoadState, EmptyState, ServiceOffline],
  providers: [DashboardStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './dashboard-page.html',
  styleUrl: './dashboard-page.scss',
})
export class DashboardPage {
  protected readonly status = inject(StatusStore);
  protected readonly store = inject(DashboardStore);
  protected readonly clock = inject(ClockService);
  protected readonly register = inject(RegisterLauncher);
  protected readonly settingsWindow = inject(SettingsLauncher);
  private readonly theme = inject(ThemeService);

  protected readonly ranges = Object.entries(RANGES).map(([key, v]) => ({ key: key as Range, label: v.label }));
  protected readonly hours = computed(() => RANGES[this.store.range()].hours);
  protected readonly rangeLabel = computed(() => RANGES[this.store.range()].label);
  protected readonly runners = this.status.runners;
  protected readonly names = computed(() => this.runners().map((r) => r.name));

  protected readonly summary = computed(() => {
    const rs = this.runners();
    const busy = rs.filter((r) => r.status === 'busy').length;
    const attention = this.status.attention().length;
    if (rs.length === 0) return 'No runners yet';
    const parts = [plural(rs.length, 'runner')];
    if (busy) parts.push(`${busy} running ${busy === 1 ? 'a job' : 'jobs'}`);
    if (attention) parts.push(`${attention} ${attention === 1 ? 'needs' : 'need'} attention`);
    return parts.join(' · ');
  });

  /** The runners the charts show (all, or the focused one). */
  private readonly shown = computed(() => {
    const f = this.store.focus();
    return f ? this.names().filter((n) => n === f) : this.names();
  });
  private readonly replies = computed(() => {
    const out: Record<string, ActivityReply | null> = {};
    for (const n of this.shown()) out[n] = this.store.activity()[n]?.value ?? null;
    return out;
  });
  protected readonly activityState = computed(() => {
    const all = this.shown().map((n) => this.store.activity()[n]);
    return { hasValue: all.some((l) => l?.value), error: all.find((l) => l?.error)?.error ?? null };
  });

  protected readonly lastJobs = computed(() => {
    const out: Record<string, FinishedJob | undefined> = {};
    for (const [n, l] of Object.entries(this.store.activity())) out[n] = l.value?.summary.recentJobs[0];
    return out;
  });

  protected readonly jobDays = computed(() => sumJobDays(Object.values(this.replies()).map((r) => (this.hourly() ? r?.jobHours : r?.jobDays) ?? [])));
  /** The 24 h range shows jobs per hour. */
  protected readonly hourly = computed(() => this.store.range() === '24h');
  protected readonly jobTotal = computed(() => this.jobDays().reduce((a, d) => a + d.success + d.failure + d.cancelled + d.unknown, 0));
  protected readonly noToken = computed(() =>
    Object.entries(this.replies())
      .filter(([, r]) => r && !r.hasApiToken)
      .map(([n]) => n),
  );
  protected readonly jobsOption = computed(() => jobsPerDayOption(this.jobDays(), this.theme.chartTokens(), this.store.range() === '30d', this.hourly()));
  protected readonly durationOpt = computed(() =>
    durationOption(
      Object.entries(this.replies()).map(([name, r]) => ({ name, points: (r?.summary.days ?? []).map((d) => [d.start + 12 * 3_600_000, d.avgJobMs] as [number, number]) })),
      this.theme.chartTokens(),
      this.hours(),
    ),
  );
  protected readonly hasQueue = computed(() => this.jobDays().some((d) => d.avgQueuedMs !== null));
  protected readonly queueOpt = computed(() => queueOption(this.jobDays(), this.theme.chartTokens()));
  protected readonly diskOpt = computed(() => {
    const now = this.clockMinute();
    const from = now - this.hours() * 3_600_000;
    const series = this.shown().map((name) => ({ name, points: this.store.folders()[name]?.value?.folder ?? [] }));
    const cleanups = Object.values(this.replies())
      .flatMap((r) => r?.summary.marks ?? [])
      .filter((m) => m.kind === 'cleanup' && m.at >= from)
      .map((m) => m.at);
    return diskOption(series, [...new Set(cleanups)], this.theme.chartTokens(), this.hours());
  });
  private readonly samples = computed(() => this.store.machine().value?.machine ?? []);
  protected readonly cpuOpt = computed(() =>
    loadOption(
      this.samples().map((x) => ({ at: x.at, runners: x.runners.cpuPermille, machine: x.cpuPermille })),
      (v) => formatPermille(v),
      1000,
      this.theme.chartTokens(),
      this.hours(),
    ),
  );
  protected readonly memOpt = computed(() =>
    loadOption(
      this.samples().map((x) => ({ at: x.at, runners: x.runners.memBytes, machine: x.memUsedBytes })),
      (v) => formatBytes(v, 0),
      this.samples().at(-1)?.memTotalBytes,
      this.theme.chartTokens(),
      this.hours(),
    ),
  );
  protected readonly diskState = computed(() => {
    const all = this.shown().map((n) => this.store.folders()[n]);
    return { hasValue: all.some((l) => l?.value), error: all.find((l) => l?.error)?.error ?? null };
  });

  protected readonly updates = computed(() =>
    Object.entries(this.replies())
      .flatMap(([runner, r]) => (r?.summary.updates ?? []).map((u) => ({ runner, at: u.at, look: outcomeLook(u.outcome) })))
      .sort((a, b) => b.at - a.at)
      .slice(0, 8),
  );

  /** The clock rounded to the minute, so range-relative charts don't rebuild every second. */
  private readonly clockMinute = computed(() => Math.floor(this.clock.now() / 60_000) * 60_000);
  protected readonly timelineFrom = computed(() => {
    const froms = Object.values(this.replies()).map((r) => r?.summary.from ?? Infinity);
    const from = Math.min(...froms);
    return Number.isFinite(from) ? Math.max(from, this.clockMinute() - this.hours() * 3_600_000) : this.clockMinute() - this.hours() * 3_600_000;
  });
  protected readonly timelineRows = computed<TimelineRow[]>(() =>
    Object.entries(this.replies()).map(([name, r]) => ({
      name,
      up: r?.summary.up ?? [],
      busy: r?.summary.busy ?? [],
      marks: r?.summary.marks ?? [],
      uptimePermille: r?.summary.uptimePermille ?? null,
    })),
  );

  protected readonly feed = computed(() => {
    const all: Record<string, ActivityReply | null> = {};
    for (const n of this.names()) all[n] = this.store.activity()[n]?.value ?? null;
    const from = this.timelineFrom();
    return feedItems(all, this.status.notifications()).filter((i) => i.at >= from);
  });

  constructor() {
    // Load when the runners change; refresh on pushed events and while visible, at most every 60 s.
    effect(() => {
      const names = this.names();
      this.status.eventCount();
      const visible = this.clock.visible();
      if (visible && names.length > 0) untracked(() => void this.store.refresh(names));
    });
    // Details for the jobs that are running now (title, workflow, link).
    let jobKeys = '';
    effect(() => {
      const busy = this.runners().filter((r) => r.currentJob);
      const key = busy.map((r) => `${r.name}:${r.currentJob?.info.taskId ?? r.currentJob?.startedAt}`).join('|');
      if (key === jobKeys) return;
      jobKeys = key;
      untracked(() => busy.forEach((r) => void this.store.loadDetails(r.name)));
    });
    const timer = setInterval(() => {
      if (this.clock.visible() && this.names().length > 0) void this.store.refresh(this.names());
    }, REFRESH_MS);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }

  protected setFocus(value: string): void {
    this.store.setFocus(value === '' ? null : value);
  }

  protected when(at: number): string {
    return formatDateTime(at);
  }
}
