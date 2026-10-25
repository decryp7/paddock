import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, linkedSignal, signal } from '@angular/core';
import { STATUS_COMPACT } from '@decryp7/paddock-ui/shell';
import { repairButtonLabel } from './notice-actions';
import { OPEN_SETTINGS, RAISE_CAP } from './notify-config';
import { ServiceHealthStore } from './service-health.store';
import { chipCompactText, chipText, shortName } from './service-status-text';
import type { RaiseCap, ServiceHealth } from './wire.types';

let nextId = 0;

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
    '[class.pd-open]': 'open()',
    '[class.pd-below]': 'placement() === "below"',
    '(document:keydown.escape)': 'close(true)',
    '(document:pointerdown)': 'outside($event)',
  },
  template: `
    @if (problems().length || unknown()) {
      <button type="button" class="pd-chip" data-testid="service-health-chip" [class.pd-repairing]="allRepairing()" [class.pd-unknown]="!problems().length" [attr.aria-expanded]="open()" [attr.aria-controls]="panelId" [attr.aria-label]="compact() ? text() : null" [attr.title]="text()" (click)="open.set(!open())">
        <span class="pd-mark" aria-hidden="true"></span>
        <span class="pd-text">{{ compact() ? compactText() : text() }}</span>
      </button>
      @if (open()) {
        <div class="pd-popover" role="region" [attr.aria-label]="t.popover" [id]="panelId" data-testid="service-health-details">
          @if (unknown(); as why) {
            <section class="pd-svc">
              <strong>{{ t.unknown }}</strong>
              <span class="pd-hint">{{ why }}</span>
              <button type="button" class="pd-btn pd-primary" data-testid="service-health-reload" (click)="store.load()">{{ t.tryAgain }}</button>
            </section>
          }
          @for (s of problems(); track s.service) {
            <section class="pd-svc" [attr.data-testid]="'service-health-' + s.service" [attr.aria-label]="shortName(s.label)">
              <strong>{{ s.cause?.message ?? notWorking(s.label) }}</strong>
              @if (s.cause?.hint; as hint) {
                <span class="pd-hint">{{ hint }}</span>
              }
              <div class="pd-actions">
                @if (s.state === 'repairing') {
                  <span class="pd-hint" role="status">{{ t.repairing }}</span>
                } @else if (s.repair; as action) {
                  @if (canRepair(s)) {
                    <button type="button" class="pd-btn pd-primary" [attr.data-testid]="'service-health-repair-' + s.service" (click)="repair(s)">{{ label(action, s.label) }}</button>
                  }
                }
                @if (s.cause?.raiseCap; as cap) {
                  @if (canRaise) {
                    <button type="button" class="pd-btn" [attr.data-testid]="'service-health-raise-cap-' + s.service" (click)="raise(cap)">{{ t.raiseCap }}</button>
                  }
                }
              </div>
              @if (store.repairErrors()[s.service]; as why) {
                <span class="pd-err" role="alert">{{ why }}</span>
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
    /* The chip's edge is visible, the other items' are not: 5 px of margin gives it the same 14 px to its neighbours'
       text as they have between them (4 px padding + 1 px border each side, 4 px gap; design review R3-2). */
    :host:has(.pd-chip) { margin-inline: 5px; }
    .pd-chip { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; min-height: 20px; padding: 0 var(--pd-space-2); border: 0; border-radius: var(--pd-radius-pill); background: var(--pd-danger-tint); color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-chip:hover { background: color-mix(in srgb, var(--pd-danger-tint), var(--pd-text) 6%); }
    /* Pressed while its details are open: the link between the chip and its popover. */
    .pd-chip[aria-expanded='true'] { background: color-mix(in srgb, var(--pd-danger-tint), var(--pd-text) 10%); box-shadow: inset 0 0 0 1px var(--pd-danger); }
    .pd-chip.pd-repairing, .pd-chip.pd-unknown { background: var(--pd-surface-2); }
    /* Not knowing is not failing: the unknown state is calm (a neutral mark and the words), never the danger colour. */
    .pd-chip.pd-unknown .pd-mark { background: var(--pd-text-muted); }
    .pd-chip:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-mark { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--pd-danger); }
    .pd-repairing .pd-mark { background: var(--pd-text-muted); border-radius: 50%; }
    .pd-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-popover { position: absolute; right: 0; bottom: calc(100% + var(--pd-space-1)); z-index: var(--pd-z-overlay); display: flex; flex-direction: column; gap: var(--pd-space-3); width: min(380px, calc(100vw - 32px)); max-height: min(480px, 70vh); overflow: auto; box-sizing: border-box; padding: var(--pd-space-3); background: var(--pd-surface-3); color: var(--pd-text); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-popover); box-shadow: var(--pd-shadow-2); font-size: var(--pd-text-body); }
    :host(.pd-below) .pd-popover { top: calc(100% + var(--pd-space-1)); bottom: auto; }
    .pd-svc { display: flex; flex-direction: column; align-items: flex-start; gap: var(--pd-space-1); }
    .pd-svc + .pd-svc { padding-top: var(--pd-space-3); border-top: 1px solid var(--pd-hairline); }
    .pd-hint { color: var(--pd-text-muted); line-height: var(--pd-lh-body); }
    .pd-err { color: var(--pd-danger); }
    .pd-actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .pd-actions:empty { display: none; }
    .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn.pd-primary { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .pd-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    details { max-width: 100%; color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    summary { cursor: pointer; }
    code { display: block; margin-top: var(--pd-space-1); white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--pd-font-mono); -webkit-user-select: text; user-select: text; }
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
  /** The status bar is narrow: the chip shows its compact form. */
  protected readonly compact = inject(STATUS_COMPACT, { optional: true }) ?? signal(false);
  protected readonly compactText = computed(() => chipCompactText(this.problems()));
  protected readonly label = repairButtonLabel;
  protected readonly shortName = shortName;

  protected close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    if (refocus) this.host.nativeElement.querySelector<HTMLButtonElement>('.pd-chip')?.focus();
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
