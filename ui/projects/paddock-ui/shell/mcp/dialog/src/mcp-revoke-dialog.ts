import { afterNextRender, ChangeDetectionStrategy, Component, effect, ElementRef, inject, Injector, input, viewChild } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The revoke confirmation (a modal decision, one window per key): it names the client and says what revoking does. Keep is the
 * default (Enter, Escape, focus on opening); Revoke is the danger action and is never the default. It calls nothing: it resolves
 * `true` for Revoke and `false` for Keep, and the page revokes the key. A window opened without a key says so inline, with a
 * Close button, instead of showing a question about nothing.
 */
@Component({
  selector: 'pd-mcp-revoke-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <div class="pd-rv">
        <p class="pd-rv__text" data-testid="mcp-revoke-text">{{ question() }} {{ t.effect }}</p>
        <div class="pd-rv__actions">
          <button type="button" class="pd-rv__btn pd-rv__btn--danger" data-testid="mcp-revoke-yes" (click)="ref.close(true)">{{ t.revoke }}</button>
          <button #keep type="button" class="pd-rv__btn" data-default data-dialog-close data-testid="mcp-revoke-keep" (click)="ref.close(false)">{{ t.keep }}</button>
        </div>
      </div>
    } @else {
      <div class="pd-rv">
        <p class="pd-rv__error" role="alert" data-testid="mcp-revoke-invalid">{{ t.invalid }}</p>
        <div class="pd-rv__actions">
          <button type="button" class="pd-rv__btn" data-dialog-close data-testid="mcp-revoke-close" (click)="ref.close()">{{ t.close }}</button>
        </div>
      </div>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-rv { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-rv__text, .pd-rv__error { margin: 0; overflow-wrap: anywhere; }
    .pd-rv__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
    .pd-rv__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-rv__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-rv__btn--danger { background: var(--pd-danger-fill); border-color: var(--pd-danger-fill); color: var(--pd-on-danger); }
    .pd-rv__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdMcpRevokeDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
  private readonly injector = inject(Injector);
  private readonly keep = viewChild<ElementRef<HTMLButtonElement>>('keep');

  /** The key to revoke. */
  readonly keyId = input<number | null>(null);
  /** The client it belongs to, for the question. */
  readonly client = input('');

  protected readonly t = {
    revoke: $localize`:@@pd.mcp.revoke-yes:Revoke`,
    keep: $localize`:@@pd.mcp.revoke-keep:Keep`,
    close: $localize`:@@pd.mcp.revoke-close:Close`,
    effect: $localize`:@@pd.mcp.revoke-effect:It stops working at once, and the client has to be given a new key to connect again.`,
    invalid: $localize`:@@pd.mcp.revoke-invalid:This window was opened without a key to revoke. Close it and choose Revoke on a client in the list.`,
  };

  protected valid(): boolean {
    return this.keyId() !== null;
  }

  protected question(): string {
    const client = this.client().trim() || String(this.keyId());
    return $localize`:@@pd.mcp.revoke-question:Revoke the key of "${client}:client:"?`;
  }

  constructor() {
    // The title names the client; it is known only once the opener's inputs are set (a side effect on the window's title).
    effect(() => {
      if (this.valid()) this.ref.setTitle?.(this.question());
    });
    // Keep has the focus: a stray Enter never revokes anything.
    effect(() => {
      if (this.valid()) afterNextRender(() => this.keep()?.nativeElement.focus(), { injector: this.injector });
    });
  }
}
