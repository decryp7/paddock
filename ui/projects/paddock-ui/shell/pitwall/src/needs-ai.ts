import { booleanAttribute, ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { PdNeedsAiCard } from '@decryp7/paddock-ui/states';
import type { NextStep } from '../generated/pitwall';
import { PitwallStore } from './pitwall.store';

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
  imports: [PdNeedsAiCard],
  template: `
    @if (store.needsAttention()) {
      <pd-needs-ai-card
        [title]="title()"
        [message]="store.message()"
        [icon]="icon()"
        [kind]="store.kind() ?? ''"
        [code]="store.pairingCode() ?? ''"
        [codeLabel]="t.codeLabel"
        [approve]="approve()"
        [hint]="store.onBuiltIn() ? t.onBuiltIn : ''"
        [error]="store.actionError() ?? ''"
      >
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
      </pd-needs-ai-card>
    } @else if (store.requestError(); as err) {
      <pd-needs-ai-card [name]="t.stateUnknown" [message]="err" icon="warning" testid="needs-ai-failed">
        <button type="button" class="pd-btn" data-testid="needs-ai-retry" (click)="store.load()">{{ t.retry }}</button>
      </pd-needs-ai-card>
    }
  `,
  styles: `:host { display: block; }`,
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
  protected readonly icon = computed(() => (['incompatible', 'unavailable'].includes(this.store.kind() ?? '') ? 'warning' : 'info'));
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
