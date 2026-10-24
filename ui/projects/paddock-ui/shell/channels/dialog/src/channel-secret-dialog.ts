import { ChangeDetectionStrategy, Component, effect, inject, input, signal } from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import { ChannelsBackend, channelsMessage } from '@decryp7/paddock-ui/shell/channels';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The secret dialog (a modal form): takes the token or password of one channel and sends it straight to the engine
 * (`channels_set_secret`). The secret lives in this input only: it is in no store, event, log or URL, it is cleared as soon as
 * the engine has it, and the dialog resolves `true` (stored) or nothing (dismissed). Enter saves, Escape cancels (the dialog
 * shell handles both: the Save button is the default action, Cancel closes).
 */
@Component({
  selector: 'pd-channel-secret-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FormField],
  template: `
    @if (!valid()) {
      <div class="pd-secret">
        <p class="pd-secret__error" role="alert" data-testid="channel-secret-invalid">{{ t.invalid }}</p>
        <div class="pd-secret__actions">
          <button type="button" class="pd-secret__btn" data-dialog-close data-testid="channel-secret-close" (click)="ref.close()">{{ t.close }}</button>
        </div>
      </div>
    } @else {
    <form class="pd-secret" (submit)="$event.preventDefault(); save()" data-testid="channel-secret-form">
      <p class="pd-secret__lede">{{ lede() }}</p>
      <label class="pd-secret__field">
        <span>{{ fieldLabel() }}</span>
        <span class="pd-secret__control">
          <input
            [type]="shown() ? 'text' : 'password'"
            [formField]="f.secret"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            data-testid="channel-secret-input"
            [attr.aria-invalid]="error() ? true : null"
            [attr.aria-describedby]="error() ? 'pd-secret-error' : null"
            [readOnly]="saving()"
          />
          <button type="button" class="pd-secret__eye" [attr.aria-label]="shown() ? t.hide : t.show" [attr.aria-pressed]="shown()" [title]="shown() ? t.hide : t.show" data-testid="channel-secret-toggle" (click)="shown.set(!shown())">
            {{ shown() ? t.hideShort : t.showShort }}
          </button>
        </span>
      </label>
      @if (error(); as e) {
        <p id="pd-secret-error" class="pd-secret__error" role="alert" data-testid="channel-secret-error">{{ e }}</p>
      }
      <div class="pd-secret__actions">
        <button type="button" class="pd-secret__btn" data-dialog-close data-testid="channel-secret-cancel" (click)="ref.close()">{{ t.cancel }}</button>
        <button type="submit" class="pd-secret__btn primary" data-testid="channel-secret-save" [disabled]="!canSave()">{{ t.save }}</button>
      </div>
    </form>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; container-type: inline-size; }
    .pd-secret { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-secret__lede { margin: 0; color: var(--pd-text-muted); overflow-wrap: anywhere; }
    .pd-secret__field { display: flex; flex-direction: column; gap: var(--pd-space-1); font-weight: var(--pd-weight-medium); }
    .pd-secret__control { display: flex; gap: var(--pd-space-1); min-width: 0; }
    input { flex: 1; min-width: 0; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: normal; }
    input:focus-visible, .pd-secret__eye:focus-visible, .pd-secret__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-secret__eye { flex: none; display: inline-flex; align-items: center; justify-content: center; min-width: 4rem; padding: 0 var(--pd-space-2); font: inherit; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text-muted); cursor: pointer; }
    .pd-secret__eye[aria-pressed='true'] { color: var(--pd-text); background: var(--pd-hover); }
    .pd-secret__error { margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); overflow-wrap: anywhere; }
    .pd-secret__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-secret__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-secret__btn.primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
    .pd-secret__btn:disabled { opacity: 0.6; cursor: default; }
  `,
})
export class PdChannelSecretDialog {
  private readonly backend = inject(ChannelsBackend);
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);

  /** The channel the secret is for. */
  readonly channelId = input('');
  /** The channel's name, for the title and the sentence. */
  readonly label = input('');
  /** Whether the secret is a token or a password. */
  readonly kind = input<'token' | 'password'>('token');

  protected readonly model = signal({ secret: '' });
  protected readonly f = form(this.model);
  protected readonly shown = signal(false);
  protected readonly saving = signal(false);
  protected readonly error = signal<string | null>(null);

  protected readonly t = {
    cancel: $localize`:@@pd.channels.secret.cancel:Cancel`,
    close: $localize`:@@pd.channels.secret.close:Close`,
    invalid: $localize`:@@pd.channels.secret.invalid:This window was opened without a channel to set a secret for. Close it and choose Set token or Set password in the channel's editor.`,
    save: $localize`:@@pd.channels.secret.save:Save`,
    show: $localize`:@@pd.channels.secret.show:Show what you typed`,
    hide: $localize`:@@pd.channels.secret.hide:Hide what you typed`,
    showShort: $localize`:@@pd.channels.secret.show-short:Show`,
    hideShort: $localize`:@@pd.channels.secret.hide-short:Hide`,
  };

  constructor() {
    // The title names the channel; it is known only once the opener's inputs are set (a side effect on the window's title).
    effect(() => {
      if (!this.valid()) return;
      const label = this.label() || this.channelId();
      this.ref.setTitle?.(this.kind() === 'password' ? $localize`:@@pd.channels.secret.title-password:Password for ${label}:channel:` : $localize`:@@pd.channels.secret.title-token:Token for ${label}:channel:`);
    });
  }

  protected valid(): boolean {
    return this.channelId().trim() !== '';
  }

  protected fieldLabel(): string {
    return this.kind() === 'password' ? $localize`:@@pd.channels.secret.password:Password` : $localize`:@@pd.channels.secret.token:Token`;
  }

  protected lede(): string {
    const label = this.label() || this.channelId();
    return this.kind() === 'password'
      ? $localize`:@@pd.channels.secret.lede-password:The password for ${label}:channel: is kept by the app's engine, not in the settings file. It is never shown again.`
      : $localize`:@@pd.channels.secret.lede-token:The token for ${label}:channel: is kept by the app's engine, not in the settings file. It is never shown again.`;
  }

  protected canSave(): boolean {
    return !this.saving() && this.model().secret.length > 0;
  }

  protected async save(): Promise<void> {
    const value = this.model().secret;
    const id = this.channelId();
    if (!this.valid() || !this.canSave()) return;
    this.saving.set(true);
    this.error.set(null);
    try {
      await this.backend.setSecret(id, value);
    } catch (e) {
      this.error.set(channelsMessage(e, $localize`:@@pd.channels.secret.failed:The engine could not store the secret.`));
      this.saving.set(false);
      return;
    }
    this.model.set({ secret: '' });
    this.saving.set(false);
    this.ref.close(true);
  }
}
