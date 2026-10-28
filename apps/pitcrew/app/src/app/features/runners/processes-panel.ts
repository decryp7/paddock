import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { ChangeDetectionStrategy, Component, computed, effect, inject, input, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { KeepFor, ProcessEntry, ProcessOverride, ProcessPolicy } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatBytes, formatDuration, formatPermille, plural } from '../../shared/format';
import { ProcessOverrideEditor } from '../../shared/policy/process-override-editor';
import { KEEP_EXTRAS, cleanupSummary, fateText } from '../../shared/process-format';
import { LoadState } from '../../shared/ui/load-state';
import { SettingsStore } from '../../state/settings.store';
import { Notify } from '../../state/notify';
import { ProcessesStore } from './processes.store';
import { confirmStopProcess } from './stop-process-dialog';

/** One table row: an entry with the words and numbers the table shows. */
export interface ProcessRow {
  entry: ProcessEntry;
  cpu: string;
  mem: string;
  age: string;
  job: string | null;
  fate: ReturnType<typeof fateText> | null;
  canKeep: boolean;
}

/** The Processes tab: the runner's process tree, the leftovers of finished jobs and what happens to them. */
@Component({
  selector: 'pc-processes-panel',
  imports: [MatIcon, LoadState, CdkMenu, CdkMenuItem, CdkMenuTrigger, ProcessOverrideEditor],
  providers: [ProcessesStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './processes-panel.html',
  styleUrl: './processes-panel.scss',
})
export class ProcessesPanel {
  readonly runner = input.required<string>();

  protected readonly store = inject(ProcessesStore);
  protected readonly settings = inject(SettingsStore);
  private readonly clock = inject(ClockService);
  private readonly ports = inject(SHELL_PORTS);
  private readonly toasts = inject(Notify);

  protected readonly extras = KEEP_EXTRAS;
  protected readonly list = computed(() => this.store.list().value);
  protected readonly rows = computed<ProcessRow[]>(() => {
    const l = this.list();
    if (!l) return [];
    const now = this.clock.now();
    return l.entries.map((entry) => ({
      entry,
      cpu: formatPermille(entry.cpuPermille),
      mem: formatBytes(entry.memBytes),
      age: entry.startedAt === null ? '—' : formatDuration(now - entry.startedAt),
      job: entry.job ? [entry.job.repository, entry.job.job, entry.job.runNumber === null ? null : `#${entry.job.runNumber}`].filter((x) => x).join(' · ') || null : null,
      fate: entry.leftover ? fateText(entry.leftover, now, l.idle, l.keep) : null,
      canKeep: entry.leftover?.fate.kind === 'stopsAt' && entry.startedAt !== null,
    }));
  });
  protected readonly summary = computed(() => {
    const l = this.list();
    if (!l) return null;
    const leftovers = l.entries.filter((e) => e.leftover).length;
    return { text: `${plural(l.entries.length, 'process', 'processes')}, ${plural(leftovers, 'leftover')}`, leftovers, cleanup: cleanupSummary(l.cleanup, l.keep) };
  });
  protected readonly override = computed<ProcessOverride | null>(() => this.settings.runnerConfig(this.runner())?.processes ?? null);
  protected readonly defaults = computed<ProcessPolicy | null>(() => this.settings.config()?.defaults.processes ?? null);

  constructor() {
    effect(() => {
      const name = this.runner();
      untracked(() => {
        this.store.watch(name);
        if (!this.settings.settings().value) void this.settings.load();
      });
    });
  }

  protected keep(entry: ProcessEntry, extra: KeepFor): void {
    void this.store.keep(entry, extra);
  }

  protected async stop(entry: ProcessEntry): Promise<void> {
    const ok = await confirmStopProcess(this.ports, { name: entry.name, pid: entry.pid, isRunner: entry.isRunner, jobRunning: !!this.list() && !this.list()!.idle });
    if (ok) await this.store.stop(entry, entry.isRunner);
  }

  protected async copy(entry: ProcessEntry): Promise<void> {
    try {
      await navigator.clipboard.writeText(entry.command);
      this.toasts.show({ tone: 'success', icon: 'check-circle', title: 'Command copied', body: null });
    } catch {
      this.toasts.show({ tone: 'warning', icon: 'alert', title: 'Could not copy', body: 'Select the command and press Ctrl/Cmd+C instead.' });
    }
  }

  protected async saveOverride(override: ProcessOverride | null): Promise<void> {
    const ok = await this.settings.saveRunner(this.runner(), (r) => ({ ...r, processes: override }), override ? 'Process overrides saved' : 'This runner follows the default process policy');
    if (ok) await this.store.retry();
  }
}
