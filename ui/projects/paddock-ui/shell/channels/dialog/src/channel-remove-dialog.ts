import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The remove confirmation (a modal decision, one window per channel): it names the channel and says its stored secret goes
 * too. Its actions are declared (ADR 0026 §2): Keep is the way out and has the focus and Enter; Remove channel is the
 * destructive answer and is never the Enter target. It calls nothing: it answers `true` for Remove channel and is
 * dismissed by Keep, and the page removes the channel. A window opened without a channel says so inline, with a Close.
 */
@Component({
  selector: 'pd-channel-remove-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <div class="pd-rm">
        <p class="pd-rm__text" data-testid="channel-remove-text">{{ t.secretGoes }}</p>
      </div>
    } @else {
      <div class="pd-rm">
        <p class="pd-rm__error" role="alert" data-testid="channel-remove-invalid">{{ t.invalid }}</p>
      </div>
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .pd-rm { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-rm__text, .pd-rm__error { margin: 0; overflow-wrap: anywhere; }
    .pd-rm__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  `,
})
export class PdChannelRemoveDialog implements DialogContent<boolean> {

  /** The channel to remove. */
  readonly channelId = input('');
  /** Its name, for the question. */
  readonly name = input('');

  protected readonly t = {
    remove: $localize`:@@pd.channels.remove-yes:Remove channel`,
    keep: $localize`:@@pd.channels.remove-keep:Keep`,
    close: $localize`:@@pd.channels.remove-close:Close`,
    secretGoes: $localize`:@@pd.channels.remove-secret-goes:Its stored secret is deleted too.`,
    invalid: $localize`:@@pd.channels.remove-invalid:This window was opened without a channel to remove. Close it and choose Remove on a channel in the list.`,
  };

  protected valid(): boolean {
    return this.channelId().trim() !== '';
  }

  /** Keep and Remove channel; a window without a channel offers only Close. */
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() =>
    this.valid()
      ? [
          { kind: 'dismiss', id: 'keep', label: this.t.keep },
          { kind: 'answer', id: 'remove', label: this.t.remove, role: 'destructive', answer: () => true },
        ]
      : [{ kind: 'dismiss', id: 'close', label: this.t.close }],
  );

  /** The question, as the decision's heading (ADR 0026: icon, heading, message). */
  readonly dialogHeading = computed(() => (this.valid() ? this.question() : null));

  protected question(): string {
    const name = this.name().trim() || this.channelId();
    return $localize`:@@pd.channels.remove-question:Remove the channel “${name}:name:”?`;
  }
}
