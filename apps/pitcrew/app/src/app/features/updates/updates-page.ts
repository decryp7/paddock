import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import type { AppUpdateInfo, AppUpdateStatus, CommandError, RunnerSnapshot, UpdatePolicy } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ClockService } from '../../core/clock.service';
import { toCommandError } from '../../core/command-error';
import { formatDateTime, formatRelative } from '../../shared/format';
import { outcomeLook, updateModeLabel } from '../../shared/labels';
import { UpdatePolicyEditor } from '../../shared/policy/update-policy-editor';
import { EmptyState } from '../../shared/ui/empty-state';
import { LoadState } from '../../shared/ui/load-state';
import { UpdateProgress } from '../../shared/ui/update-progress';
import { idle, pending, settle, type Loadable } from '../../state/loadable';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';

/** Runner versions and updates in progress, the default update schedule, and Pitcrew's own updates. */
@Component({
  selector: 'pc-updates-page',
  imports: [MatIcon, RouterLink, UpdateProgress, UpdatePolicyEditor, LoadState, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './updates-page.html',
  styleUrl: './updates-page.scss',
})
export class UpdatesPage {
  protected readonly status = inject(StatusStore);
  protected readonly settings = inject(SettingsStore);
  protected readonly clock = inject(ClockService);
  private readonly backend = inject(BACKEND);

  protected readonly app = signal<Loadable<AppUpdateStatus>>(idle());
  protected readonly found = signal<AppUpdateInfo | null | undefined>(undefined);
  protected readonly appBusy = signal<'check' | 'install' | null>(null);
  protected readonly appError = signal<CommandError | null>(null);

  protected readonly available = computed(() => this.status.runners().filter((r) => r.updateTarget !== null && r.updatePhase.phase === 'idle').length);
  protected readonly inProgress = computed(() => this.status.runners().filter((r) => r.updatePhase.phase !== 'idle').length);

  constructor() {
    void this.loadApp();
    if (!this.settings.settings().value) void this.settings.load();
  }

  protected async loadApp(): Promise<void> {
    this.app.set(pending(this.app()));
    this.app.set(await settle(this.app(), () => this.backend.appUpdateStatus()));
  }

  protected async checkApp(): Promise<void> {
    this.appBusy.set('check');
    this.appError.set(null);
    try {
      this.found.set(await this.backend.checkAppUpdate());
    } catch (e) {
      this.appError.set(toCommandError(e));
    } finally {
      this.appBusy.set(null);
    }
  }

  protected async installApp(): Promise<void> {
    this.appBusy.set('install');
    this.appError.set(null);
    try {
      await this.backend.installAppUpdate();
    } catch (e) {
      this.appError.set(toCommandError(e));
    } finally {
      this.appBusy.set(null);
    }
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
  protected busy(r: RunnerSnapshot): boolean {
    return this.status.inFlight()[r.name] !== undefined;
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
}
