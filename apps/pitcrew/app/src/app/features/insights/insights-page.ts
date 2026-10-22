import { ChangeDetectionStrategy, Component, computed, inject, OnInit, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import type { RepoInsight } from '../../core/api.types';
import { ThemeService } from '../../core/theme.service';
import { Chart } from '../../shared/charts/chart';
import { repoDaysOption } from '../../shared/charts/repo-days-option';
import { formatCount, formatDuration, formatPermille, plural } from '../../shared/format';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { adviceLook, trendText } from './insights-format';
import { INSIGHTS_DAYS, InsightsStore, type InsightsDays } from './insights.store';

/** How workflows run on the runners: advice, queue times, success per repository, slow and flaky jobs, runner load. */
@Component({
  selector: 'pc-insights-page',
  imports: [RouterLink, MatIcon, Chart, LoadState, EmptyState],
  providers: [InsightsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './insights-page.html',
  styleUrl: './insights-page.scss',
})
export class InsightsPage implements OnInit {
  protected readonly store = inject(InsightsStore);
  private readonly theme = inject(ThemeService);

  protected readonly dayOptions = INSIGHTS_DAYS.map((d) => ({ key: d, label: `${d} days` }));
  protected readonly reply = computed(() => this.store.reply().value);
  protected readonly data = computed(() => this.reply()?.insights ?? null);
  protected readonly pendingText = computed(() => {
    const n = this.reply()?.pendingOutcomes ?? 0;
    return n > 0 ? `${plural(n, 'job')} ${n === 1 ? 'has' : 'have'} no outcome yet; refresh to look up more.` : null;
  });

  protected readonly advice = computed(() => (this.data()?.advice ?? []).map((a) => ({ ...a, ...adviceLook(a.kind) })));

  /** Hero tiles. The engine returns no overall success figure and rates are not computed here, so queue times lead. */
  protected readonly kpis = computed(() => {
    const d = this.data();
    if (!d) return [];
    return [
      { label: 'Success rate', value: formatPermille(d.successPermille) },
      { label: 'Average queue time', value: formatDuration(d.queue.avgMs) },
      { label: '95th percentile queue time', value: formatDuration(d.queue.p95Ms) },
    ];
  });
  protected readonly footer = computed(() => {
    const d = this.data();
    return d ? `${plural(d.jobs, 'job')} in total, ${formatCount(d.jobsWithOutcome)} with a known outcome` : '';
  });
  private readonly open = signal<ReadonlySet<number>>(new Set());

  protected readonly repos = computed(() =>
    (this.data()?.repos ?? []).map((r) => ({
      repo: r.repo,
      jobs: formatCount(r.jobs),
      rate: formatPermille(r.successPermille),
      trend: trendText(r.earlierPermille, r.laterPermille),
      queue: formatDuration(r.avgQueuedMs),
      label: this.chartLabel(r),
      option: repoDaysOption(r.days, this.theme.chartTokens()),
    })),
  );

  protected readonly slowest = computed(() =>
    (this.data()?.slowest ?? []).map((j) => ({
      repo: j.repo,
      job: j.job,
      runs: formatCount(j.runs),
      avg: formatDuration(j.avgMs),
      p95: formatDuration(j.p95Ms),
      max: formatDuration(j.maxMs),
    })),
  );

  protected readonly flaky = computed(() =>
    (this.data()?.flaky ?? []).map((j) => ({
      repo: j.repo,
      job: j.job,
      runs: formatCount(j.runs),
      failures: formatCount(j.failures),
      flips: formatCount(j.flips),
    })),
  );

  protected readonly runners = computed(() =>
    (this.data()?.runners ?? []).map((r) => ({
      name: r.runner,
      jobs: plural(r.jobs, 'job'),
      busy: formatPermille(r.busyPermille),
      width: Math.min(100, Math.max(0, r.busyPermille / 10)),
    })),
  );

  protected isOpen(i: number): boolean {
    return this.open().has(i);
  }

  protected toggle(i: number): void {
    const next = new Set(this.open());
    if (!next.delete(i)) next.add(i);
    this.open.set(next);
  }

  ngOnInit(): void {
    void this.store.load();
  }

  protected setDays(days: InsightsDays): void {
    void this.store.setDays(days);
  }

  private chartLabel(r: RepoInsight): string {
    return `${r.repo}: ${plural(r.success, 'succeeded job')} and ${plural(r.failure, 'failed job')} per day`;
  }
}
