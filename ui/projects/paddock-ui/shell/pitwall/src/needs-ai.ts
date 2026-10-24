import { booleanAttribute, ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { NextStep } from '../generated/pitwall';
import { PitwallStore } from './pitwall.store';

let nextId = 0;

/** The button text of a step the component cannot run itself: the app does it (`(step)`), because only it knows where Pitwall is. */
export function stepLabel(step: NextStep): string | null {
  switch (step) {
    case 'startPitwall':
      return $localize`:@@pd.pitwall.step.start:Open Pitwall`;
    case 'installPitwall':
      return $localize`:@@pd.pitwall.step.install:Get Pitwall`;
    case 'installModel':
      return $localize`:@@pd.pitwall.step.model:Download a model in Pitwall`;
    case 'updateApp':
      return $localize`:@@pd.pitwall.step.update-app:Update this app`;
    case 'updatePitwall':
      return $localize`:@@pd.pitwall.step.update-pitwall:Update Pitwall`;
    case 'changePermissions':
      return $localize`:@@pd.pitwall.step.permissions:Change permissions in Pitwall`;
    case 'wait':
      return $localize`:@@pd.pitwall.step.retry:Try again`;
    default:
      return null;
  }
}

/**
 * The family's one "this feature needs local AI" state (ADR 0004): what a feature draws instead of itself when Pitwall is
 * missing, not running, not connected, paused or without a suitable model. It shows the engine's sentence and only the actions
 * the engine offers; it never shows an error or a dead button for a missing Pitwall. Connecting shows Pitwall's six-digit code
 * to compare; "Use the built-in engine" (the repair that does not depend on Pitwall, rule 19) appears when the app has one.
 * Steps only the app can run (open or install Pitwall, update, change permissions) come out as `(step)`; `(openSettings)`
 * opens the app's AI settings. Needs the window's `PitwallStore` (`providePitwall`).
 *
 * ```html
 * <pd-needs-ai feature="Ask about this log" settingsLink (step)="open($event)" (openSettings)="settings()" />
 * ```
 */
@Component({
  selector: 'pd-needs-ai',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `
    @if (store.needsAttention()) {
      <section class="pd-needs-ai" [attr.aria-labelledby]="titleId" data-testid="needs-ai" [attr.data-kind]="store.kind()">
        <pd-glyph class="pd-needs-ai__icon" [name]="icon()" [size]="20" />
        <div class="pd-needs-ai__body">
          <p class="pd-needs-ai__title" [id]="titleId">{{ title() }}</p>
          <p class="pd-needs-ai__message" data-testid="needs-ai-message">{{ store.message() }}</p>
          @if (store.pairingCode(); as code) {
            <p class="pd-needs-ai__code" data-testid="needs-ai-code" [attr.aria-label]="t.codeLabel">{{ code }}</p>
            <p class="pd-needs-ai__hint" data-testid="needs-ai-approve">{{ approve() }}</p>
          }
          <div class="pd-needs-ai__actions">
            @if (store.pairingCode()) {
              <button type="button" class="pd-btn" data-testid="needs-ai-cancel" [disabled]="store.busy()" (click)="store.cancelPairing()">{{ t.cancel }}</button>
            } @else {
              @for (step of steps(); track $index) {
                <button type="button" class="pd-btn" [class.pd-btn--primary]="$first" [attr.data-testid]="'needs-ai-' + step" [disabled]="store.busy()" (click)="run(step)">
                  {{ label(step) }}
                </button>
              }
            }
            @if (store.canUseBuiltIn()) {
              <button type="button" class="pd-btn" data-testid="needs-ai-builtin" [disabled]="store.busy()" (click)="store.useBuiltIn()">{{ t.useBuiltIn }}</button>
            }
            @if (settingsLink()) {
              <button type="button" class="pd-btn pd-btn--ghost" data-testid="needs-ai-settings" (click)="openSettings.emit()">{{ t.openSettings }}</button>
            }
          </div>
          @if (store.onBuiltIn()) {
            <p class="pd-needs-ai__hint" data-testid="needs-ai-on-builtin">{{ t.onBuiltIn }}</p>
          }
          @if (store.actionError(); as err) {
            <p class="pd-needs-ai__error" role="alert" data-testid="needs-ai-error">{{ err }}</p>
          }
        </div>
      </section>
    } @else if (store.requestError(); as err) {
      <section class="pd-needs-ai" [attr.aria-label]="t.stateUnknown" data-testid="needs-ai-failed">
        <pd-glyph class="pd-needs-ai__icon" name="alert" [size]="20" />
        <div class="pd-needs-ai__body">
          <p class="pd-needs-ai__message">{{ err }}</p>
          <div class="pd-needs-ai__actions">
            <button type="button" class="pd-btn" data-testid="needs-ai-retry" (click)="store.load()">{{ t.retry }}</button>
          </div>
        </div>
      </section>
    }
  `,
  styles: `
    :host { display: block; container-type: inline-size; }
    .pd-needs-ai { display: flex; gap: var(--pd-space-3); align-items: flex-start; padding: var(--pd-space-4); border: 1px solid var(--pd-border-subtle); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); color: var(--pd-text); }
    .pd-needs-ai__icon { color: var(--pd-text-muted); margin-top: 2px; }
    .pd-needs-ai[data-kind='incompatible'] .pd-needs-ai__icon, .pd-needs-ai[data-kind='unavailable'] .pd-needs-ai__icon { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-needs-ai__body { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; flex: 1; }
    p { margin: 0; overflow-wrap: anywhere; }
    .pd-needs-ai__title { font-weight: var(--pd-weight-semibold); }
    .pd-needs-ai__hint { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-needs-ai__code { font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); letter-spacing: 0.15em; font-variant-numeric: tabular-nums; }
    .pd-needs-ai__actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
    .pd-needs-ai__error { color: var(--pd-danger-text, var(--pd-danger)); }
    @container (max-width: 360px) { .pd-needs-ai { flex-direction: column; padding: var(--pd-space-3); } }
  `,
})
export class PdNeedsAi {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    cancel: $localize`:@@pd.pitwall.cancel:Cancel`,
    useBuiltIn: $localize`:@@pd.pitwall.use-builtin:Use the built-in engine`,
    openSettings: $localize`:@@pd.pitwall.open-settings:Open AI settings`,
    onBuiltIn: $localize`:@@pd.pitwall.on-builtin:This session uses the built-in engine.`,
    codeLabel: $localize`:@@pd.pitwall.code-label:Pairing code`,
    retry: $localize`:@@pd.pitwall.retry:Try again`,
    stateUnknown: $localize`:@@pd.pitwall.state-unknown:Pitwall`,
  };
  protected readonly store = inject(PitwallStore);
  protected readonly titleId = `pd-needs-ai-${++nextId}`;
  /** The feature, as the user knows it ("Ask about this log"). */
  readonly feature = input.required<string>();
  /** Offer "Open AI settings" (the app's AI page). */
  readonly settingsLink = input(false, { transform: booleanAttribute });
  /** The user chose a step only the app can run: open or install Pitwall, update, change permissions. */
  readonly step = output<NextStep>();
  /** The user chose "Open AI settings". */
  readonly openSettings = output<void>();

  protected readonly title = computed(() => $localize`:@@pd.pitwall.title:${this.feature()}:feature: needs local AI`);
  protected readonly approve = computed(() => $localize`:@@pd.pitwall.approve:Approve ${this.store.app()}:app: in Pitwall > Apps & keys and check that it shows this code.`);
  protected readonly icon = computed(() => (['incompatible', 'unavailable'].includes(this.store.kind() ?? '') ? 'alert' : 'info'));
  /** The engine's buttons, without the ones with nothing to press (`none`). */
  protected readonly steps = computed(() => this.store.actions().filter((s) => s === 'pair' || stepLabel(s) !== null));

  protected label(step: NextStep): string {
    return step === 'pair' ? $localize`:@@pd.pitwall.step.pair:Connect to Pitwall…` : (stepLabel(step) ?? '');
  }

  protected run(step: NextStep): void {
    if (step === 'pair') void this.store.connect();
    else if (step === 'wait') void this.store.load();
    else this.step.emit(step);
  }
}
