import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import { copyText } from '@decryp7/paddock-ui';
import { McpBackend, mcpMessage } from '@decryp7/paddock-ui/shell/mcp';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The key dialog (a modal form, the credential dialog of the AI access page): step one names the client; Create asks the engine
 * for a key (`mcp_issue_key`); step two shows the key once, with Copy and Done. The key lives in this component's state only:
 * it is in no store, event, log or URL, it is dropped as soon as the dialog closes, and the engine keeps only its hash. The page re-reads its list
 * whenever this dialog closes, however it closed. Enter creates, Escape cancels (the dialog
 * shell handles both: Create is the default action, Cancel closes).
 */
@Component({
  selector: 'pd-mcp-key-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FormField],
  template: `
    @if (key(); as issued) {
      <div class="pd-key" data-testid="mcp-key-shown">
        <p class="pd-key__lede">{{ t.once }}</p>
        <label class="pd-key__field">
          <span>{{ t.keyLabel }}</span>
          <input type="text" readonly [value]="issued" spellcheck="false" autocomplete="off" data-testid="mcp-key-value" (focus)="$any($event.target).select()" />
        </label>
        <p class="pd-key__hint">{{ t.how }}</p>
        @if (copyStatus(); as c) {
          <p class="pd-key__hint" role="status" data-testid="mcp-key-copy-status">{{ c }}</p>
        }
        <div class="pd-key__actions">
          <button type="button" class="pd-key__btn" data-testid="mcp-key-copy" (click)="copy(issued)">{{ t.copy }}</button>
          <button type="button" class="pd-key__btn primary" data-default data-dialog-close data-testid="mcp-key-done" (click)="done()">{{ t.done }}</button>
        </div>
      </div>
    } @else {
      <form class="pd-key" (submit)="$event.preventDefault(); create()" data-testid="mcp-key-form">
        <p class="pd-key__lede">{{ t.lede }}</p>
        <label class="pd-key__field">
          <span>{{ t.client }}</span>
          <input
            type="text"
            [formField]="f.client"
            autocomplete="off"
            spellcheck="false"
            data-testid="mcp-key-client"
            [attr.aria-invalid]="error() ? true : null"
            [attr.aria-describedby]="error() ? 'pd-key-error' : null"
            [readOnly]="busy()"
          />
        </label>
        @if (error(); as e) {
          <p id="pd-key-error" class="pd-key__error" role="alert" data-testid="mcp-key-error">{{ e }}</p>
        }
        <div class="pd-key__actions">
          <button type="button" class="pd-key__btn" data-dialog-close data-testid="mcp-key-cancel" (click)="ref.close()">{{ t.cancel }}</button>
          <button type="submit" class="pd-key__btn primary" data-testid="mcp-key-create" [disabled]="!canCreate()">{{ t.create }}</button>
        </div>
      </form>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; container-type: inline-size; }
    .pd-key { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-key__lede, .pd-key__hint { margin: 0; overflow-wrap: anywhere; }
    .pd-key__hint { color: var(--pd-text-muted); }
    .pd-key__field { display: flex; flex-direction: column; gap: var(--pd-space-1); font-weight: var(--pd-weight-medium); }
    input { box-sizing: border-box; width: 100%; min-width: 0; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: normal; }
    input[readonly] { font-family: var(--pd-font-mono, monospace); font-size: var(--pd-text-caption); }
    input:focus-visible, .pd-key__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-key__error { margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); overflow-wrap: anywhere; }
    .pd-key__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-key__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-key__btn.primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
    .pd-key__btn:disabled { opacity: 0.6; cursor: default; }
  `,
})
export class PdMcpKeyDialog {
  private readonly backend = inject(McpBackend);
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);

  protected readonly model = signal({ client: '' });
  protected readonly f = form(this.model);
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  /** The issued key: shown once, held only here. */
  protected readonly key = signal<string | null>(null);
  protected readonly copyStatus = signal<string | null>(null);

  protected readonly t = {
    lede: $localize`:@@pd.mcp.key.lede:Name the program that will connect, for example "Pitwall". It gets its own key, which you can revoke at any time.`,
    client: $localize`:@@pd.mcp.key.client:Client name`,
    cancel: $localize`:@@pd.mcp.key.cancel:Cancel`,
    create: $localize`:@@pd.mcp.key.create:Create key`,
    once: $localize`:@@pd.mcp.key.once:This is the only time the key is shown. Copy it now; the app keeps only a fingerprint and cannot show it again.`,
    keyLabel: $localize`:@@pd.mcp.key.key-label:Key`,
    how: $localize`:@@pd.mcp.key.how:Give it to the client as the first line it sends when it connects: {"key": "..."}. Anyone with the key can read what the enabled tools expose.`,
    copy: $localize`:@@pd.mcp.key.copy:Copy`,
    copied: $localize`:@@pd.mcp.key.copied:Copied to the clipboard.`,
    notCopied: $localize`:@@pd.mcp.key.not-copied:The clipboard is not available. Select the key and copy it with the keyboard.`,
    done: $localize`:@@pd.mcp.key.done:Done`,
    failed: $localize`:@@pd.mcp.key.failed:The engine could not issue a key.`,
  };

  protected canCreate(): boolean {
    return !this.busy() && this.model().client.trim().length > 0;
  }

  protected async create(): Promise<void> {
    if (!this.canCreate()) return;
    const client = this.model().client;
    this.busy.set(true);
    this.error.set(null);
    try {
      const issued = await this.backend.issueKey(client);
      this.key.set(issued.key);
    } catch (e) {
      this.error.set(mcpMessage(e, this.t.failed));
    }
    this.busy.set(false);
  }

  protected async copy(key: string): Promise<void> {
    this.copyStatus.set((await copyText(key)) ? this.t.copied : this.t.notCopied);
  }

  protected done(): void {
    this.key.set(null);
    this.ref.close(true);
  }
}
