import { ChangeDetectionStrategy, Component, computed, input, signal } from '@angular/core';
import { MatFormField, MatHint, MatLabel } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** The id of the "Connect <app>" dialog (`src-tauri/src/dialogs.rs`). */
export const MCP_KEY_DIALOG_ID = 'pw.mcp-key';

/**
 * `pw.mcp-key`: asks for the AI access key an app issued (Dyno: Settings > AI access > Issue key) and answers it trimmed. A
 * real OS window (rule 17). The field is a password field, so the key is never shown on screen; the Settings page that
 * opened it keeps the key in Rust (`set_app_key`), which checks it again. Connect is enabled once the text looks like a key.
 */
@Component({
  selector: 'pw-mcp-key-dialog',
  imports: [MatFormField, MatLabel, MatHint, MatInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="msg">In {{ app() }}, open Settings &gt; AI access, turn it on and choose Issue key. Name it "Pitwall", then paste the key here. {{ app() }} shows it only once.</p>
    <mat-form-field class="key">
      <mat-label>AI access key</mat-label>
      <input matInput id="pw-mcp-key" type="password" autocomplete="off" spellcheck="false" placeholder="pmk_…" [value]="key()" (input)="key.set($any($event.target).value)" />
      @if (notAKey()) {
        <mat-hint class="bad" data-testid="mcp-key-not-a-key">An AI access key starts with pmk_.</mat-hint>
      } @else {
        <mat-hint>Kept on this computer, never shown again.</mat-hint>
      }
    </mat-form-field>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    .msg { margin: 0; color: var(--pd-text-muted); }
    .key { width: 100%; }
    .bad { color: var(--pd-text); }
  `,
})
export class McpKeyDialog implements DialogContent<string> {
  /** The app's title ("Dyno"). */
  readonly app = input('the app');
  protected readonly key = signal('');
  private readonly trimmed = computed(() => this.key().trim());
  /** Something was typed or pasted and it is not a key: the hint says why Connect stays off. */
  protected readonly notAKey = computed(() => this.trimmed().length > 0 && !/^pmk_\S+$/.test(this.trimmed()));

  readonly dialogHeading = computed<string | null>(() => `Paste ${this.app()}'s AI access key`);

  readonly dialogActions = computed<readonly DialogAction<string>[]>(() => {
    const key = this.trimmed();
    return [
      { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
      { kind: 'answer', id: 'connect', label: 'Connect', role: 'primary', enabled: /^pmk_\S+$/.test(key), answer: () => key },
    ];
  });
}
