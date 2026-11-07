import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatCheckbox, type MatCheckboxChange } from '@angular/material/checkbox';
import { MatProgressBar } from '@angular/material/progress-bar';
import { PdSelect } from '@decryp7/paddock-ui/select';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { SuJobPhase } from '@decryp7/paddock-ui';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { UpdatesBackend } from './backend';
import { UpdatesStore } from './updates.store';
import { toUpdateError } from './wire';
import type { UpdateErrorDto } from '../generated/about';
import { ABOUT_STYLES } from './styles';
import { formatStorage } from './format';

/**
 * Updates (a form: modeless, medium, fitted to its content, kept open beside the work). Every step is the engine's: Check, Download (a job: the download
 * and the signature check, its progress in the Jobs list and here when this window has jobs), Restart to update or
 * install when the app quits, and the preferences. A refused step shows the engine's message and next step. A build
 * without an update key says "Updates are not available in this build" and offers nothing to press.
 */
@Component({
  selector: 'pd-updates-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatCheckbox, MatProgressBar, PdSelect],
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
            @if (!store.canInstall()) {
              <p class="pd-muted" data-testid="updates-manual">{{ t.manual }}</p>
            }
          }
          @case ('downloading') {
            <p role="status">{{ downloading() }}</p>
            @if (progress(); as p) {
              <p class="pd-muted">{{ p.label }}</p>
              <mat-progress-bar [mode]="p.fraction === null ? 'indeterminate' : 'determinate'" [value]="(p.fraction ?? 0) * 100" [aria-label]="p.label" />
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
          @case ('installerRunning') {
            <p role="status" data-testid="updates-installer-running">{{ t.installerRunning }}</p>
          }
          @case ('failed') {
            <p>{{ t.failed }}</p>
          }
        }
      </div>
      @if (s.install.administratorNotice; as notice) {
        @if (s.phase.kind === 'available' || s.phase.kind === 'ready') {
          <p class="pd-muted" data-testid="updates-administrator">{{ notice }}</p>
        }
      }
      @if (store.info()?.notes; as notes) {
        <pre class="pd-text" tabindex="0" [attr.aria-label]="t.notes">{{ notes }}</pre>
      }
      @if (openFailed(); as why) {
        <div class="pd-problem" role="alert" data-testid="updates-open-failed">
          <p class="pd-message">{{ why.message }}</p>
          <p>{{ why.nextStep }}</p>
        </div>
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
        <div class="pd-prefs" data-testid="updates-prefs">
          <mat-checkbox data-testid="updates-auto" [checked]="s.autoCheck" (change)="setAuto($event)">{{ t.autoCheck }}</mat-checkbox>
          @if (s.channels.length > 1) {
            <!-- eslint-disable-next-line @angular-eslint/template/label-has-associated-control -- the label wraps a pd-select, whose trigger button is the labelled control (ariaLabel is set) -->
            <label class="pd-field" data-testid="updates-channel">
              <span class="pd-label">{{ t.channel }}</span>
              <pd-select [options]="channelOptions()" [value]="s.channel" [ariaLabel]="t.channel" (valueChange)="setChannel($event)" />
            </label>
          }
          @if (s.phase.kind === 'ready' && store.canInstall() && s.install.installOnQuitAvailable) {
            <mat-checkbox [checked]="installOnQuit()" (change)="store.installOnQuit($event.checked)">{{ t.installOnQuit }}</mat-checkbox>
          }
        </div>
        @if (checkedText(); as checked) {
          <p class="pd-muted" data-testid="updates-checked">{{ checked }}</p>
        }
      }
    } @else if (store.requestError(); as error) {
      <p role="alert">{{ error }} <button type="button" class="pd-btn" (click)="store.load()">{{ t.retry }}</button></p>
    } @else {
      <p class="pd-muted" role="status">{{ t.asking }}</p>
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdUpdatesDialog implements DialogContent {
  /** The component's texts in the window's language (plan S15). The engine's errors and next steps arrive as they are. */
  protected readonly t = {
    unavailable: $localize`:@@pd.updates.unavailable:Updates are not available in this build.`,
    noKey: $localize`:@@pd.updates.unavailable.why:This build was made without an update signing key, so it cannot verify updates. Download new versions from the releases page.`,
    idle: $localize`:@@pd.updates.idle:Not checked yet.`,
    checking: $localize`:@@pd.updates.checking:Checking for updates…`,
    upToDate: $localize`:@@pd.updates.up-to-date:You have the latest version.`,
    progressInJobs: $localize`:@@pd.updates.progress-in-jobs:The progress is in Jobs.`,
    failed: $localize`:@@pd.updates.failed:The last step did not finish.`,
    installerRunning: $localize`:@@pd.updates.installer-running:An update is being installed. This window updates when the installer is done.`,
    notes: $localize`:@@pd.updates.notes:Release notes`,
    autoCheck: $localize`:@@pd.updates.auto-check:Check for updates automatically`,
    channel: $localize`:@@pd.updates.channel:Channel`,
    manual: $localize`:@@pd.updates.manual:This build does not install updates itself. Download the new version from the releases page.`,
    installOnQuit: $localize`:@@pd.updates.install-on-quit:Install when the app quits`,
    stopChecking: $localize`:@@pd.updates.stop-checking:Stop checking`,
    stopDownload: $localize`:@@pd.updates.stop-download:Stop download`,
    discard: $localize`:@@pd.updates.discard:Discard`,
    restart: $localize`:@@pd.updates.restart:Restart to update`,
    openReleases: $localize`:@@pd.updates.open-releases:Open releases page`,
    install: $localize`:@@pd.updates.install:Install`,
    installAgain: $localize`:@@pd.updates.install-again:Install again`,
    checkAgain: $localize`:@@pd.updates.check-again:Check again`,
    checkNow: $localize`:@@pd.updates.check-now:Check now`,
    download: $localize`:@@pd.updates.download:Download`,
    tryAgain: $localize`:@@pd.updates.try-again:Try again`,
    retry: $localize`:@@pd.updates.retry:Retry`,
    asking: $localize`:@@pd.updates.asking:Asking the updater…`,
  };
  protected readonly store = inject(UpdatesStore);
  private readonly backend = inject(UpdatesBackend);
  /** Why the releases page did not open, in the engine's words; null when it did. */
  protected readonly openFailed = signal<UpdateErrorDto | null>(null);
  /** The window's jobs, when it has them: the download's phases. */
  private readonly jobs = inject(JobsStore, { optional: true });

  private readonly version = computed(() => this.store.info()?.version ?? '');

  /**
   * The footer by phase (ADR 0026 §2): what moves the update on is the primary, the rest secondary; the frame adds Close.
   * They start or stop the updater's jobs and return at once.
   */
  readonly dialogActions = computed<readonly DialogAction[]>(() => {
    const s = this.store.status();
    if (!s) return [];
    const run = (id: string, label: string, role: 'primary' | 'secondary', go: () => unknown): DialogAction => ({ kind: 'run', id, label, role, run: () => void go() });
    // A build that cannot install (no key, or declared `no_install`) points to the releases page: that is the primary action.
    const releases = s.releasesPage ? run('releases', this.t.openReleases, 'primary', () => this.openReleases()) : null;
    const viaReleases = (...rest: DialogAction[]): readonly DialogAction[] => (releases ? [releases, ...rest] : rest.map((a) => ({ ...a, role: 'primary' as const })));
    switch (s.phase.kind) {
      case 'unavailable':
        return releases ? [releases] : [];
      case 'checking':
        return [run('stop', this.t.stopChecking, 'secondary', () => this.store.cancel())];
      case 'downloading':
        return [run('stop', this.t.stopDownload, 'secondary', () => this.store.cancel())];
      case 'ready':
        // Only a build that can install gets here; one that cannot is not offered the install, whatever the phase.
        if (!this.store.canInstall()) return [run('discard', this.t.discard, 'secondary', () => this.store.cancel())];
        return [run('discard', this.t.discard, 'secondary', () => this.store.cancel()), run('restart', s.install.scope === 'perMachine' ? this.t.install : this.t.restart, 'primary', () => this.store.restart())];
      case 'available':
        if (!this.store.canInstall()) return viaReleases(run('check', this.t.checkAgain, 'secondary', () => this.store.check()));
        return [run('check', this.t.checkAgain, 'secondary', () => this.store.check()), run('download', this.t.download, 'primary', () => this.store.download())];
      case 'failed':
        if (!this.store.canInstall()) return viaReleases(run('check', this.t.checkAgain, 'secondary', () => this.store.check()));
        if (s.phase.error.kind === 'notInstalled' || s.phase.error.kind === 'declined') {
          return [run('install-again', this.t.installAgain, 'primary', () => this.store.installAgain())];
        }
        return this.store.canDownload() ? [run('download', this.t.tryAgain, 'primary', () => this.store.download())] : [run('check', this.t.checkAgain, 'primary', () => this.store.check())];
      case 'idle':
      case 'upToDate':
        return [run('check', this.t.checkNow, 'primary', () => this.store.check())];
      default:
        return [];
    }
  });

  /** A webview window opens no links itself: the engine opens the page in the system browser (`updates_open_releases`: the engine knows the address). */
  private async openReleases(): Promise<void> {
    this.openFailed.set(null);
    try {
      await this.backend.openReleases();
    } catch (e) {
      this.openFailed.set(toUpdateError(e));
    }
  }

  protected current(version: string): string {
    return $localize`:@@pd.updates.current:You have version ${version}:version:.`;
  }

  protected readonly available = computed(() => {
    const version = this.version();
    const size = this.store.info()?.size;
    return size
      ? $localize`:@@pd.updates.available.sized:Version ${version}:version: is available (${formatStorage(size)}:size:).`
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
    // "Not checked yet" is the phase line's own sentence; saying it twice was the duplicate.
    const checked = when ? $localize`:@@pd.updates.last-checked:Last checked ${when}:when:` : null;
    const host = s?.endpointHost;
    if (!host) return checked;
    return checked ? $localize`:@@pd.updates.asks:${checked}:checked: · asks ${host}:host:` : $localize`:@@pd.updates.asks-only:Asks ${host}:host:`;
  });

  /** The download job's phase, from the window's jobs (null without them or before the first event). */
  protected readonly progress = computed(() => {
    const id = this.store.job();
    const job = id === null ? undefined : this.jobs?.jobs().find((j) => j.id === id);
    const phase: SuJobPhase | null | undefined = job?.state === 'running' ? job.phase : null;
    if (!phase) return null;
    const fraction = phase.total ? Math.min(1, phase.done / phase.total) : null;
    const amount = phase.unit === 'bytes' ? formatStorage(phase.done) : String(phase.done);
    const label =
      phase.total && phase.unit === 'bytes'
        ? $localize`:@@pd.updates.progress.of:${phase.label}:phase:: ${amount}:done: of ${formatStorage(phase.total)}:total:`
        : $localize`:@@pd.updates.progress:${phase.label}:phase:: ${amount}:done:`;
    return { label, fraction };
  });

  protected setAuto(change: MatCheckboxChange): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck: change.checked, channel: s.channel });
  }

  protected readonly channelOptions = computed(() => (this.store.status()?.channels ?? []).map((c) => ({ value: c.id, label: c.label })));

  protected setChannel(channel: string): void {
    const s = this.store.status();
    if (s) void this.store.setPreferences({ autoCheck: s.autoCheck, channel });
  }
}
