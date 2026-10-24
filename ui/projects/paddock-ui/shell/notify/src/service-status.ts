import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, linkedSignal } from '@angular/core';
import { repairLabel } from './notice-actions';
import { OPEN_SETTINGS, RAISE_CAP } from './notify-config';
import { ServiceHealthStore } from './service-health.store';
import type { RaiseCap, ServiceHealth, ServiceState } from './wire.types';

let nextId = 0;

/** "the log store" becomes "Log store". */
export function shortName(label: string): string {
  const bare = label.replace(/^the /i, '');
  return bare.charAt(0).toUpperCase() + bare.slice(1);
}

/** The state of a service that is not working, in a word. */
export function stateWord(state: ServiceState): string {
  switch (state) {
    case 'quarantined':
      return $localize`:@@pd.health.state.quarantined:switched off`;
    case 'repairing':
      return $localize`:@@pd.health.state.repairing:repairing`;
    case 'degraded':
      return $localize`:@@pd.health.state.degraded:limited`;
    default:
      return $localize`:@@pd.health.state.stopped:stopped`;
  }
}

/** What the chip says: one service by name and state, or how many need attention. */
export function chipText(problems: readonly Pick<ServiceHealth, 'label' | 'state'>[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) return $localize`:@@pd.health.chip.one:${shortName(p.label)}:service:: ${stateWord(p.state)}:state:`;
  return $localize`:@@pd.health.chip.many:${problems.length}:count: services need attention`;
}

/**
 * The one place a service outage is stated (ADR 0010, Debrief ADR 0055): a status-bar chip that appears when a service is
 * not working (a count when several) and opens the plain message, the next step, the technical details and the Repair
 * button, which the shell serves (rule 19: it never depends on the broken service). Panels say "Waiting for …" and link
 * here instead of showing errors of their own. When the window could not learn the services' state at all, the chip
 * says so and offers Try again (nothing is assumed healthy).
 */
@Component({
  selector: 'pd-service-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.open]': 'open()',
    '[class.below]': 'placement() === "below"',
    '(document:keydown.escape)': 'close(true)',
    '(document:pointerdown)': 'outside($event)',
  },
  template: `
    @if (problems().length || unknown()) {
      <button type="button" class="chip" data-testid="service-health-chip" [class.repairing]="allRepairing()" [attr.aria-expanded]="open()" [attr.aria-controls]="panelId" (click)="open.set(!open())">
        <span class="mark" aria-hidden="true"></span>
        <span class="text">{{ text() }}</span>
      </button>
      @if (open()) {
        <div class="popover" role="region" [attr.aria-label]="t.popover" [id]="panelId" data-testid="service-health-details">
          @if (unknown(); as why) {
            <section class="svc">
              <strong>{{ t.unknown }}</strong>
              <span class="hint">{{ why }}</span>
              <button type="button" class="btn primary" data-testid="service-health-reload" (click)="store.load()">{{ t.tryAgain }}</button>
            </section>
          }
          @for (s of problems(); track s.service) {
            <section class="svc" [attr.data-testid]="'service-health-' + s.service" [attr.aria-label]="shortName(s.label)">
              <strong>{{ s.cause?.message ?? notWorking(s.label) }}</strong>
              @if (s.cause?.hint; as hint) {
                <span class="hint">{{ hint }}</span>
              }
              <div class="actions">
                @if (s.state === 'repairing') {
                  <span class="hint" role="status">{{ t.repairing }}</span>
                } @else if (s.repair; as action) {
                  @if (canRepair(s)) {
                    <button type="button" class="btn primary" [attr.data-testid]="'service-health-repair-' + s.service" (click)="repair(s)">{{ label(action) }}</button>
                  }
                }
                @if (s.cause?.raiseCap; as cap) {
                  @if (canRaise) {
                    <button type="button" class="btn" [attr.data-testid]="'service-health-raise-cap-' + s.service" (click)="raise(cap)">{{ t.raiseCap }}</button>
                  }
                }
              </div>
              @if (store.repairErrors()[s.service]; as why) {
                <span class="err" role="alert">{{ why }}</span>
              }
              @if (s.cause?.detail; as detail) {
                <details><summary>{{ t.technicalDetails }}</summary><code>{{ detail }}</code></details>
              }
            </section>
          }
        </div>
      }
    }
  `,
  styles: `
    :host { position: relative; display: inline-flex; min-width: 0; }
    .chip { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; min-height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-danger); border-radius: var(--pd-radius-pill); background: var(--pd-danger-tint); color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-semibold); cursor: pointer; }
    .chip.repairing { border-color: var(--pd-border-control); background: transparent; }
    .chip:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .mark { flex: none; width: 8px; height: 8px; border-radius: 1px; background: var(--pd-danger); }
    .repairing .mark { background: var(--pd-text-muted); border-radius: 50%; }
    .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .popover { position: absolute; right: 0; bottom: calc(100% + var(--pd-space-1)); z-index: var(--pd-z-overlay); display: flex; flex-direction: column; gap: var(--pd-space-3); width: min(380px, calc(100vw - 32px)); max-height: min(480px, 70vh); overflow: auto; box-sizing: border-box; padding: var(--pd-space-3); background: var(--pd-surface-3); color: var(--pd-text); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-popover); box-shadow: var(--pd-shadow-2); font-size: var(--pd-text-body); }
    :host(.below) .popover { top: calc(100% + var(--pd-space-1)); bottom: auto; }
    .svc { display: flex; flex-direction: column; align-items: flex-start; gap: var(--pd-space-1); }
    .svc + .svc { padding-top: var(--pd-space-3); border-top: 1px solid var(--pd-hairline); }
    .hint { color: var(--pd-text-muted); line-height: var(--pd-lh-body); }
    .err { color: var(--pd-danger); }
    .actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .actions:empty { display: none; }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn.primary { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    details { max-width: 100%; color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    summary { cursor: pointer; }
    code { display: block; margin-top: var(--pd-space-1); white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--pd-font-mono); user-select: text; }
  `,
})
export class PdServiceStatus {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    popover: $localize`:@@pd.health.popover:Services`,
    unknown: $localize`:@@pd.health.unknown:The state of the services is not known yet.`,
    tryAgain: $localize`:@@pd.health.try-again:Try again`,
    repairing: $localize`:@@pd.health.repairing:Repairing…`,
    raiseCap: $localize`:@@pd.health.raise-cap:Raise the cap…`,
    technicalDetails: $localize`:@@pd.health.technical-details:Technical details`,
  };
  protected readonly store = inject(ServiceHealthStore);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly raiseCap = inject(RAISE_CAP, { optional: true });
  private readonly openSettings = inject(OPEN_SETTINGS, { optional: true });
  protected readonly canRaise = !!this.raiseCap;
  /** Where the details open: above (a status bar at the bottom, the default) or below. */
  readonly placement = input<'above' | 'below'>('above');
  protected readonly panelId = `pd-service-status-${++nextId}`;
  protected readonly problems = this.store.problems;
  /** The first read failed and nothing was heard since: why. */
  protected readonly unknown = computed(() => (this.store.isReady() ? null : this.store.requestError()));
  /** Opens by itself when a "See what happened" link asks (`showDetails`). */
  /** Closes by itself when the chip goes (every service works again), so it does not reopen with the next outage. */
  protected readonly open = linkedSignal<{ asked: number; shown: boolean }, boolean>({
    source: () => ({ asked: this.store.detailsRequested(), shown: this.problems().length > 0 || this.unknown() !== null }),
    computation: (src, prev) => {
      if (!prev) return false;
      if (src.asked !== prev.source.asked) return true;
      return src.shown && prev.value;
    },
  });
  protected readonly allRepairing = computed(() => this.problems().length > 0 && this.problems().every((p) => p.state === 'repairing'));
  protected readonly text = computed(() => (this.problems().length ? chipText(this.problems()) : $localize`:@@pd.health.chip.unknown:Services: unknown`));
  protected notWorking(label: string): string {
    return $localize`:@@pd.health.not-working:${shortName(label)}:service: is not working.`;
  }
  protected readonly label = repairLabel;
  protected readonly shortName = shortName;

  protected close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    if (refocus) this.host.nativeElement.querySelector<HTMLButtonElement>('.chip')?.focus();
  }

  protected outside(e: Event): void {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node | null)) this.close(false);
  }

  protected raise(cap: RaiseCap): void {
    void this.raiseCap?.(cap);
  }

  /** The shell runs every repair but `openSettings`, which needs the app's Settings opener and a section. */
  protected canRepair(s: ServiceHealth): boolean {
    return s.repair !== 'openSettings' || (!!this.openSettings && !!s.cause?.settings);
  }

  protected repair(s: ServiceHealth): void {
    if (s.repair === 'openSettings') {
      void this.openSettings?.(s.cause?.settings ?? '');
      return;
    }
    void this.store.repair(s.service);
  }
}
