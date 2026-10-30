import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { MatCheckbox } from '@angular/material/checkbox';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** The answer of the quit question: whether to stop serving, and whether to keep the choice as the "When you quit" setting. */
export interface QuitAnswer {
  readonly stopServing: boolean;
  readonly remember: boolean;
}

/**
 * `pw.quit`: "Keep serving after you quit?" (its window is titled "Quit Pitwall", so the heading asks the question rather than
 * repeating the title), asked when the setting "When you quit Pitwall" is "Ask each time". The footer is the frame's:
 * Cancel, "Quit and stop serving" and "Quit, keep serving" (the safe default, the Enter target); "Don't ask again" in the body
 * keeps the choice as the setting. It calls nothing: the window that opened it quits (rule 17, ADR 0026 §2).
 */
@Component({
  selector: 'pw-quit-dialog',
  imports: [MatCheckbox],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="msg" data-testid="quit-message">
      Apps on this computer can keep using your models while Pitwall's server runs in the background. Stopping unloads every model and
      ends the engine processes; Pitwall serves again when you open it.
    </p>
    <mat-checkbox class="remember" data-testid="quit-remember" [checked]="remember()" (change)="remember.set($event.checked)">Don't ask again</mat-checkbox>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    .msg { margin: 0; color: var(--pd-text-muted); overflow-wrap: anywhere; }
    .remember { margin-inline-start: calc(-1 * var(--pd-space-2)); }
  `,
})
export class QuitDialog implements DialogContent<QuitAnswer> {
  protected readonly remember = signal(false);

  readonly dialogHeading = signal<string | null>('Keep serving after you quit?');

  readonly dialogActions = computed<readonly DialogAction<QuitAnswer>[]>(() => {
    const remember = this.remember();
    return [
      { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
      { kind: 'answer', id: 'stop', label: 'Quit and stop serving', role: 'secondary', answer: () => ({ stopServing: true, remember }) },
      { kind: 'answer', id: 'keep', label: 'Quit, keep serving', role: 'primary', answer: () => ({ stopServing: false, remember }) },
    ];
  });
}
