import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The revoke confirmation (a modal decision, one window per key): it names the client and says what revoking does. Its
 * actions are declared (ADR 0026 §2): Keep is the way out and has the focus and Enter; Revoke key is the destructive answer
 * and is never the Enter target. It calls nothing: it answers `true` for Revoke key and is dismissed by Keep, and the page
 * revokes the key. A window opened without a key says so inline, with a Close.
 */
@Component({
  selector: 'pd-mcp-revoke-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <div class="pd-rv">
        <p class="pd-rv__text" data-testid="mcp-revoke-text">{{ t.effect }}</p>
      </div>
    } @else {
      <div class="pd-rv">
        <p class="pd-rv__error" role="alert" data-testid="mcp-revoke-invalid">{{ t.invalid }}</p>
      </div>
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .pd-rv { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-rv__text, .pd-rv__error { margin: 0; overflow-wrap: anywhere; }
    .pd-rv__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  `,
})
export class PdMcpRevokeDialog implements DialogContent<boolean> {

  /** The key to revoke. */
  readonly keyId = input<number | null>(null);
  /** The client it belongs to, for the question. */
  readonly client = input('');

  protected readonly t = {
    revoke: $localize`:@@pd.mcp.revoke-yes:Revoke key`,
    keep: $localize`:@@pd.mcp.revoke-keep:Keep`,
    close: $localize`:@@pd.mcp.revoke-close:Close`,
    effect: $localize`:@@pd.mcp.revoke-effect:It stops working at once, and the client has to be given a new key to connect again.`,
    invalid: $localize`:@@pd.mcp.revoke-invalid:This window was opened without a key to revoke. Close it and choose Revoke on a client in the list.`,
  };

  protected valid(): boolean {
    return this.keyId() !== null;
  }

  /** Keep and Revoke key; a window without a key offers only Close. */
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() =>
    this.valid()
      ? [
          { kind: 'dismiss', id: 'keep', label: this.t.keep },
          { kind: 'answer', id: 'revoke', label: this.t.revoke, role: 'destructive', answer: () => true },
        ]
      : [{ kind: 'dismiss', id: 'close', label: this.t.close }],
  );

  /** The question, as the decision's heading (ADR 0026: icon, heading, message). */
  readonly dialogHeading = computed(() => (this.valid() ? this.question() : null));

  protected question(): string {
    const client = this.client().trim() || String(this.keyId());
    return $localize`:@@pd.mcp.revoke-question:Revoke the key of “${client}:client:”?`;
  }
}
