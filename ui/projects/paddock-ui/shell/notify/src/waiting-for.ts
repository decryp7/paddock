import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, untracked } from '@angular/core';
import { ServiceHealthStore } from './service-health.store';

/**
 * What a panel shows instead of its own error while a service it needs is not working: calm, neutral, no red and no
 * Retry of its own. The status chip says what happened and repairs it; this links to it.
 */
@Component({
  selector: 'pd-waiting-for',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="waiting" role="status" [attr.data-testid]="'waiting-for-' + service()">
      <span class="spinner" aria-hidden="true"></span>
      <strong>Waiting for {{ label() }}</strong>
      <button type="button" class="link" data-testid="waiting-for-details" (click)="health.showDetails()">See what happened</button>
    </div>
  `,
  styles: `
    :host { display: block; }
    .waiting { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); text-align: center; color: var(--pd-text-muted); padding: var(--pd-space-4); }
    strong { color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .link { border: 0; padding: 0; background: none; font: inherit; cursor: pointer; color: var(--pd-accent); text-decoration: underline; font-size: var(--pd-text-caption); }
    .link:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .spinner { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--pd-hairline); border-top-color: var(--pd-accent); animation: pd-waiting-spin 0.9s linear infinite; }
    @keyframes pd-waiting-spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
  `,
})
export class PdWaitingFor {
  protected readonly health = inject(ServiceHealthStore);
  /** The service id. */
  readonly service = input.required<string>();
  protected readonly label = computed(() => this.health.health(this.service())?.label ?? this.service());
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
      <pd-waiting-for [service]="service() ?? ''" />
    } @else {
      <div class="error" role="alert">
        <strong class="summary">{{ summary() }}</strong>
        @if (retryable()) {
          <button class="btn" type="button" (click)="retry.emit()">Retry</button>
        }
        @if (detail(); as d) {
          <details class="tech">
            <summary>Technical details</summary>
            <code>{{ d }}</code>
          </details>
        }
      </div>
    }
  `,
  styles: `
    :host { display: block; }
    .error { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); text-align: center; padding: var(--pd-space-4); }
    .summary { color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn:focus-visible, summary:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .tech { max-width: 100%; color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .tech summary { cursor: pointer; }
    .tech code { display: block; margin-top: var(--pd-space-1); white-space: pre-wrap; overflow-wrap: anywhere; text-align: start; font-family: var(--pd-font-mono); user-select: text; }
  `,
})
export class PdServiceError {
  private readonly health = inject(ServiceHealthStore);
  /** What did not happen, as a sentence. */
  readonly summary = input.required<string>();
  /** The backend's message (technical); folded away. */
  readonly detail = input<string | null>(null);
  readonly retryable = input(true);
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
