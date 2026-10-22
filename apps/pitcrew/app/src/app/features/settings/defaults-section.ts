import { ChangeDetectionStrategy, Component, computed, inject, linkedSignal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import type { CleanupPolicy, Config, DiskGuardPolicy, PausePolicy, ProcessPolicy } from '../../core/api.types';
import { CleanupPolicyEditor } from '../../shared/policy/cleanup-policy-editor';
import { DiskGuardEditor } from '../../shared/policy/disk-guard-editor';
import { PauseEditor } from '../../shared/policy/pause-editor';
import { ProcessPolicyEditor } from '../../shared/policy/process-policy-editor';
import { LoadState } from '../../shared/ui/load-state';
import { SettingsStore } from '../../state/settings.store';

type Defaults = Config['defaults'];

/** One number in the defaults, shown in a friendlier unit (stored value = shown × scale). */
export interface DefaultsField {
  id: string;
  label: string;
  unit: string;
  scale: number;
  get: (d: Defaults) => number;
  set: (d: Defaults, v: number) => void;
}

export interface DefaultsGroup {
  title: string;
  hint: string;
  fields: DefaultsField[];
}

export const GROUPS: DefaultsGroup[] = [
  {
    title: 'Watchdog',
    hint: 'A runner whose polls keep timing out while no job runs is stalled and restarted.',
    fields: [
      { id: 'stall-threshold', label: 'Stalled after', unit: 'timed-out polls', scale: 1, get: (d) => d.watchdog.threshold, set: (d, v) => (d.watchdog.threshold = v) },
      { id: 'stall-window', label: 'Within', unit: 'minutes', scale: 60, get: (d) => d.watchdog.window_secs, set: (d, v) => (d.watchdog.window_secs = v) },
    ],
  },
  {
    title: 'Restarts',
    hint: 'Restarts back off; repeated exits are a crash loop, and restarts pause for the cool-down.',
    fields: [
      { id: 'restart-initial', label: 'First delay', unit: 'seconds', scale: 1, get: (d) => d.restart.initial_delay_secs, set: (d, v) => (d.restart.initial_delay_secs = v) },
      { id: 'restart-max', label: 'Longest delay', unit: 'seconds', scale: 1, get: (d) => d.restart.max_delay_secs, set: (d, v) => (d.restart.max_delay_secs = v) },
      { id: 'restart-reset', label: 'Reset after running', unit: 'minutes', scale: 60, get: (d) => d.restart.reset_after_secs, set: (d, v) => (d.restart.reset_after_secs = v) },
      { id: 'loop-exits', label: 'Crash loop after', unit: 'exits', scale: 1, get: (d) => d.restart.crash_loop_restarts, set: (d, v) => (d.restart.crash_loop_restarts = v) },
      { id: 'loop-window', label: 'Within', unit: 'minutes', scale: 60, get: (d) => d.restart.crash_loop_window_secs, set: (d, v) => (d.restart.crash_loop_window_secs = v) },
      { id: 'loop-cooldown', label: 'Pause restarts for', unit: 'minutes', scale: 60, get: (d) => d.restart.crash_loop_cooldown_secs, set: (d, v) => (d.restart.crash_loop_cooldown_secs = v) },
    ],
  },
  {
    title: 'Update timeouts',
    hint: 'An update waits for idle (never killing a job), stops the runner gracefully, then waits for the server to accept the new one.',
    fields: [
      { id: 'drain', label: 'Wait for idle up to', unit: 'hours', scale: 3600, get: (d) => d.update_timeouts.drain_secs, set: (d, v) => (d.update_timeouts.drain_secs = v) },
      { id: 'stop', label: 'Graceful stop up to', unit: 'seconds', scale: 1, get: (d) => d.update_timeouts.stop_secs, set: (d, v) => (d.update_timeouts.stop_secs = v) },
      { id: 'confirm', label: 'Confirm within', unit: 'minutes', scale: 60, get: (d) => d.update_timeouts.confirm_secs, set: (d, v) => (d.update_timeouts.confirm_secs = v) },
    ],
  },
  {
    title: 'Runner logs',
    hint: 'Each runner’s log file rotates at this size; older files are kept up to the count.',
    fields: [
      { id: 'log-size', label: 'Rotate at', unit: 'MB', scale: 1024 * 1024, get: (d) => d.logs.max_bytes, set: (d, v) => (d.logs.max_bytes = v) },
      { id: 'log-keep', label: 'Keep', unit: 'files', scale: 1, get: (d) => d.logs.keep, set: (d, v) => (d.logs.keep = v) },
    ],
  },
];

/** The defaults every runner uses unless it overrides them: watchdog, restarts, update timeouts, logs, cleanup. */
@Component({
  selector: 'pc-defaults-section',
  imports: [MatIcon, RouterLink, LoadState, CleanupPolicyEditor, ProcessPolicyEditor, DiskGuardEditor, PauseEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pc-load-state what="The settings" [hasValue]="!!settings.config()" [error]="settings.settings().error" [skeleton]="[80, 160, 160]" (retry)="settings.load()">
      @if (settings.readOnly()) {
        <div class="callout warning" role="status">
          <mat-icon svgIcon="pc:lock" aria-hidden="true" />
          <div class="callout-body">
            <strong>Read-only</strong>
            <span>pitcrew.toml was written by a newer Pitcrew. Update Pitcrew to change these settings.</span>
          </div>
        </div>
      }
      @if (settings.problems().length > 0) {
        <div class="callout warning" role="status">
          <mat-icon svgIcon="pc:alert" aria-hidden="true" />
          <div class="callout-body">
            <strong>Problems in pitcrew.toml</strong>
            @for (p of settings.problems(); track p) {
              <span class="mono small">{{ p }}</span>
            }
          </div>
        </div>
      }
      @if (draft(); as d) {
        <section class="card">
          <div class="card-body groups">
            @for (g of groups; track g.title) {
              <fieldset class="group" [disabled]="settings.readOnly()">
                <legend>{{ g.title }}</legend>
                <p class="faint small">{{ g.hint }}</p>
                <div class="field-grid">
                  @for (f of g.fields; track f.id) {
                    <div class="field">
                      <label [for]="f.id">{{ f.label }}</label>
                      <div class="with-unit">
                        <input class="input" type="number" min="1" [id]="f.id" [value]="shown(d, f)" (change)="set(f, $any($event.target).value)" />
                        <span>{{ f.unit }}</span>
                      </div>
                    </div>
                  }
                </div>
              </fieldset>
            }
            @if (!settings.readOnly()) {
              <div class="btn-row">
                <button type="button" class="btn primary" (click)="save()" [disabled]="!dirty() || settings.saving()">{{ settings.saving() ? 'Saving…' : 'Save defaults' }}</button>
                @if (dirty()) {
                  <button type="button" class="btn ghost" (click)="reset()">Discard changes</button>
                }
              </div>
            }
          </div>
        </section>

        <section class="card">
          <header class="card-header">
            <h2>Cleanup</h2>
            <span class="meta">runners can override this on their Disk tab</span>
          </header>
          <div class="card-body">
            <pc-cleanup-policy-editor id="defaults-cleanup" [policy]="d.cleanup" [readOnly]="settings.readOnly()" [saving]="settings.saving()" (save)="saveCleanup($event)" />
          </div>
        </section>
        <section class="card" aria-labelledby="processes-title">
          <header class="card-header">
            <h2 id="processes-title">Leftover processes</h2>
            <span class="meta">runners can override this on their Processes tab</span>
          </header>
          <div class="card-body">
            <pc-process-policy-editor id="defaults-processes" [policy]="d.processes" [readOnly]="settings.readOnly()" [saving]="settings.saving()" (save)="saveProcesses($event)" />
          </div>
        </section>
        <section class="card" aria-labelledby="guard-title">
          <header class="card-header">
            <h2 id="guard-title">Disk guard</h2>
            <span class="meta">before the disk is full</span>
          </header>
          <div class="card-body">
            <pc-disk-guard-editor id="defaults-guard" [policy]="d.disk_guard" [readOnly]="settings.readOnly()" [saving]="settings.saving()" (save)="saveDiskGuard($event)" />
          </div>
        </section>
        <section class="card" aria-labelledby="pause-title">
          <header class="card-header">
            <h2 id="pause-title">Pause windows</h2>
            <span class="meta">when runners take no new jobs</span>
          </header>
          <div class="card-body">
            <pc-pause-editor id="defaults-pause" [policy]="d.pause" [readOnly]="settings.readOnly()" [saving]="settings.saving()" (save)="savePause($event)" />
          </div>
        </section>
        <p class="faint small">The update schedule is on the <a routerLink="/updates">Updates</a> page.</p>
      }
    </pc-load-state>
  `,
  styles: `
    pc-load-state {
      gap: var(--space-4);
    }
    h2 {
      font-size: var(--text-md);
    }
    .groups {
      display: grid;
      gap: var(--space-5);
    }
    .group {
      display: grid;
      gap: var(--space-2);
      margin: 0;
      padding: 0 0 var(--space-5);
      border: 0;
      border-bottom: 1px solid var(--border);
      min-width: 0;
    }
    legend {
      padding: 0;
      font-weight: var(--weight-semibold);
    }
    .small {
      font-size: var(--text-sm);
    }
    .field-grid {
      grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    }
  `,
})
export class DefaultsSection {
  protected readonly settings = inject(SettingsStore);
  protected readonly groups = GROUPS;
  protected readonly draft = linkedSignal(() => {
    const c = this.settings.config();
    return c ? structuredClone(c.defaults) : null;
  });
  protected readonly dirty = computed(() => {
    const c = this.settings.config();
    return !!c && JSON.stringify(this.draft()) !== JSON.stringify(c.defaults);
  });

  constructor() {
    if (!this.settings.settings().value) void this.settings.load();
  }

  protected shown(d: Defaults, f: DefaultsField): number {
    return Math.round((f.get(d) / f.scale) * 100) / 100;
  }

  protected set(f: DefaultsField, value: string): void {
    const n = Number(value);
    if (!Number.isFinite(n) || n <= 0) return;
    this.draft.update((d) => {
      if (!d) return d;
      const next = structuredClone(d);
      f.set(next, Math.round(n * f.scale));
      return next;
    });
  }

  protected reset(): void {
    const c = this.settings.config();
    this.draft.set(c ? structuredClone(c.defaults) : null);
  }

  protected save(): void {
    const c = this.settings.config();
    const d = this.draft();
    if (c && d) void this.settings.save({ ...c, defaults: { ...d, cleanup: c.defaults.cleanup, update: c.defaults.update, processes: c.defaults.processes, disk_guard: c.defaults.disk_guard, pause: c.defaults.pause } }, 'Defaults saved');
  }

  protected saveProcesses(policy: ProcessPolicy): void {
    const c = this.settings.config();
    if (c) void this.settings.save({ ...c, defaults: { ...c.defaults, processes: policy } }, 'Process defaults saved');
  }

  protected saveDiskGuard(policy: DiskGuardPolicy): void {
    const c = this.settings.config();
    if (c) void this.settings.save({ ...c, defaults: { ...c.defaults, disk_guard: policy } }, 'Disk guard saved');
  }

  protected savePause(policy: PausePolicy): void {
    const c = this.settings.config();
    if (c) void this.settings.save({ ...c, defaults: { ...c.defaults, pause: policy } }, 'Pause windows saved');
  }

  protected saveCleanup(policy: CleanupPolicy): void {
    const c = this.settings.config();
    if (c) void this.settings.save({ ...c, defaults: { ...c.defaults, cleanup: policy } }, 'Cleanup defaults saved');
  }
}
