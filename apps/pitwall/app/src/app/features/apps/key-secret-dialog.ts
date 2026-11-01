import { ChangeDetectionStrategy, Component, DOCUMENT, computed, inject, input, signal } from '@angular/core';
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
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="sub">Paste it into {{ appName() }}'s settings.</p>
    <!-- A callout (one status bar, no icon of its own): the decision's icon beside the heading is the dialog's one icon. -->
    <p class="warn" id="pw-key-warn" role="note" data-testid="key-warning">
      <b>Copy it now: you won't see it again.</b> Pitwall keeps only a hash. If you lose it, revoke it and create a new one.
    </p>
    <!--
      The whole key, centred in one read-only mono field and never cut: a 46-character key does not fit one line of a
      400 px window, so it wraps into two balanced lines (a soft wrap: copying it gives the key without a line break).
      Focus selects all of it (a click does too, by user-select: all).
    -->
    <div
      id="pw-key-secret"
      class="secret pitwall-selectable"
      role="textbox"
      aria-readonly="true"
      aria-label="Secret key"
      aria-describedby="pw-key-warn"
      tabindex="0"
      data-testid="key-secret"
      (focus)="selectKey()"
    >{{ secret() }}</div>
    <p class="meta">
      <span>Prefix <span class="mono">{{ prefix() }}</span></span>
      <span class="copy-note" role="status" aria-live="polite">
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
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      line-height: 1.45;
      background: var(--pd-surface-2);
      border-left: 3px solid var(--pd-warning);
      border-radius: var(--pd-radius-tag);
    }
    .secret {
      padding: var(--pd-space-2) var(--pd-space-3);
      font-family: var(--pd-font-mono);
      font-size: var(--pd-text-body);
      line-height: 1.5;
      text-align: center;
      text-wrap: balance;
      word-break: break-all;
      user-select: all;
      color: var(--pd-text);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-border-control);
      border-radius: var(--pd-radius-control);
      &:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 1px; }
    }
    .meta { display: flex; justify-content: space-between; gap: var(--pd-space-3); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .copy-note { text-align: end; }
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

  /** Selects the whole key (a click or focus), so a manual copy takes all of it. */
  protected selectKey(): void {
    const el = this.doc.getElementById('pw-key-secret');
    const sel = this.doc.defaultView?.getSelection();
    if (!el || !sel) return;
    const range = this.doc.createRange();
    range.selectNodeContents(el);
    sel.removeAllRanges();
    sel.addRange(range);
  }

  private async copy(): Promise<void> {
    try {
      const clip = this.doc.defaultView?.navigator.clipboard;
      if (!clip) throw new Error('no clipboard');
      await clip.writeText(this.secret());
      this.copied.set(true);
      this.copyFailed.set(false);
    } catch {
      this.copyFailed.set(true);
      this.selectKey();
    }
  }
}
