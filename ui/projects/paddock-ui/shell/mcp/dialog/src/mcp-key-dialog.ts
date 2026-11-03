import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import { copyText } from '@decryp7/paddock-ui';
import { McpBackend, mcpMessage } from '@decryp7/paddock-ui/shell/mcp';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { injectTrackLoad } from '@decryp7/paddock-ui/shell/store';

/**
 * The key dialog (a modal form, the credential dialog of the AI access page): step one names the client; Create asks the engine
 * for a key (`mcp_issue_key`); step two shows the key once, with Copy and Done. The key lives in this component's state only:
 * it is in no store, event, log or URL, it is dropped as soon as the dialog closes, and the engine keeps only its hash. The page re-reads its list
 * whenever this dialog closes, however it closed. Its actions are declared (ADR 0026 §2): step one has Create key (the
 * primary, a run: busy while the engine issues it, a refusal shown with Try again) and Cancel; step two has Copy at the
 * leading edge, Close and Done (the primary, answering `true`).
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
      </div>
    } @else {
      <form class="pd-key" (submit)="$event.preventDefault()" data-testid="mcp-key-form">
        <p class="pd-key__lede">{{ t.lede }}</p>
        <label class="pd-key__field">
          <span>{{ t.client }}</span>
          <input
            type="text"
            [formField]="f.client"
            autocomplete="off"
            spellcheck="false"
            data-testid="mcp-key-client"
            data-autofocus
            [attr.aria-invalid]="limit() !== null"
            [attr.aria-describedby]="limit() !== null ? 'pd-key-limit' : null"
          />
        </label>
        @if (limit(); as message) {
          <p class="pd-key__error" id="pd-key-limit" data-testid="mcp-key-limit">{{ message }}</p>
        }
      </form>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; container-type: inline-size; }
    .pd-key { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-key__lede, .pd-key__hint { margin: 0; overflow-wrap: anywhere; }
    .pd-key__hint { color: var(--pd-text-muted); }
    .pd-key__field { display: flex; flex-direction: column; gap: var(--pd-space-1); color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
    .pd-key__error { margin: 0; color: var(--pd-text); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); overflow-wrap: anywhere; }
    input { box-sizing: border-box; width: 100%; min-width: 0; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: normal; }
    input[readonly] { font-family: var(--pd-font-mono, monospace); font-size: var(--pd-text-caption); }
    input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdMcpKeyDialog implements DialogContent<boolean> {
  private readonly backend = inject(McpBackend);
  private readonly trackLoad = injectTrackLoad();

  protected readonly model = signal({ client: '' });
  protected readonly f = form(this.model);
  /** The issued key: shown once, held only here. */
  protected readonly key = signal<string | null>(null);
  protected readonly copyStatus = signal<string | null>(null);
  /** The key limit as a validation message on the name field (null while keys can still be created). */
  protected readonly limit = signal<string | null>(null);

  constructor() {
    // The limit is known before anything is typed: a full list is a validation error, not a failed Create.
    void this.trackLoad(() =>
      this.backend.get().then(
        (view) => {
          if (view.keys.filter((k) => !k.revoked).length >= view.maxKeys) this.limit.set(this.t.limit(view.maxKeys));
        },
        () => undefined,
      ),
    );
  }

  protected readonly t = {
    lede: $localize`:@@pd.mcp.key.lede:Name the program that will connect, for example “Pitwall”. It gets its own key, which you can revoke at any time.`,
    client: $localize`:@@pd.mcp.key.client:Client name`,
    close: $localize`:@@pd.mcp.key.close:Close`,
    create: $localize`:@@pd.mcp.key.create:Create key`,
    creating: $localize`:@@pd.mcp.key-creating:Creating…`,
    once: $localize`:@@pd.mcp.key.once:This is the only time the key is shown. Copy it now; the app keeps only a fingerprint and cannot show it again.`,
    keyLabel: $localize`:@@pd.mcp.key.key-label:Key`,
    how: $localize`:@@pd.mcp.key.how:Give it to the client as the first line it sends when it connects: {"key": "..."}. Anyone with the key can read what the enabled tools expose.`,
    copy: $localize`:@@pd.mcp.key.copy:Copy`,
    copied: $localize`:@@pd.mcp.key.copied:Copied to the clipboard.`,
    notCopied: $localize`:@@pd.mcp.key.not-copied:The clipboard is not available. Select the key and copy it with the keyboard.`,
    done: $localize`:@@pd.mcp.key.done:Done`,
    limit: (max: number) => $localize`:@@pd.mcp.key.limit:${max}:max: keys are in use. Revoke one you no longer need first.`,
    failed: $localize`:@@pd.mcp.key.failed:The engine could not issue a key.`,
  };

  /** Step one: Create key (enabled once a client is named). Step two: Copy, Close and Done. */
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() => {
    const issued = this.key();
    if (issued === null) {
      return [{ kind: 'run', id: 'create', label: this.t.create, busyLabel: this.t.creating, role: 'primary', enabled: this.limit() === null && this.model().client.trim().length > 0, run: () => this.create() }];
    }
    return [
      { kind: 'run', id: 'copy', label: this.t.copy, role: 'secondary', lead: true, run: () => this.copy(issued) },
      { kind: 'dismiss', id: 'close', label: this.t.close },
      { kind: 'answer', id: 'done', label: this.t.done, role: 'primary', answer: () => this.done() },
    ];
  });

  private async create(): Promise<void> {
    try {
      const issued = await this.backend.issueKey(this.model().client);
      this.key.set(issued.key);
    } catch (e) {
      throw new DialogActionError(mcpMessage(e, this.t.failed));
    }
  }

  private async copy(key: string): Promise<void> {
    this.copyStatus.set((await copyText(key)) ? this.t.copied : this.t.notCopied);
  }

  private done(): boolean {
    this.key.set(null);
    return true;
  }
}
