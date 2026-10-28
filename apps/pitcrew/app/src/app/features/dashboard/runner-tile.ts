import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import type { FinishedJob, RunDetails, RunnerSnapshot } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { formatBytes, formatDuration, formatElapsed, formatPermille, formatRelative, plural, share } from '../../shared/format';
import { holdWords, pendingLabel, phaseLabel, sessionLabel } from '../../shared/labels';
import { RunnerActions } from '../../shared/runner-actions';
import { StatusBadge } from '../../shared/ui/status-badge';
import { StatusStore } from '../../state/status.store';

/** One runner on the dashboard: status, current job, version, uptime, disk, and quick actions. */
@Component({
  selector: 'pc-runner-tile',
  imports: [MatIcon, RouterLink, StatusBadge, RunnerActions],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './runner-tile.html',
  styleUrl: './runner-tile.scss',
})
export class RunnerTile {
  readonly runner = input.required<RunnerSnapshot>();
  readonly details = input<RunDetails | null | undefined>(undefined);
  readonly lastJob = input<FinishedJob | undefined>(undefined);
  readonly now = input.required<number>();

  protected readonly store = inject(StatusStore);
  private readonly backend = inject(BACKEND);

  protected readonly busy = computed(() => this.store.inFlight()[this.runner().name]);
  protected readonly elapsed = computed(() => {
    const job = this.runner().currentJob;
    return job ? formatElapsed(this.now() - job.startedAt) : '';
  });
  protected readonly uptime = computed(() => {
    const since = this.runner().runningSince;
    return since === null ? '—' : formatDuration(this.now() - since);
  });
  protected readonly disk = computed(() => {
    const d = this.runner().disk;
    if (!d) return null;
    const total = d.freeBytes === null ? null : d.folderBytes + d.freeBytes;
    return {
      used: formatBytes(d.folderBytes),
      free: d.freeBytes === null ? null : formatBytes(d.freeBytes),
      share: total === null ? 0 : share(d.folderBytes, total),
      low: d.low,
    };
  });
  /** What the runner and its processes use now (ADR 0007). */
  protected readonly load = computed(() => {
    const u = this.runner().usage;
    return u ? { cpu: formatPermille(u.cpuPermille), mem: formatBytes(u.memBytes, 'memory'), processes: `${u.processes} ${u.processes === 1 ? 'process' : 'processes'}` } : null;
  });
  protected readonly nextStart = computed(() => formatRelative(this.runner().nextStartAt, this.now()));
  protected readonly lastJobAgo = computed(() => {
    const j = this.lastJob();
    return j ? formatRelative(j.finishedAt, this.now()) : null;
  });
  protected readonly phase = computed(() => {
    const p = this.runner().updatePhase;
    return p.phase === 'idle' ? null : `${phaseLabel(p)} · ${p.plan.to}`;
  });
  protected readonly session = computed(() => sessionLabel(this.runner().session));
  protected readonly pending = computed(() => {
    const p = this.runner().pending;
    return p ? pendingLabel(p) : null;
  });
  /** Why Pitcrew itself holds the runner (low disk, a pause window, a busy machine). */
  protected readonly hold = computed(() => {
    const h = this.runner().hold;
    const first = h?.reasons[0];
    if (!h || !first) return null;
    const why = holdWords(first);
    const left = h.minutesLeft !== null && first === 'schedule' ? ` · ${formatDuration(h.minutesLeft * 60_000)} left` : '';
    const text = h.paused ? `Paused by Pitcrew: ${why}${left}` : `Pausing after this job: ${why}${left}`;
    return { text, detail: 'Pitcrew resumes the runner by itself when the reason is gone. Resume it yourself to override.' };
  });
  /** The countdown to the owner's update deadline (ADR 0014). */
  protected readonly deadline = computed(() => {
    const d = this.runner().deadline;
    if (!d || !this.runner().updateTarget) return null;
    const days = Math.abs(d.daysLeft);
    const text = d.state === 'overdue' ? `Update overdue by ${plural(days, 'day')}` : d.daysLeft === 0 ? 'Update due today' : `Update due in ${plural(days, 'day')}`;
    return { text, warn: d.state !== 'ok' };
  });
  protected readonly paused = computed(() => this.runner().status === 'paused' || this.runner().status === 'draining');

  protected openRun(url: string): void {
    void this.backend.openUrl(url);
  }
}
