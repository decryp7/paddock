import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, untracked } from '@angular/core';
import { SU_STATUS_ICON, type SuStatus } from '@decryp7/paddock-ui';
import { repairButtonLabel } from './notice-actions';
import { OPEN_SETTINGS } from './notify-config';
import { ServiceHealthStore } from './service-health.store';
import { shortName, stateWord } from './service-status-text';

/** How a panel's service stands, for what the panel draws instead of its data. */
export type WaitingKind = 'waiting' | 'repairing' | 'offline';

/** Unknown, starting or repairing: something is under way (a spinner). Down, limited or switched off: nothing will
 * happen until someone acts (an offline state with the repair, never a spinner). */
export function waitingKind(state: string | null): WaitingKind {
  if (state === 'repairing') return 'repairing';
  return state === 'down' || state === 'degraded' || state === 'quarantined' ? 'offline' : 'waiting';
}

/**
 * The one severity of a service's state wherever it is shown (rule 8: icon, word and colour as one unit): the status
 * chip, a panel's offline state, a tray's pill and banner. Not working is `danger`; under way is `neutral`.
 */
export function serviceStatus(state: string | null): SuStatus {
  if (state === 'down' || state === 'degraded' || state === 'quarantined') return 'danger';
  return state === 'ok' ? 'success' : 'neutral';
}

/** "the demo engine" restarts, is reset, is installed: what brings it back, by its repair. */
function untilPhrase(service: string, repair: string | null): string {
  switch (repair) {
    case 'restart':
      return $localize`:@@pd.health.until.restart:until ${service}:service: restarts`;
    case 'reset':
      return $localize`:@@pd.health.until.reset:until ${service}:service: is reset`;
    case 'install':
      return $localize`:@@pd.health.until.install:until ${service}:service: is installed`;
    default:
      return $localize`:@@pd.health.until.works:until ${service}:service: works again`;
  }
}

/**
 * The offline state's line: what is missing and what brings it back ("Engine data is unavailable until the demo engine
 * restarts."). Without the panel's `subject`, the service itself ("Demo engine is unavailable until it restarts.").
 */
export function offlineLine(subject: string | null, label: string, repair: string | null): string {
  const what = subject ?? shortName(label);
  const until = untilPhrase(subject ? label : $localize`:@@pd.health.it:it`, repair);
  return $localize`:@@pd.health.offline:${what}:subject: is unavailable ${until}:until:.`;
}

/**
 * What a panel shows instead of its own error while a service it needs is not working. While something is under way
 * (the first snapshot, a start, a repair) it waits with a spinner. When the service stopped it draws the offline state:
 * the warning icon and word, one muted line, and the same repair the status chip offers (rule 19), plus a link to the
 * chip's details. The panel never retries on its own; it loads again when the service is back.
 */
@Component({
  selector: 'pd-waiting-for',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="pd-waiting" role="status" [attr.data-testid]="'waiting-for-' + service()" [attr.data-kind]="kind()">
      @if (kind() === 'offline') {
        <svg class="pd-icon" [attr.data-status]="status()" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path [attr.d]="icon()" /></svg>
        <strong>{{ stopped() }}</strong>
        <span class="pd-line">{{ line() }}</span>
        @if (repairText(); as text) {
          <button type="button" class="pd-btn" data-testid="waiting-for-repair" (click)="repair()">{{ text }}</button>
        }
      } @else {
        <span class="pd-spinner" aria-hidden="true"></span>
        <strong>{{ kind() === 'repairing' ? repairing() : waitingFor() }}</strong>
      }
      <button type="button" class="pd-link" data-testid="waiting-for-details" (click)="health.showDetails()">{{ t.seeWhatHappened }}</button>
    </div>
  `,
  styles: `
    :host { display: block; }
    .pd-waiting { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); text-align: center; color: var(--pd-text-muted); padding: var(--pd-space-4); }
    strong { color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .pd-line { max-width: 48ch; }
    .pd-icon { color: var(--pd-danger); }
    .pd-icon path { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; }
    .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-link { border: 0; padding: 0; background: none; font: inherit; cursor: pointer; color: var(--pd-accent); text-decoration: underline; font-size: var(--pd-text-caption); }
    .pd-link:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-spinner { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--pd-hairline); border-top-color: var(--pd-accent); animation: pd-waiting-spin 0.9s linear infinite; }
    @keyframes pd-waiting-spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .pd-spinner { animation: none; } }
  `,
})
export class PdWaitingFor {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    seeWhatHappened: $localize`:@@pd.health.see-what-happened:See what happened`,
  };
  protected readonly health = inject(ServiceHealthStore);
  private readonly openSettings = inject(OPEN_SETTINGS, { optional: true });
  /** The service id. */
  readonly service = input.required<string>();
  /** What the panel shows when the service works ("Engine data"), for the offline line; the service's name without it. */
  readonly subject = input<string | null>(null);
  private readonly state = computed(() => this.health.health(this.service()));
  protected readonly kind = computed(() => waitingKind(this.state()?.state ?? null));
  protected readonly label = computed(() => this.state()?.label ?? this.service());
  protected readonly waitingFor = computed(() => $localize`:@@pd.health.waiting-for:Waiting for ${this.label()}:service:`);
  protected readonly repairing = computed(() => $localize`:@@pd.health.repairing-for:Repairing ${this.label()}:service:…`);
  protected readonly status = computed(() => serviceStatus(this.state()?.state ?? null));
  protected readonly icon = computed(() => SU_STATUS_ICON[this.status()]);
  protected readonly line = computed(() => offlineLine(this.subject(), this.label(), this.state()?.repair ?? null));
  protected readonly stopped = computed(() => `${shortName(this.label())} ${stateWord(this.state()?.state ?? 'down', this.state()?.cause)}`);
  /** The chip's repair, by the same words; none when nothing repairs it here (the details say what to do). */
  protected readonly repairText = computed(() => {
    const s = this.state();
    if (!s?.repair) return null;
    if (s.repair === 'openSettings' && (!this.openSettings || !s.cause?.settings)) return null;
    return repairButtonLabel(s.repair, s.label);
  });

  protected repair(): void {
    const s = this.state();
    if (!s?.repair) return;
    if (s.repair === 'openSettings') void this.openSettings?.(s.cause?.settings ?? '');
    else void this.health.repair(s.service);
  }
}

/**
 * An inline error as people read it: what did not happen ("Couldn't load events."), Retry, and the backend's message
 * folded under "Technical details". While the panel's `service` is not working it shows `pd-waiting-for` instead (one
 * calm state in every panel; the chip says why), and when the service comes back it emits `retry` by itself, once.
 */
@Component({
  selector: 'pd-service-error',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWaitingFor],
  template: `
    @if (waiting()) {
      <pd-waiting-for [service]="service() ?? ''" [subject]="subject()" />
    } @else {
      <div class="pd-error" role="alert">
        <strong class="pd-summary">{{ summary() }}</strong>
        @if (retryable()) {
          <button class="pd-btn" type="button" (click)="retry.emit()">{{ t.retry }}</button>
        }
        @if (detail(); as d) {
          <details class="pd-tech">
            <summary>{{ t.technicalDetails }}</summary>
            <code>{{ d }}</code>
          </details>
        }
      </div>
    }
  `,
  styles: `
    :host { display: block; }
    .pd-error { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); text-align: center; padding: var(--pd-space-4); }
    .pd-summary { color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn:focus-visible, summary:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-tech { max-width: 100%; color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .pd-tech summary { cursor: pointer; }
    .pd-tech code { display: block; margin-top: var(--pd-space-1); white-space: pre-wrap; overflow-wrap: anywhere; text-align: start; font-family: var(--pd-font-mono); -webkit-user-select: text; user-select: text; }
  `,
})
export class PdServiceError {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    retry: $localize`:@@pd.health.retry:Retry`,
    technicalDetails: $localize`:@@pd.health.technical-details:Technical details`,
  };
  private readonly health = inject(ServiceHealthStore);
  /** What did not happen, as a sentence. */
  readonly summary = input.required<string>();
  /** The backend's message (technical); folded away. */
  readonly detail = input<string | null>(null);
  readonly retryable = input(true);
  /** What the panel shows when it works ("Engine data"), for the offline line while its service is stopped. */
  readonly subject = input<string | null>(null);
  /** The service the panel's data comes from; null when it needs none (never waits). */
  readonly service = input<string | null>(null);
  readonly retry = output<void>();
  /** That service is not working (or not known yet): the panel waits instead of showing an error of its own. */
  protected readonly waiting = computed(() => {
    const s = this.service();
    return s !== null && !this.health.isServing(s);
  });

  constructor() {
    // Loads again by itself when the service comes back after this panel waited for it (an output is outside the signal
    // graph). Waiting for the first snapshot of a new window does not count: nothing was lost then.
    let waited = false;
    effect(() => {
      const heard = this.health.isReady();
      const now = this.waiting();
      if (waited && !now) untracked(() => this.retry.emit());
      waited = heard && now;
    });
  }
}
