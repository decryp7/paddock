import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import type { RunDetails, UsagePoint } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ClockService } from '../../core/clock.service';
import { formatBytes, formatDateTime, formatDuration, formatElapsed, formatPermille, formatRelative } from '../../shared/format';
import { loggedOffLabel, pendingLabel, sessionLabel } from '../../shared/labels';
import { RunnerActions } from '../../shared/runner-actions';
import { EmptyState } from '../../shared/ui/empty-state';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';
import { ConfigPanel } from './config-panel';
import { DiskPanel } from './disk-panel';
import { ProcessesPanel } from './processes-panel';
import { RunnerStatusText } from './runner-status';
import { RunnerUpdatePanel } from './runner-update-panel';
import { UsageChart } from './usage-chart';

export type DetailTab = 'overview' | 'updates' | 'processes' | 'config' | 'disk';

const TABS: { key: DetailTab; label: string }[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'updates', label: 'Updates' },
  { key: 'processes', label: 'Processes' },
  { key: 'config', label: 'Configuration' },
  { key: 'disk', label: 'Disk & cleanup' },
];

/** One runner: overview, updates and disk, with its actions. */
@Component({
  selector: 'pc-runner-detail-page',
  imports: [MatIcon, RouterLink, RunnerStatusText, RunnerActions, EmptyState, DiskPanel, RunnerUpdatePanel, UsageChart, ProcessesPanel, ConfigPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './runner-detail-page.html',
  styleUrl: './runner-detail-page.scss',
})
export class RunnerDetailPage {
  /** Route parameter. */
  readonly name = input.required<string>();
  /** Query parameter. */
  readonly tab = input<string | undefined>(undefined);

  protected readonly status = inject(StatusStore);
  protected readonly settings = inject(SettingsStore);
  protected readonly clock = inject(ClockService);
  private readonly backend = inject(BACKEND);
  private readonly router = inject(Router);

  protected readonly tabs = TABS;
  protected readonly current = computed<DetailTab>(() => (TABS.some((t) => t.key === this.tab()) ? (this.tab() as DetailTab) : 'overview'));
  protected readonly runner = computed(() => this.status.runners().find((r) => r.name === this.name()) ?? null);
  protected readonly config = computed(() => this.settings.runnerConfig(this.name()));
  protected readonly details = signal<RunDetails | null>(null);
  /** The runner's CPU and memory over the last 24 hours (with its processes). */
  protected readonly usage = signal<UsagePoint[]>([]);
  /** Counts shown on a tab only when non-zero: the pending update and the leftover processes. */
  protected readonly tabCounts = computed<Partial<Record<DetailTab, number>>>(() => {
    const r = this.runner();
    return r ? { updates: r.updateTarget ? 1 : 0, processes: r.leftovers } : {};
  });
  protected readonly leftoverText = computed(() => {
    const n = this.runner()?.leftovers ?? 0;
    return `${n} leftover ${n === 1 ? 'process' : 'processes'}`;
  });
  /** The CPU chart is fixed to 0-100 % of the machine (1000 permille). */
  protected readonly cpuPeak = computed(() => {
    const u = this.usage();
    return u.length ? Math.max(...u.map((p) => p.cpuPeakPermille)) : null;
  });
  protected readonly memPeak = computed(() => {
    const u = this.usage();
    return u.length ? Math.max(...u.map((p) => p.memPeakBytes)) : null;
  });
  protected readonly usageNow = computed(() => {
    const u = this.runner()?.usage;
    return u ? { cpu: formatPermille(u.cpuPermille), mem: formatBytes(u.memBytes), processes: u.processes } : null;
  });
  protected readonly usageCpu = computed(() => this.usage().map((p) => p.cpuPermille));
  protected readonly usageMem = computed(() => this.usage().map((p) => p.memBytes));
  protected readonly usagePeak = computed(() => {
    const u = this.usage();
    return u.length ? { cpu: formatPermille(Math.max(...u.map((p) => p.cpuPeakPermille))), mem: formatBytes(Math.max(...u.map((p) => p.memPeakBytes))) } : null;
  });

  protected readonly cpuPeakLabel = computed(() => `peak ${this.usagePeak()?.cpu ?? '—'}`);
  protected readonly memPeakLabel = computed(() => `peak ${this.usagePeak()?.mem ?? '—'}`);

  protected readonly session = computed(() => {
    const r = this.runner();
    return r ? sessionLabel(r.session) : '';
  });
  /** Jobs would run (or run now) as the service account, without a desktop. */
  protected readonly serviceAccountWarning = computed(() => {
    const r = this.runner();
    if (!r) return null;
    if (r.session.kind === 'serviceAccount' && r.session.fallback) return 'Nobody is logged on, so this runner runs as the service account (LocalSystem on Windows) without a desktop. Jobs that need a desktop, such as WebView2 or UI tests, will fail until a user logs on.';
    if (r.sessionMode === 'service') return 'This runner runs in the service session: jobs run as LocalSystem without a desktop.';
    return null;
  });
  protected readonly loggedOff = computed(() => {
    const c = this.config();
    return c ? loggedOffLabel(c.logged_off) : '—';
  });
  protected readonly pending = computed(() => {
    const p = this.runner()?.pending;
    return p ? pendingLabel(p) : null;
  });

  constructor() {
    effect(() => {
      const r = this.runner();
      const key = r?.currentJob ? `${r.name}:${r.currentJob.startedAt}` : null;
      untracked(() => {
        if (key && r) this.backend.runDetails(r.name).then((d) => this.details.set(d), () => this.details.set(null));
        else this.details.set(null);
      });
    });
    effect(() => {
      const name = this.name();
      untracked(() => this.backend.metrics(name, 24).then((m) => this.usage.set(m.usage), () => this.usage.set([])));
    });
    effect(() => {
      if (!this.settings.settings().value && !this.settings.settings().loading) untracked(() => void this.settings.load());
    });
  }

  protected select(tab: DetailTab): void {
    void this.router.navigate([], { queryParams: { tab: tab === 'overview' ? null : tab }, queryParamsHandling: 'merge', replaceUrl: true });
  }

  protected onTabKey(e: KeyboardEvent, i: number): void {
    const n = TABS.length;
    const next = e.key === 'ArrowRight' ? (i + 1) % n : e.key === 'ArrowLeft' ? (i - 1 + n) % n : e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    this.select(TABS[next]!.key);
    queueMicrotask(() => document.getElementById(`tab-${TABS[next]!.key}`)?.focus());
  }

  protected elapsed(since: number): string {
    return formatElapsed(this.clock.now() - since);
  }
  protected duration(since: number | null): string {
    return since === null ? '—' : formatDuration(this.clock.now() - since);
  }
  protected ago(at: number | null): string {
    return formatRelative(at, this.clock.now());
  }
  protected when(at: number | null): string {
    return formatDateTime(at);
  }
  protected openRun(url: string): void {
    void this.backend.openUrl(url);
  }
  protected reveal(): void {
    void this.backend.revealLogs(this.name()).catch(() => undefined);
  }
}
