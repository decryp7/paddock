import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { RunnerSnapshot, UpdatePolicy } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatDateTime, formatRelative } from '../../shared/format';
import { outcomeLook, updateModeLabel } from '../../shared/labels';
import { UpdatePolicyEditor } from '../../shared/policy/update-policy-editor';
import { UpdateProgress } from '../../shared/ui/update-progress';
import { SettingsStore } from '../../state/settings.store';
import { StatusStore } from '../../state/status.store';

const VERSION = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

/** A runner's updates: versions and the update in progress, Update now / Cancel / Skip / Pin, and its policy. */
@Component({
  selector: 'pc-runner-update-panel',
  imports: [MatIcon, UpdateProgress, UpdatePolicyEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let r = runner();
    <section class="card" aria-labelledby="ver-title">
      <header class="card-header">
        <h3 id="ver-title">Version</h3>
        <span class="meta">{{ r.checkedAt ? 'checked ' + ago(r.checkedAt) : 'not checked yet' }}</span>
      </header>
      <div class="card-body body">
        @if (!r.managedBinary) {
          <div class="callout info">
            <mat-icon svgIcon="pc:info" aria-hidden="true" />
            <div class="callout-body">
              <strong>Pitcrew supervises this runner but does not update it.</strong>
              <span class="muted">It runs a binary you provided ({{ r.kind }}). Replace the binary yourself to update.</span>
            </div>
          </div>
        }
        <dl class="versions">
          <div><dt>Installed</dt><dd class="num">{{ r.currentVersion ?? '—' }}</dd></div>
          <div><dt>Latest</dt><dd class="num">{{ r.latestVersion ?? '—' }}</dd></div>
          <div>
            <dt>Target</dt>
            <dd class="num">
              @if (r.updateTarget) {
                <span class="badge accent">{{ r.updateTarget }}</span>
              } @else {
                <span class="faint">up to date</span>
              }
            </dd>
          </div>
          <div><dt>Policy</dt><dd>{{ mode() }}</dd></div>
        </dl>
        @if (r.updatePhase.phase !== 'idle') {
          <pc-update-progress [phase]="r.updatePhase" [now]="clock.now()" />
        }
        @if (r.lastUpdate; as u) {
          <div class="last">
            <span class="badge" [class]="last()!.tone">{{ last()!.label }}</span>
            <span class="muted">{{ last()!.detail }}</span>
            <span class="faint">{{ when(u.finishedAt) }}</span>
          </div>
        }
        @if (r.managedBinary) {
          <div class="btn-row">
            @if (r.updatePhase.phase === 'idle') {
              <button type="button" class="btn primary" (click)="status.act(r.name, 'applyUpdate')" [disabled]="!r.updateTarget || busy()">
                <mat-icon svgIcon="pc:download" aria-hidden="true" />{{ r.updateTarget ? 'Update to ' + r.updateTarget : 'Update now' }}
              </button>
            } @else {
              <button type="button" class="btn" (click)="status.act(r.name, 'cancelUpdate')" [disabled]="busy() || !cancellable()">
                <mat-icon svgIcon="pc:x" aria-hidden="true" />Cancel update
              </button>
            }
            <button type="button" class="btn" (click)="status.act(r.name, 'checkUpdate')" [disabled]="busy()">
              <mat-icon svgIcon="pc:refresh" aria-hidden="true" [class.spin]="status.inFlight()[r.name] === 'checkUpdate'" />Check now
            </button>
            @if (r.updateTarget && r.updatePhase.phase === 'idle') {
              <button type="button" class="btn ghost" (click)="status.skipVersion(r.name, r.updateTarget)" [disabled]="busy()">
                <mat-icon svgIcon="pc:skip" aria-hidden="true" />Skip {{ r.updateTarget }}
              </button>
            }
          </div>
          <p class="faint small">Updates wait until the runner is idle; a running job is never stopped for an update. If the server does not accept the new version, Pitcrew rolls back.</p>
        }
      </div>
    </section>

    @if (r.managedBinary) {
      <section class="card" aria-labelledby="pin-title">
        <header class="card-header"><h3 id="pin-title">Pin and skip</h3></header>
        <div class="card-body body">
          <div class="pin">
            <div class="field">
              <label for="pin-version">Pinned version</label>
              <div class="btn-row">
                <input id="pin-version" class="input" placeholder="e.g. 5.2.0" [value]="pinDraft()" (input)="pinDraft.set($any($event.target).value)" [attr.aria-invalid]="pinInvalid()" />
                <button type="button" class="btn" (click)="pin()" [disabled]="pinInvalid() || !pinDraft() || busy()"><mat-icon svgIcon="pc:pin" aria-hidden="true" />Pin</button>
                @if (r.updatePolicy.pinned) {
                  <button type="button" class="btn ghost" (click)="status.pinVersion(r.name, null)" [disabled]="busy()">Unpin</button>
                }
              </div>
              <span class="hint">{{ r.updatePolicy.pinned ? 'Stays on ' + r.updatePolicy.pinned + '. Pinning an older version is how you go back.' : 'Not pinned: follows the latest release on its channel.' }}</span>
              @if (pinInvalid()) {
                <span class="error-text">Use a version like 5.2.0.</span>
              }
            </div>
            <div class="field">
              <span class="label">Skipped versions</span>
              @if (r.updatePolicy.skipped.length === 0) {
                <span class="faint">None</span>
              } @else {
                <div class="chips">
                  @for (v of r.updatePolicy.skipped; track v) {
                    <span class="chip num">{{ v }}</span>
                  }
                </div>
              }
            </div>
          </div>
        </div>
      </section>

      <section class="card" aria-labelledby="policy-title">
        <header class="card-header">
          <h3 id="policy-title">Update policy</h3>
          <label class="check small">
            <input type="checkbox" [checked]="!!override()" (change)="setOverride($any($event.target).checked)" [disabled]="settings.readOnly() || settings.saving() || !settings.config()" />
            Override the defaults for this runner
          </label>
        </header>
        <div class="card-body">
          @if (policy(); as p) {
            <pc-update-policy-editor id="runner-upd" [policy]="p" [readOnly]="!override() || settings.readOnly()" [saving]="settings.saving()" (save)="savePolicy($event)" />
          }
        </div>
      </section>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-4);
    }
    .body {
      display: grid;
      gap: var(--space-4);
    }
    .versions {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: var(--space-4);
      margin: 0;
    }
    dt {
      font-size: var(--text-xs);
      color: var(--text-3);
      font-weight: var(--weight-medium);
    }
    dd {
      margin: 2px 0 0;
      font-size: var(--text-lg);
      font-weight: var(--weight-semibold);
    }
    .last {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
      font-size: var(--text-sm);
    }
    .small {
      font-size: var(--text-sm);
    }
    .pin {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: var(--space-6);
    }
    #pin-version {
      width: 140px;
    }
  `,
})
export class RunnerUpdatePanel {
  readonly runner = input.required<RunnerSnapshot>();

  protected readonly status = inject(StatusStore);
  protected readonly settings = inject(SettingsStore);
  protected readonly clock = inject(ClockService);

  protected readonly pinDraft = signal('');
  protected readonly pinInvalid = computed(() => this.pinDraft() !== '' && !VERSION.test(this.pinDraft().trim()));
  protected readonly busy = computed(() => this.status.inFlight()[this.runner().name] !== undefined);
  protected readonly mode = computed(() => updateModeLabel(this.runner().updatePolicy.mode) + (this.runner().updatePolicy.channel === 'prerelease' ? ' · pre-releases' : ''));
  protected readonly last = computed(() => {
    const u = this.runner().lastUpdate;
    return u ? outcomeLook(u.outcome) : null;
  });
  protected readonly cancellable = computed(() => {
    const p = this.runner().updatePhase.phase;
    return p === 'downloading' || p === 'verifying' || p === 'draining' || p === 'stopping';
  });
  protected readonly override = computed(() => this.settings.runnerConfig(this.runner().name)?.update ?? null);
  protected readonly policy = computed<UpdatePolicy | null>(() => this.override() ?? this.settings.config()?.defaults.update ?? this.runner().updatePolicy);

  constructor() {
    effect(() => {
      this.pinDraft.set(this.runner().updatePolicy.pinned ?? '');
    });
    effect(() => {
      if (!this.settings.settings().value && !this.settings.settings().loading) untracked(() => void this.settings.load());
    });
  }

  protected ago(at: number): string {
    return formatRelative(at, this.clock.now());
  }
  protected when(at: number): string {
    return formatDateTime(at);
  }

  protected pin(): void {
    const v = this.pinDraft().trim();
    if (VERSION.test(v)) void this.status.pinVersion(this.runner().name, v);
  }

  protected setOverride(on: boolean): void {
    const defaults = this.settings.config()?.defaults.update;
    if (!defaults) return;
    void this.settings.saveRunner(this.runner().name, (r) => ({ ...r, update: on ? structuredClone(defaults) : null }), on ? 'This runner now has its own update policy' : 'This runner follows the default update policy');
  }

  protected savePolicy(policy: UpdatePolicy): void {
    void this.settings.saveRunner(this.runner().name, (r) => ({ ...r, update: policy }), 'Update policy saved');
  }
}
