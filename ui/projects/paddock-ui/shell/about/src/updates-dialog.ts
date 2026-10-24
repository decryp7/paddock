import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { SuJobPhase } from '@decryp7/paddock-ui';
import { UpdatesStore } from './updates.store';
import { ABOUT_STYLES } from './styles';
import { formatBytes } from './format';

/**
 * Updates (a tool: modeless, kept open beside the work). Every step is the engine's: Check, Download (a job: the download
 * and the signature check, its progress in the Jobs list and here when this window has jobs), Restart to update or
 * install when the app quits, and the preferences. A refused step shows the engine's message and next step. A build
 * without an update key says "Updates are not available in this build" and offers nothing to press.
 */
@Component({
  selector: 'pd-updates-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.status(); as s) {
      <p class="muted" data-testid="updates-current">You have version {{ s.currentVersion }}.</p>
      <div aria-live="polite" data-testid="updates-phase">
        @switch (s.phase.kind) {
          @case ('unavailable') {
            <p>Updates are not available in this build.</p>
          }
          @case ('idle') {
            <p>Not checked yet.</p>
          }
          @case ('checking') {
            <p role="status">Checking for updates…</p>
          }
          @case ('upToDate') {
            <p role="status">You have the latest version.</p>
          }
          @case ('available') {
            <p>Version {{ store.info()?.version }} is available{{ size() }}.</p>
          }
          @case ('downloading') {
            <p role="status">Downloading version {{ store.info()?.version }}…</p>
            @if (progress(); as p) {
              <p class="muted">{{ p.label }}</p>
              <progress [attr.value]="p.fraction" max="1" [attr.aria-label]="p.label"></progress>
            } @else {
              <p class="muted">The progress is in Jobs.</p>
            }
          }
          @case ('ready') {
            <p>Version {{ store.info()?.version }} is downloaded and its signature verified.</p>
          }
          @case ('installing') {
            <p role="status">Installing version {{ store.info()?.version }}…</p>
          }
          @case ('failed') {
            <p>The last step did not finish.</p>
          }
        }
      </div>
      @if (store.info()?.notes; as notes) {
        <pre class="text" tabindex="0" aria-label="Release notes">{{ notes }}</pre>
      }
      @if (store.error(); as e) {
        <div class="problem" role="alert" data-testid="updates-error">
          <p class="message">{{ e.message }}</p>
          <p>{{ e.nextStep }}</p>
          @if (e.detail) {
            <p class="detail">{{ e.detail }}</p>
          }
        </div>
      } @else if (s.phase.kind === 'unavailable') {
        <p class="muted">This build was made without an update signing key, so it cannot verify updates. Download new versions from the releases page.</p>
      }
      @if (s.phase.kind !== 'unavailable') {
        <div class="row">
          <label class="check">
            <input type="checkbox" [checked]="s.autoCheck" (change)="setAuto($any($event.target).checked)" />
            Check for updates automatically
          </label>
          @if (s.channels.length > 1) {
            <label class="check">
              Channel
              <select [value]="s.channel" (change)="setChannel($any($event.target).value)">
                @for (c of s.channels; track c.id) {
                  <option [value]="c.id" [selected]="c.id === s.channel">{{ c.label }}</option>
                }
              </select>
            </label>
          }
        </div>
        @if (s.phase.kind === 'ready') {
          <label class="check">
            <input type="checkbox" [checked]="installOnQuit()" (change)="store.installOnQuit($any($event.target).checked)" />
            Install when the app quits
          </label>
        }
        <p class="muted">{{ checkedText() }}@if (s.endpointHost) { · asks {{ s.endpointHost }}}</p>
      }
      <div class="actions">
        @switch (s.phase.kind) {
          @case ('checking') {
            <button type="button" class="btn" (click)="store.cancel()">Cancel</button>
          }
          @case ('downloading') {
            <button type="button" class="btn" (click)="store.cancel()">Cancel</button>
          }
          @case ('ready') {
            <button type="button" class="btn" (click)="store.cancel()">Discard</button>
            <button type="button" class="btn primary" (click)="store.restart()">Restart to update</button>
          }
          @case ('available') {
            <button type="button" class="btn" (click)="store.check()">Check again</button>
            <button type="button" class="btn primary" (click)="store.download()">Download</button>
          }
          @case ('failed') {
            @if (store.canDownload()) {
              <button type="button" class="btn primary" (click)="store.download()">Try again</button>
            } @else {
              <button type="button" class="btn primary" (click)="store.check()">Check again</button>
            }
          }
          @case ('idle') {
            <button type="button" class="btn primary" (click)="store.check()">Check now</button>
          }
          @case ('upToDate') {
            <button type="button" class="btn primary" (click)="store.check()">Check now</button>
          }
        }
      </div>
    } @else if (store.requestError(); as error) {
      <p role="alert">{{ error }} <button type="button" class="btn" (click)="store.load()">Retry</button></p>
    } @else {
      <p class="muted" role="status">Asking the updater…</p>
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdUpdatesDialog {
  protected readonly store = inject(UpdatesStore);
  /** The window's jobs, when it has them: the download's phases. */
  private readonly jobs = inject(JobsStore, { optional: true });

  protected readonly size = computed(() => {
    const size = this.store.info()?.size;
    return size ? ` (${formatBytes(size)})` : '';
  });

  protected readonly installOnQuit = computed(() => {
    const p = this.store.phase();
    return p?.kind === 'ready' && p.installOnQuit;
  });

  protected readonly checkedText = computed(() => {
    const t = this.store.status()?.lastChecked;
    return t ? `Last checked ${new Date(t * 1000).toLocaleString()}` : 'Not checked in this installation yet';
  });

  /** The download job's phase, from the window's jobs (null without them or before the first event). */
  protected readonly progress = computed(() => {
    const id = this.store.job();
    const job = id === null ? undefined : this.jobs?.jobs().find((j) => j.id === id);
    const phase: SuJobPhase | null | undefined = job?.state === 'running' ? job.phase : null;
    if (!phase) return null;
    const fraction = phase.total ? Math.min(1, phase.done / phase.total) : null;
    const amount = phase.unit === 'bytes' ? formatBytes(phase.done) : String(phase.done);
    return { label: `${phase.label}: ${amount}${phase.total && phase.unit === 'bytes' ? ` of ${formatBytes(phase.total)}` : ''}`, fraction };
  });

  protected setAuto(autoCheck: boolean): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck, channel: s.channel });
  }

  protected setChannel(channel: string): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck: s.autoCheck, channel });
  }
}
