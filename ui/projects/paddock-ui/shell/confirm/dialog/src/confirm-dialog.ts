import { ChangeDetectionStrategy, Component, computed, effect, inject, input } from '@angular/core';
import { DIALOG_REF, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * `pd.confirm` (ADR 0026 §5): the question as the title, the message and any details, and the frame's footer from two
 * declared actions: the way out (Cancel, or the opener's label) and the affirmative, a primary or, for `destructive`, the
 * danger action that is never the Enter target and leaves the first focus on the way out. A window opened without a
 * question says so, with Close only.
 */
@Component({
  selector: 'pd-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <p class="pd-confirm__text" data-testid="confirm-message">{{ message() }}</p>
      @if (details().length) {
        <ul class="pd-confirm__details" data-testid="confirm-details">
          @for (d of details(); track $index) {
            <li>{{ d }}</li>
          }
        </ul>
      }
    } @else {
      <p class="pd-confirm__error" role="alert" data-testid="confirm-invalid">{{ t.invalid }}</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); color: var(--pd-text); min-width: 0; }
    .pd-confirm__text, .pd-confirm__error { margin: 0; overflow-wrap: anywhere; }
    .pd-confirm__details { margin: 0; padding-inline-start: var(--pd-space-4); color: var(--pd-text-muted); overflow-wrap: anywhere; }
    .pd-confirm__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  `,
})
export class PdConfirmDialog implements DialogContent<true> {
  private readonly ref = inject(DIALOG_REF);
  readonly heading = input('');
  readonly message = input('');
  readonly details = input<readonly string[]>([]);
  readonly confirm = input<{ readonly label: string; readonly destructive?: boolean } | null>(null);
  readonly cancelLabel = input<string | null>(null);

  protected readonly t = {
    cancel: $localize`:@@pd.confirm.cancel:Cancel`,
    close: $localize`:@@pd.confirm.close:Close`,
    invalid: $localize`:@@pd.confirm.invalid:This window was opened without a question. Close it and try again from where you started.`,
  };

  protected readonly valid = computed(() => this.heading().trim() !== '' && (this.confirm()?.label ?? '').trim() !== '');

  /** The way out and the affirmative; without a question, Close only. */
  readonly dialogActions = computed<readonly DialogAction<true>[]>(() => {
    const c = this.confirm();
    if (!this.valid() || !c) return [{ kind: 'dismiss', id: 'close', label: this.t.close }];
    return [
      { kind: 'dismiss', id: 'cancel', label: this.cancelLabel()?.trim() || this.t.cancel },
      { kind: 'answer', id: 'confirm', label: c.label, role: c.destructive ? 'destructive' : 'primary', answer: () => true as const },
    ];
  });

  constructor() {
    // The heading is the window's title; it is known only once the opener's inputs are set.
    effect(() => {
      if (this.valid()) this.ref.setTitle?.(this.heading().trim());
    });
  }
}
