import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { PdSelect } from '@decryp7/paddock-ui/select';
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
  imports: [PdSelect],
  template: `
    @if (store.status(); as s) {
      <p class="pd-muted" data-testid="updates-current">{{ current(s.currentVersion) }}</p>
      <div aria-live="polite" data-testid="updates-phase">
        @switch (s.phase.kind) {
          @case ('unavailable') {
            <p>{{ t.unavailable }}</p>
          }
          @case ('idle') {
            <p>{{ t.idle }}</p>
          }
          @case ('checking') {
            <p role="status">{{ t.checking }}</p>
          }
          @case ('upToDate') {
            <p role="status">{{ t.upToDate }}</p>
          }
          @case ('available') {
            <p>{{ available() }}</p>
          }
          @case ('downloading') {
            <p role="status">{{ downloading() }}</p>
            @if (progress(); as p) {
              <p class="pd-muted">{{ p.label }}</p>
              <progress [attr.value]="p.fraction" max="1" [attr.aria-label]="p.label"></progress>
            } @else {
              <p class="pd-muted">{{ t.progressInJobs }}</p>
            }
          }
          @case ('ready') {
            <p>{{ ready() }}</p>
          }
          @case ('installing') {
            <p role="status">{{ installing() }}</p>
          }
          @case ('failed') {
            <p>{{ t.failed }}</p>
          }
        }
      </div>
      @if (store.info()?.notes; as notes) {
        <pre class="pd-text" tabindex="0" [attr.aria-label]="t.notes">{{ notes }}</pre>
      }
      @if (store.error(); as e) {
        <div class="pd-problem" role="alert" data-testid="updates-error">
          <p class="pd-message">{{ e.message }}</p>
          <p>{{ e.nextStep }}</p>
          @if (e.detail) {
            <p class="pd-detail">{{ e.detail }}</p>
          }
        </div>
      } @else if (s.phase.kind === 'unavailable') {
        <p class="pd-muted">{{ t.noKey }}</p>
      }
      @if (s.phase.kind !== 'unavailable') {
        <div class="pd-row">
          <label class="pd-check">
            <input type="checkbox" [checked]="s.autoCheck" (change)="setAuto($any($event.target).checked)" />
            {{ t.autoCheck }}
          </label>
          @if (s.channels.length > 1) {
            <label class="pd-check">
              {{ t.channel }}
              <pd-select [options]="channelOptions()" [value]="s.channel" [ariaLabel]="t.channel" (valueChange)="setChannel($event)" />
            </label>
          }
        </div>
        @if (s.phase.kind === 'ready') {
          <label class="pd-check">
            <input type="checkbox" [checked]="installOnQuit()" (change)="store.installOnQuit($any($event.target).checked)" />
            {{ t.installOnQuit }}
          </label>
        }
        <p class="pd-muted">{{ checkedText() }}</p>
      }
      <div class="pd-actions">
        @switch (s.phase.kind) {
          @case ('checking') {
            <button type="button" class="pd-btn" (click)="store.cancel()">{{ t.cancel }}</button>
          }
          @case ('downloading') {
            <button type="button" class="pd-btn" (click)="store.cancel()">{{ t.cancel }}</button>
          }
          @case ('ready') {
            <button type="button" class="pd-btn" (click)="store.cancel()">{{ t.discard }}</button>
            <button type="button" class="pd-btn pd-primary" (click)="store.restart()">{{ t.restart }}</button>
          }
          @case ('available') {
            <button type="button" class="pd-btn" (click)="store.check()">{{ t.checkAgain }}</button>
            <button type="button" class="pd-btn pd-primary" (click)="store.download()">{{ t.download }}</button>
          }
          @case ('failed') {
            @if (store.canDownload()) {
              <button type="button" class="pd-btn pd-primary" (click)="store.download()">{{ t.tryAgain }}</button>
            } @else {
              <button type="button" class="pd-btn pd-primary" (click)="store.check()">{{ t.checkAgain }}</button>
            }
          }
          @case ('idle') {
            <button type="button" class="pd-btn pd-primary" (click)="store.check()">{{ t.checkNow }}</button>
          }
          @case ('upToDate') {
            <button type="button" class="pd-btn pd-primary" (click)="store.check()">{{ t.checkNow }}</button>
          }
        }
      </div>
    } @else if (store.requestError(); as error) {
      <p role="alert">{{ error }} <button type="button" class="pd-btn" (click)="store.load()">{{ t.retry }}</button></p>
    } @else {
      <p class="pd-muted" role="status">{{ t.asking }}</p>
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdUpdatesDialog {
  /** The component's texts in the window's language (plan S15). The engine's errors and next steps arrive as they are. */
  protected readonly t = {
    unavailable: $localize`:@@pd.updates.unavailable:Updates are not available in this build.`,
    noKey: $localize`:@@pd.updates.unavailable.why:This build was made without an update signing key, so it cannot verify updates. Download new versions from the releases page.`,
    idle: $localize`:@@pd.updates.idle:Not checked yet.`,
    checking: $localize`:@@pd.updates.checking:Checking for updates…`,
    upToDate: $localize`:@@pd.updates.up-to-date:You have the latest version.`,
    progressInJobs: $localize`:@@pd.updates.progress-in-jobs:The progress is in Jobs.`,
    failed: $localize`:@@pd.updates.failed:The last step did not finish.`,
    notes: $localize`:@@pd.updates.notes:Release notes`,
    autoCheck: $localize`:@@pd.updates.auto-check:Check for updates automatically`,
    channel: $localize`:@@pd.updates.channel:Channel`,
    installOnQuit: $localize`:@@pd.updates.install-on-quit:Install when the app quits`,
    cancel: $localize`:@@pd.updates.cancel:Cancel`,
    discard: $localize`:@@pd.updates.discard:Discard`,
    restart: $localize`:@@pd.updates.restart:Restart to update`,
    checkAgain: $localize`:@@pd.updates.check-again:Check again`,
    checkNow: $localize`:@@pd.updates.check-now:Check now`,
    download: $localize`:@@pd.updates.download:Download`,
    tryAgain: $localize`:@@pd.updates.try-again:Try again`,
    retry: $localize`:@@pd.updates.retry:Retry`,
    asking: $localize`:@@pd.updates.asking:Asking the updater…`,
  };
  protected readonly store = inject(UpdatesStore);
  /** The window's jobs, when it has them: the download's phases. */
  private readonly jobs = inject(JobsStore, { optional: true });

  private readonly version = computed(() => this.store.info()?.version ?? '');

  protected current(version: string): string {
    return $localize`:@@pd.updates.current:You have version ${version}:version:.`;
  }

  protected readonly available = computed(() => {
    const version = this.version();
    const size = this.store.info()?.size;
    return size
      ? $localize`:@@pd.updates.available.sized:Version ${version}:version: is available (${formatBytes(size)}:size:).`
      : $localize`:@@pd.updates.available:Version ${version}:version: is available.`;
  });

  protected readonly downloading = computed(() => $localize`:@@pd.updates.downloading:Downloading version ${this.version()}:version:…`);
  protected readonly ready = computed(() => $localize`:@@pd.updates.ready:Version ${this.version()}:version: is downloaded and its signature verified.`);
  protected readonly installing = computed(() => $localize`:@@pd.updates.installing:Installing version ${this.version()}:version:…`);

  protected readonly installOnQuit = computed(() => {
    const p = this.store.phase();
    return p?.kind === 'ready' && p.installOnQuit;
  });

  protected readonly checkedText = computed(() => {
    const s = this.store.status();
    const when = s?.lastChecked ? new Date(s.lastChecked * 1000).toLocaleString() : null;
    const checked = when ? $localize`:@@pd.updates.last-checked:Last checked ${when}:when:` : $localize`:@@pd.updates.never-checked:Not checked in this installation yet`;
    return s?.endpointHost ? $localize`:@@pd.updates.asks:${checked}:checked: · asks ${s.endpointHost}:host:` : checked;
  });

  /** The download job's phase, from the window's jobs (null without them or before the first event). */
  protected readonly progress = computed(() => {
    const id = this.store.job();
    const job = id === null ? undefined : this.jobs?.jobs().find((j) => j.id === id);
    const phase: SuJobPhase | null | undefined = job?.state === 'running' ? job.phase : null;
    if (!phase) return null;
    const fraction = phase.total ? Math.min(1, phase.done / phase.total) : null;
    const amount = phase.unit === 'bytes' ? formatBytes(phase.done) : String(phase.done);
    const label =
      phase.total && phase.unit === 'bytes'
        ? $localize`:@@pd.updates.progress.of:${phase.label}:phase:: ${amount}:done: of ${formatBytes(phase.total)}:total:`
        : $localize`:@@pd.updates.progress:${phase.label}:phase:: ${amount}:done:`;
    return { label, fraction };
  });

  protected setAuto(autoCheck: boolean): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck, channel: s.channel });
  }

  protected readonly channelOptions = computed(() => (this.store.status()?.channels ?? []).map((c) => ({ value: c.id, label: c.label })));

  protected setChannel(channel: string): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck: s.autoCheck, channel });
  }
}
