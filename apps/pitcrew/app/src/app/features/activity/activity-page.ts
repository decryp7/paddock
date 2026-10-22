import { ChangeDetectionStrategy, Component, computed, effect, inject, input, untracked } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import type { ActivityReply, JobConclusion } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { ThemeService } from '../../core/theme.service';
import { Chart } from '../../shared/charts/chart';
import { jobsPerDayOption, sumJobDays } from '../../shared/charts/chart-options';
import { formatBytes, formatDateTime, formatDuration, formatPermille, formatRelative } from '../../shared/format';
import { outcomeLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { DebriefButton } from '../../shared/ui/debrief-button';
import { LoadState } from '../../shared/ui/load-state';
import { Sparkline } from '../../shared/ui/sparkline';
import { Timeline, type TimelineRow } from '../../shared/ui/timeline';
import { StatusStore } from '../../state/status.store';
import { ACTIVITY_DAYS, ActivityStore, type ActivityDays } from './activity.store';

/** History per runner or for all: uptime timeline, per-day sparklines, jobs per day, updates and recent jobs. */
@Component({
  selector: 'pc-activity-page',
  imports: [RouterLink, DebriefButton, Chart, Timeline, Sparkline, LoadState, EmptyState],
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

  protected readonly cards = computed(() =>
    this.replies()
      .filter((x): x is { name: string; reply: ActivityReply } => x.reply !== null)
      .map(({ name, reply }) => {
        const s = reply.summary;
        const busy = s.days.reduce((a, d) => a + d.busyMs, 0);
        return {
          name,
          jobs: s.jobs,
          uptime: formatPermille(s.uptimePermille),
          restarts: s.restarts,
          avgJob: s.jobs ? formatDuration(busy / s.jobs) : '—',
          jobsPerDay: s.days.map((d) => d.jobs),
          uptimePerDay: s.days.map((d) => d.uptimePermille),
        };
      }),
  );

  protected readonly from = computed(() => {
    const from = Math.min(...this.replies().map((x) => x.reply?.summary.from ?? Infinity));
    return Number.isFinite(from) ? from : this.clock.now() - this.store.days() * 86_400_000;
  });
  protected readonly rows = computed<TimelineRow[]>(() =>
    this.replies().map(({ name, reply }) => ({ name, up: reply?.summary.up ?? [], busy: reply?.summary.busy ?? [], marks: reply?.summary.marks ?? [], uptimePermille: reply?.summary.uptimePermille ?? null })),
  );
  protected readonly jobsOption = computed(() =>
    jobsPerDayOption(
      sumJobDays(this.replies().map((x) => x.reply?.jobDays ?? [])),
      this.theme.chartTokens(),
      this.store.days() === 30,
    ),
  );
  protected readonly updates = computed(() =>
    this.replies()
      .flatMap(({ name, reply }) => (reply?.summary.updates ?? []).map((u) => ({ name, at: u.at, look: outcomeLook(u.outcome) })))
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
