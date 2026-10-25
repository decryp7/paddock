import { ChangeDetectionStrategy, Component, computed, effect, inject, input, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { CleanupPolicy, DiskEntry, RunnerConfig } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatBytes, formatDateTime, formatRelative, plural, share } from '../../shared/format';
import { deleteReasonLabel, entryGroup, entryKindLabel, triggerLabel } from '../../shared/labels';
import { CleanupPolicyEditor } from '../../shared/policy/cleanup-policy-editor';
import { LoadState } from '../../shared/ui/load-state';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';
import { DiskStore } from './disk.store';

const GROUP_ORDER = ['Work folders', 'Caches', 'Logs', 'Previous binary', 'Downloads', 'Old binaries'];
const GROUP_COLOR = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-muted)', 'var(--chart-faint)'];

/** The Disk tab: what the folder holds, what a cleanup would delete and why, the last cleanup, and the policy. */
@Component({
  selector: 'pc-disk-panel',
  imports: [MatIcon, LoadState, CleanupPolicyEditor],
  providers: [DiskStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './disk-panel.html',
  styleUrl: './disk-panel.scss',
})
export class DiskPanel {
  readonly runner = input.required<string>();

  protected readonly store = inject(DiskStore);
  protected readonly settings = inject(SettingsStore);
  private readonly status = inject(StatusStore);
  private readonly clock = inject(ClockService);

  protected readonly cleaning = computed(() => this.status.inFlight()[this.runner()] === 'clean');
  protected readonly scan = computed(() => this.store.disk().value?.scan ?? null);

  protected readonly groups = computed(() => {
    const scan = this.scan();
    if (!scan) return [];
    const sums = new Map<string, number>();
    for (const e of scan.entries) sums.set(entryGroup(e.kind), (sums.get(entryGroup(e.kind)) ?? 0) + e.bytes);
    const total = Math.max(1, scan.folderBytes);
    return GROUP_ORDER.filter((g) => sums.has(g)).map((g) => ({
      name: g,
      bytes: formatBytes(sums.get(g) ?? 0),
      share: share(sums.get(g) ?? 0, total) * 100,
      color: GROUP_COLOR[GROUP_ORDER.indexOf(g)] ?? 'var(--chart-muted)',
    }));
  });

  /** The entries, biggest first, with the reason a cleanup now would delete them. */
  protected readonly entries = computed(() => {
    const reply = this.store.disk().value;
    if (!reply) return [];
    const reasons = new Map(reply.preview.deletions.map((d) => [d.entry.path, d.reason]));
    return [...reply.scan.entries]
      .sort((a, b) => b.bytes - a.bytes)
      .map((e) => ({ entry: e, kind: entryKindLabel(e.kind), reason: reasons.has(e.path) ? deleteReasonLabel(reasons.get(e.path)!) : null }));
  });

  protected readonly preview = computed(() => {
    const p = this.store.disk().value?.preview;
    return p ? { freed: formatBytes(p.freedBytes), count: plural(p.deletions.length, 'item'), still: p.stillNeededBytes > 0 ? formatBytes(p.stillNeededBytes) : null, empty: p.deletions.length === 0 } : null;
  });

  /** The newest cleanup report: the one run here, else the service's last one. */
  protected readonly report = computed(() => this.store.report() ?? this.store.disk().value?.lastCleanup ?? null);

  protected readonly override = computed(() => this.settings.runnerConfig(this.runner())?.cleanup ?? null);
  protected readonly policy = computed<CleanupPolicy | null>(() => this.override() ?? this.settings.config()?.defaults.cleanup ?? this.store.disk().value?.policy ?? null);

  constructor() {
    effect(() => {
      const name = this.runner();
      untracked(() => {
        void this.store.load(name);
        if (!this.settings.settings().value) void this.settings.load();
      });
    });
  }

  protected bytes(n: number | null): string {
    return formatBytes(n);
  }
  protected when(at: number): string {
    return formatDateTime(at);
  }
  protected ago(at: number): string {
    return formatRelative(at, this.clock.now());
  }
  protected trigger(r: { trigger: Parameters<typeof triggerLabel>[0] }): string {
    return triggerLabel(r.trigger);
  }
  protected track(e: DiskEntry): string {
    return e.path;
  }

  protected setOverride(on: boolean): void {
    const defaults = this.settings.config()?.defaults.cleanup;
    if (!defaults) return;
    void this.settings.saveRunner(this.runner(), (r: RunnerConfig) => ({ ...r, cleanup: on ? structuredClone(defaults) : null }), on ? 'This runner now has its own cleanup policy' : 'This runner follows the default cleanup policy');
  }

  protected savePolicy(policy: CleanupPolicy): void {
    void this.settings.saveRunner(this.runner(), (r) => ({ ...r, cleanup: policy }), 'Cleanup policy saved');
  }
}
