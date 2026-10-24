import { afterNextRender, ChangeDetectionStrategy, Component, effect, ElementRef, inject, Injector, input, viewChild } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The remove confirmation (a modal decision, one window per channel): it names the channel and says its stored secret goes
 * too. Keep is the default (Enter, Escape, focus on opening); Remove is the danger action and is never the default. It calls
 * nothing: it resolves `true` for Remove and `false` for Keep, and the page removes the channel. A window opened without a
 * channel says so inline, with a Close button, instead of showing a question about nothing.
 */
@Component({
  selector: 'pd-channel-remove-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <div class="pd-rm">
        <p class="pd-rm__text" data-testid="channel-remove-text">{{ question() }} {{ t.secretGoes }}</p>
        <div class="pd-rm__actions">
          <button type="button" class="pd-rm__btn pd-rm__btn--danger" data-testid="channel-remove-yes" (click)="ref.close(true)">{{ t.remove }}</button>
          <button #keep type="button" class="pd-rm__btn" data-default data-dialog-close data-testid="channel-remove-keep" (click)="ref.close(false)">{{ t.keep }}</button>
        </div>
      </div>
    } @else {
      <div class="pd-rm">
        <p class="pd-rm__error" role="alert" data-testid="channel-remove-invalid">{{ t.invalid }}</p>
        <div class="pd-rm__actions">
          <button type="button" class="pd-rm__btn" data-dialog-close data-testid="channel-remove-close" (click)="ref.close()">{{ t.close }}</button>
        </div>
      </div>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-rm { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-rm__text, .pd-rm__error { margin: 0; overflow-wrap: anywhere; }
    .pd-rm__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
    .pd-rm__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-rm__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-rm__btn--danger { background: var(--pd-danger-fill); border-color: var(--pd-danger-fill); color: var(--pd-on-danger); }
    .pd-rm__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdChannelRemoveDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
  private readonly injector = inject(Injector);
  private readonly keep = viewChild<ElementRef<HTMLButtonElement>>('keep');

  /** The channel to remove. */
  readonly channelId = input('');
  /** Its name, for the question. */
  readonly name = input('');

  protected readonly t = {
    remove: $localize`:@@pd.channels.remove-yes:Remove`,
    keep: $localize`:@@pd.channels.remove-keep:Keep`,
    close: $localize`:@@pd.channels.remove-close:Close`,
    secretGoes: $localize`:@@pd.channels.remove-secret-goes:Its stored secret is deleted too.`,
    invalid: $localize`:@@pd.channels.remove-invalid:This window was opened without a channel to remove. Close it and choose Remove on a channel in the list.`,
  };

  protected valid(): boolean {
    return this.channelId().trim() !== '';
  }

  protected question(): string {
    const name = this.name().trim() || this.channelId();
    return $localize`:@@pd.channels.remove-question:Remove "${name}:name:"?`;
  }

  constructor() {
    // The title names the channel; it is known only once the opener's inputs are set (a side effect on the window's title).
    effect(() => {
      if (this.valid()) this.ref.setTitle?.(this.question());
    });
    // Keep has the focus: a stray Enter never removes anything.
    effect(() => {
      if (this.valid()) afterNextRender(() => this.keep()?.nativeElement.focus(), { injector: this.injector });
    });
  }
}
