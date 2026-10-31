import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { SHELL_COMMAND_RUNNER, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { ChangeDetectionStrategy, Component, ElementRef, Injector, afterNextRender, computed, inject, signal, viewChild } from '@angular/core';
import { SavedNote } from '../../shared/ui/saved-note';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import type { RunnerSnapshot, UpdatePolicy } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatCount, formatDateTime, formatRelative, plural } from '../../shared/format';
import { outcomeLook, updateModeLabel } from '../../shared/labels';
import { UpdatePolicyEditor } from '../../shared/policy/update-policy-editor';
import { confirmAction } from '../../shared/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { UpdateProgress } from '../../shared/ui/update-progress';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';
import { Notify } from '../../state/notify';

/** Runner versions and updates in progress, the default update schedule, and Pitcrew's own updates. */
@Component({
  selector: 'pc-updates-page',
  imports: [MatIcon, RouterLink, SavedNote, CdkMenu, CdkMenuItem, CdkMenuTrigger, UpdateProgress, UpdatePolicyEditor, LoadState, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown.escape)': 'close()' },
  templateUrl: './updates-page.html',
  styleUrl: './updates-page.scss',
})
export class UpdatesPage {
  protected readonly status = inject(StatusStore);
  protected readonly settings = inject(SettingsStore);
  protected readonly clock = inject(ClockService);

  private readonly runCommand = inject(SHELL_COMMAND_RUNNER);

  protected readonly plural = plural;
  private readonly router = inject(Router);
  private readonly ports = inject(SHELL_PORTS);
  private readonly toasts = inject(Notify);
  private readonly injector = inject(Injector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly panelEl = viewChild<ElementRef<HTMLElement>>('panel');

  /** Runner shown in the detail panel. */
  protected readonly selected = signal<string | null>(null);
  protected readonly detail = computed(() => this.status.runners().find((r) => r.name === this.selected()) ?? null);
  protected readonly copied = signal(false);

  /** One table, the runners that need a look first (failing, updating, update waiting), then by name. */
  protected readonly rows = computed(() =>
    this.status
      .runners()
      .map((r) => ({ r, rank: this.rank(r) }))
      .sort((a, b) => a.rank - b.rank || a.r.name.localeCompare(b.r.name))
      .map((x) => x.r),
  );
  /** Runners "Update all" applies: a managed binary with an update waiting and nothing running. */
  protected readonly updatable = computed(() => this.status.runners().filter((r) => this.canUpdate(r)));
  protected readonly available = computed(() => this.status.runners().filter((r) => r.updateTarget !== null && r.updatePhase.phase === 'idle').length);
  protected readonly inProgress = computed(() => this.status.runners().filter((r) => r.updatePhase.phase !== 'idle').length);
  protected readonly subtitle = computed(() => {
    const n = this.available();
    const base = n === 0 ? 'All runners are up to date' : `${plural(n, 'update')} available`;
    return this.inProgress() > 0 ? `${formatCount(this.inProgress())} in progress · ${base}` : base;
  });
  protected readonly updateAllLabel = computed(() => `Update all (${formatCount(this.updatable().length)})`);

  constructor() {
    if (!this.settings.settings().value) void this.settings.load();
  }

  /** Pitcrew's own updates are the shell's Updates window. */
  protected openAppUpdates(): void {
    void this.runCommand('pd.updates.open');
  }

  protected saveDefaults(policy: UpdatePolicy): void {
    const config = this.settings.config();
    if (config) void this.settings.save({ ...config, defaults: { ...config.defaults, update: policy } }, 'Update schedule saved');
  }

  protected look(r: RunnerSnapshot) {
    return r.lastUpdate ? outcomeLook(r.lastUpdate.outcome) : null;
  }
  protected mode(r: RunnerSnapshot): string {
    return updateModeLabel(r.updatePolicy.mode) + (r.updatePolicy.pinned ? ` · pinned to ${r.updatePolicy.pinned}` : '');
  }
  /** The countdown to the owner's reminder, while an update waits (ADR 0014). */
  protected deadline(r: RunnerSnapshot): { text: string; warn: boolean } | null {
    const d = r.deadline;
    if (!d || !r.updateTarget) return null;
    const n = Math.abs(d.daysLeft);
    const text = d.state === 'overdue' ? `Overdue by ${plural(n, 'day')}` : d.daysLeft === 0 ? 'Due today' : `Due in ${plural(n, 'day')}`;
    return { text, warn: d.state !== 'ok' };
  }
  /** The sentence behind the countdown: nobody enforces it. */
  protected deadlineNote(r: RunnerSnapshot): string | null {
    return r.deadline && r.updateTarget
      ? `A newer release has been available since ${formatDateTime(r.deadline.behindSince)}. Neither Gitea nor Forgejo enforces a deadline; this is your own reminder.`
      : null;
  }

  /** The failure text of the last update, when it did not finish. */
  protected failure(r: RunnerSnapshot): string | null {
    const o = r.lastUpdate?.outcome;
    return o?.result === 'failed' ? o.reason : null;
  }

  /** The status cell: a dot tone and short words; the full text is the tooltip and the detail panel. */
  protected state(r: RunnerSnapshot): { tone: 'success' | 'warning' | 'danger' | 'info' | 'neutral'; text: string; tip: string | null } {
    if (!r.managedBinary) return { tone: 'neutral', text: 'Your own binary', tip: 'Pitcrew does not manage this runner\u2019s binary, so it never updates it.' };
    if (r.updatePhase.phase !== 'idle') return { tone: 'info', text: `Updating: ${this.phaseLabel(r)}`, tip: null };
    if (r.updateLoop) return { tone: 'danger', text: `Failing: ${this.shortCause(r.updateLoop.cause)}`, tip: r.updateLoop.cause };
    const failed = this.failure(r);
    if (failed && r.updateTarget) return { tone: 'danger', text: `Failing: ${this.shortCause(failed)}`, tip: failed };
    const d = this.deadline(r);
    if (d) return { tone: d.warn ? 'warning' : 'neutral', text: d.text, tip: this.deadlineNote(r) };
    if (r.updateTarget) return { tone: 'neutral', text: 'Update available', tip: null };
    return { tone: 'success', text: 'Up to date', tip: null };
  }
  private shortCause(cause: string): string {
    const first = (cause.split(/[.:]/)[0] ?? cause).trim();
    return first.length > 40 ? `${first.slice(0, 39)}\u2026` : first;
  }
  private phaseLabel(r: RunnerSnapshot): string {
    const p = r.updatePhase.phase;
    return p === 'rollingBack' ? 'rolling back' : p === 'draining' ? 'waiting for idle' : p;
  }
  private rank(r: RunnerSnapshot): number {
    const s = this.state(r).tone;
    return s === 'danger' ? 0 : s === 'info' ? 1 : r.updateTarget ? 2 : 3;
  }

  protected busy(r: RunnerSnapshot): boolean {
    return this.status.inFlight()[r.name] !== undefined;
  }
  protected canUpdate(r: RunnerSnapshot): boolean {
    return r.managedBinary && r.updateTarget !== null && r.updatePhase.phase === 'idle' && !this.busy(r);
  }
  protected cancellable(r: RunnerSnapshot): boolean {
    const p = r.updatePhase.phase;
    return p === 'downloading' || p === 'verifying' || p === 'draining' || p === 'stopping';
  }
  protected ago(at: number | null): string {
    return formatRelative(at, this.clock.now());
  }
  protected when(at: number): string {
    return formatDateTime(at);
  }

  protected update(r: RunnerSnapshot): void {
    void this.status.act(r.name, 'applyUpdate');
  }
  protected skip(r: RunnerSnapshot): void {
    if (r.updateTarget) void this.status.skipVersion(r.name, r.updateTarget);
  }
  protected pinOrFollow(r: RunnerSnapshot): void {
    void this.status.pinVersion(r.name, r.updatePolicy.pinned ? null : r.currentVersion);
  }
  protected check(r: RunnerSnapshot): void {
    void this.status.act(r.name, 'checkUpdate');
  }
  protected cancel(r: RunnerSnapshot): void {
    void this.status.act(r.name, 'cancelUpdate');
  }
  protected history(r: RunnerSnapshot): void {
    void this.router.navigate(['/runners', r.name], { queryParams: { tab: 'updates' } });
  }

  /** The page's one primary: applies every waiting update, after naming the runners when there is more than one. */
  protected async updateAll(): Promise<void> {
    const list = this.updatable();
    if (list.length === 0) return;
    if (list.length > 1) {
      const ok = await confirmAction(this.ports, {
        title: `Update ${plural(list.length, 'runner')}?`,
        body: `${list.map((r) => `${r.name} (to ${r.updateTarget})`).join(', ')}. Each runner finishes its current job first.`,
        confirm: `Update ${formatCount(list.length)} runners`,
      });
      if (!ok) return;
    }
    for (const r of list) await this.status.act(r.name, 'applyUpdate');
  }

  protected open(name: string): void {
    this.selected.set(name);
    this.copied.set(false);
    afterNextRender(() => this.panelEl()?.nativeElement.focus(), { injector: this.injector });
  }
  protected onRowClick(e: MouseEvent, name: string): void {
    if (!(e.target as Element).closest('button, a')) this.open(name);
  }
  protected onRowKey(e: Event, name: string): void {
    if (e.target === e.currentTarget) {
      e.preventDefault();
      this.open(name);
    }
  }
  /** Closes the panel and puts focus back on the row that opened it. */
  protected close(): void {
    const name = this.selected();
    if (name === null) return;
    this.selected.set(null);
    afterNextRender(() => this.host.nativeElement.querySelector<HTMLElement>(`tr[data-runner="${CSS.escape(name)}"]`)?.focus(), { injector: this.injector });
  }

  protected async copy(textToCopy: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(textToCopy);
      this.copied.set(true);
    } catch {
      this.toasts.show({ tone: 'warning', icon: 'alert', title: 'Could not copy', body: 'Select the text and press Ctrl/Cmd+C instead.' });
    }
  }
}
