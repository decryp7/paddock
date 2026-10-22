import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import type { RunDetails } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ClockService } from '../../core/clock.service';
import { formatDateTime, formatDuration, formatElapsed, formatRelative } from '../../shared/format';
import { loggedOffLabel, pendingLabel, sessionLabel } from '../../shared/labels';
import { RunnerActions } from '../../shared/runner-actions';
import { EmptyState } from '../../shared/ui/empty-state';
import { StatusBadge } from '../../shared/ui/status-badge';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';
import { DiskPanel } from './disk-panel';
import { RunnerUpdatePanel } from './runner-update-panel';

export type DetailTab = 'overview' | 'updates' | 'disk';

const TABS: { key: DetailTab; label: string; icon: string }[] = [
  { key: 'overview', label: 'Overview', icon: 'runner' },
  { key: 'updates', label: 'Updates', icon: 'updates' },
  { key: 'disk', label: 'Disk & cleanup', icon: 'disk' },
];

/** One runner: overview, updates and disk, with its actions. */
@Component({
  selector: 'pc-runner-detail-page',
  imports: [MatIcon, RouterLink, StatusBadge, RunnerActions, EmptyState, DiskPanel, RunnerUpdatePanel],
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
