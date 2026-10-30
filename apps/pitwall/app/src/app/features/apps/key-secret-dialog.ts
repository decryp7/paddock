import { ChangeDetectionStrategy, Component, DOCUMENT, computed, inject, input, signal } from '@angular/core';
import { MatFormField } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** The id of the new key's dialog (`src-tauri/src/dialogs.rs`). */
export const KEY_SECRET_DIALOG_ID = 'pw.key-secret';

/**
 * `pw.key-secret`: a new key's secret, shown exactly once. A required choice: the footer is Copy (the Enter target, it never
 * closes) and "I've saved it", with no Cancel, and Escape or the window's close do nothing, so the secret is not lost by a
 * stray key. Nothing keeps the secret after it closes.
 */
@Component({
  selector: 'pw-key-secret-dialog',
  imports: [MatFormField, MatIconModule, MatInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="sub">Paste it into {{ appName() }}'s settings.</p>
    <div class="warn" id="pw-key-warn" role="note" data-testid="key-warning">
      <mat-icon svgIcon="pw:alert" aria-hidden="true" />
      <span><b>Copy it now: you won't see it again.</b> Pitwall keeps only a hash. If you lose it, revoke it and create a new one.</span>
    </div>
    <label class="sr-only" for="pw-key-secret">Secret key</label>
    <mat-form-field class="secret pitwall-selectable">
      <input matInput id="pw-key-secret" class="mono" readonly aria-describedby="pw-key-warn" [value]="secret()" (focus)="$any($event.target).select()" spellcheck="false" autocomplete="off" />
    </mat-form-field>
    <p class="meta">
      <span>Prefix <span class="mono">{{ prefix() }}</span></span>
      <span class="status" role="status" aria-live="polite">
        @if (copyFailed()) {
          Copying isn't allowed here. Select the key and press Ctrl+C.
        } @else if (copied()) {
          Copied to the clipboard.
        }
      </span>
    </p>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    p { margin: 0; }
    .sub { color: var(--pd-text-muted); }
    .warn {
      display: flex;
      gap: var(--pd-space-2);
      align-items: flex-start;
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      background: var(--pd-warning-tint);
      border-radius: var(--pd-radius-control);
      mat-icon { flex: none; width: 16px; height: 16px; color: var(--pd-warning); margin-top: 1px; }
    }
    .secret { width: 100%; }
    .meta { display: flex; justify-content: space-between; gap: var(--pd-space-3); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .status { text-align: end; }
  `,
})
export class KeySecretDialog implements DialogContent<true> {
  /** The app the key is for. */
  readonly appName = input('');
  /** The secret itself (only here; the server keeps a hash). */
  readonly secret = input('');
  /** The key's public prefix, as the Apps page lists it. */
  readonly prefix = input('');
  private readonly doc = inject(DOCUMENT);
  protected readonly copied = signal(false);
  protected readonly copyFailed = signal(false);

  readonly dialogHeading = computed<string | null>(() => `New key for ${this.appName()}`);

  readonly dialogActions = computed<readonly DialogAction<true>[]>(() => [
    { kind: 'run', id: 'copy', label: this.copied() ? 'Copy again' : 'Copy', role: 'primary', run: () => this.copy() },
    { kind: 'answer', id: 'saved', label: "I've saved it", role: 'secondary', answer: () => true as const },
  ]);

  private async copy(): Promise<void> {
    try {
      const clip = this.doc.defaultView?.navigator.clipboard;
      if (!clip) throw new Error('no clipboard');
      await clip.writeText(this.secret());
      this.copied.set(true);
      this.copyFailed.set(false);
    } catch {
      this.copyFailed.set(true);
      (this.doc.getElementById('pw-key-secret') as HTMLInputElement | null)?.select();
    }
  }
}
