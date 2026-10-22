import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import type { ActivityReply, JobConclusion } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { ThemeService } from '../../core/theme.service';
import { Chart } from '../../shared/charts/chart';
import { jobsPerDayOption, sumJobDays } from '../../shared/charts/chart-options';
import { MatTooltip } from '@angular/material/tooltip';
import { formatBytes, formatCount, formatDateTime, formatDuration, formatPermille, formatRelative } from '../../shared/format';
import { outcomeLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { DebriefButton } from '../../shared/ui/debrief-button';
import { LoadState } from '../../shared/ui/load-state';
import { TrendBars } from './trend-bars';
import { Timeline, type TimelineRow } from '../../shared/ui/timeline';
import { StatusStore } from '../../state/status.store';
import { ACTIVITY_DAYS, ActivityStore, type ActivityDays } from './activity.store';

export type RunnerSortKey = 'name' | 'jobs' | 'uptime' | 'restarts' | 'avgJob';

/** Column headers of the per-runner table; units live here, not in the cells. */
export const RUNNER_COLUMNS: { key: RunnerSortKey; label: string; numeric: boolean }[] = [
  { key: 'name', label: 'Runner', numeric: false },
  { key: 'jobs', label: 'Jobs', numeric: true },
  { key: 'uptime', label: 'Uptime', numeric: true },
  { key: 'restarts', label: 'Restarts', numeric: true },
  { key: 'avgJob', label: 'Avg job', numeric: true },
];

/** Uptime below this share (per mille) is tinted as a warning. */
export const LOW_UPTIME_PERMILLE = 990;
/** Days shown in the jobs trend bars. */
export const TREND_DAYS = 7;

/** The jobs-per-day chart gets a named axis and, with two or more outcomes, a legend. */
export function withJobsAxis(option: Record<string, unknown>): Record<string, unknown> {
  const series = Array.isArray(option['series']) ? option['series'] : [];
  const legend = (option['legend'] ?? {}) as object;
  const yAxis = (option['yAxis'] ?? {}) as object;
  return { ...option, legend: { ...legend, show: series.length > 1 }, yAxis: { ...yAxis, name: 'Jobs', nameTextStyle: { align: 'left' }, min: 0 } };
}

/** History per runner or for all: per-runner table, uptime timeline, jobs per day, updates and recent jobs. */
@Component({
  selector: 'pc-activity-page',
  imports: [RouterLink, DebriefButton, Chart, Timeline, LoadState, EmptyState, MatTooltip, TrendBars],
  providers: [ActivityStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './activity-page.html',
  styleUrl: './activity-page.scss',
})
export class ActivityPage {
  /** Query parameter: one runner, or all when absent. */
  readonly runner = input<string | undefined>(undefined);

  protected readonly status = inject(StatusStore);
  protected readonly store = inject(ActivityStore);
  protected readonly clock = inject(ClockService);
  private readonly theme = inject(ThemeService);
  private readonly router = inject(Router);

  protected readonly dayOptions = ACTIVITY_DAYS.map((d) => ({ key: d, label: d === 1 ? 'Today' : `${d} days` }));
  protected readonly names = computed(() => this.status.runners().map((r) => r.name));
  protected readonly shown = computed(() => {
    const r = this.runner();
    return r && this.names().includes(r) ? [r] : this.names();
  });
  private readonly replies = computed(() => this.shown().map((n) => ({ name: n, reply: this.store.activity()[n]?.value ?? null })));
  protected readonly state = computed(() => {
    const all = this.shown().map((n) => this.store.activity()[n]);
    return { hasValue: all.some((l) => l?.value), error: all.find((l) => l?.error)?.error ?? null };
  });

  protected readonly sort = signal<{ key: RunnerSortKey; dir: 'asc' | 'desc' }>({ key: 'name', dir: 'asc' });
  protected readonly columns = RUNNER_COLUMNS;

  /** One row per runner; numbers are pre-formatted here so the template interpolates only strings. */
  protected readonly runnerRows = computed(() => {
    const rows = this.replies()
      .filter((x): x is { name: string; reply: ActivityReply } => x.reply !== null)
      .map(({ name, reply }) => {
        const s = reply.summary;
        const busy = s.days.reduce((a, d) => a + d.busyMs, 0);
        const avgMs = s.jobs ? busy / s.jobs : -1;
        return {
          name,
          jobs: s.jobs,
          jobsText: formatCount(s.jobs),
          uptimePermille: s.uptimePermille,
          uptime: formatPermille(s.uptimePermille),
          lowUptime: s.uptimePermille < LOW_UPTIME_PERMILLE,
          restarts: s.restarts,
          restartsText: formatCount(s.restarts),
          avgMs,
          avgJob: s.jobs ? formatDuration(avgMs) : '—',
          trend: s.days.slice(-TREND_DAYS).map((d) => d.jobs),
        };
      });
    const { key, dir } = this.sort();
    const sign = dir === 'asc' ? 1 : -1;
    const value = (r: (typeof rows)[number]): string | number =>
      key === 'name' ? r.name.toLowerCase() : key === 'jobs' ? r.jobs : key === 'uptime' ? r.uptimePermille : key === 'restarts' ? r.restarts : r.avgMs;
    return rows.sort((a, b) => {
      const x = value(a);
      const y = value(b);
      return (x < y ? -1 : x > y ? 1 : a.name.localeCompare(b.name)) * sign;
    });
  });

  protected readonly from = computed(() => {
    const from = Math.min(...this.replies().map((x) => x.reply?.summary.from ?? Infinity));
    return Number.isFinite(from) ? from : this.clock.now() - this.store.days() * 86_400_000;
  });
  protected readonly rows = computed<TimelineRow[]>(() =>
    this.replies().map(({ name, reply }) => ({ name, up: reply?.summary.up ?? [], busy: reply?.summary.busy ?? [], marks: reply?.summary.marks ?? [], uptimePermille: reply?.summary.uptimePermille ?? null })),
  );
  protected readonly jobsOption = computed(() =>
    withJobsAxis(
      jobsPerDayOption(sumJobDays(this.replies().map((x) => x.reply?.jobDays ?? [])), this.theme.chartTokens(), this.store.days() === 30),
    ),
  );
  protected readonly updates = computed(() =>
    this.replies()
      .flatMap(({ name, reply }) => (reply?.summary.updates ?? []).map((u) => {
        const look = outcomeLook(u.outcome);
        return { name, at: u.at, tone: look.tone, text: `${look.label} ${look.detail}` };
      }))
      .sort((a, b) => b.at - a.at),
  );
  protected readonly jobs = computed(() =>
    this.replies()
      .flatMap(({ name, reply }) => (reply?.summary.recentJobs ?? []).map((j) => ({ name, job: j })))
      .sort((a, b) => b.job.finishedAt - a.job.finishedAt)
      .slice(0, 30),
  );

  constructor() {
    effect(() => {
      const names = this.shown();
      if (names.length) untracked(() => void this.store.show(names));
    });
  }

  /** Click on a column header: same column flips the direction, a new one starts ascending (names) or descending (numbers). */
  protected sortBy(key: RunnerSortKey): void {
    const cur = this.sort();
    this.sort.set(cur.key === key ? { key, dir: cur.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'name' ? 'asc' : 'desc' });
  }
  protected ariaSort(key: RunnerSortKey): 'ascending' | 'descending' | 'none' {
    const cur = this.sort();
    return cur.key !== key ? 'none' : cur.dir === 'asc' ? 'ascending' : 'descending';
  }

  protected pick(value: string): void {
    void this.router.navigate([], { queryParams: { runner: value || null }, replaceUrl: true });
  }

  protected when(at: number): string {
    return formatDateTime(at);
  }
  protected ago(at: number): string {
    return formatRelative(at, this.clock.now());
  }
  /** The result badge of a job whose outcome the server told (needs a Gitea read token). */
  protected outcome(c: JobConclusion | undefined): { label: string; tone: string } | null {
    switch (c) {
      case 'success':
        return { label: 'Succeeded', tone: 'success' };
      case 'failure':
        return { label: 'Failed', tone: 'danger' };
      case 'cancelled':
        return { label: 'Cancelled', tone: 'neutral' };
      case 'skipped':
        return { label: 'Skipped', tone: 'neutral' };
      default:
        return null;
    }
  }

  protected cpu(p: number): string {
    return formatPermille(p);
  }
  protected bytes(n: number): string {
    return formatBytes(n);
  }
  protected took(from: number, to: number): string {
    return formatDuration(to - from);
  }
  protected setDays(d: ActivityDays): void {
    void this.store.setDays(d);
  }
}
