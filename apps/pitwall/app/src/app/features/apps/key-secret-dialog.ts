import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, DOCUMENT, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { CreatedKey } from '../../core/api.types';

export interface KeySecretData {
  appName: string;
  created: CreatedKey;
}

/** Shows a new key's secret exactly once, with Copy and a clear warning. Nothing keeps the secret after it closes. */
@Component({
  selector: 'pw-key-secret-dialog',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-dialog', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'pw-key-title', 'aria-describedby': 'pw-key-warn' },
  template: `
    <div class="head">
      <span class="ic"><mat-icon svgIcon="pw:key" aria-hidden="true" /></span>
      <div>
        <h2 id="pw-key-title">New key for {{ data.appName }}</h2>
        <p class="faint">Paste it into {{ data.appName }}'s settings.</p>
      </div>
    </div>

    <div class="warn" id="pw-key-warn" role="note">
      <mat-icon svgIcon="pw:alert" aria-hidden="true" />
      <span><b>Copy it now: you won't see it again.</b> Pitwall keeps only a hash. If you lose it, revoke it and create a new one.</span>
    </div>

    <label class="sr-only" for="pw-key-secret">Secret key</label>
    <div class="secret">
      <input id="pw-key-secret" class="mono" readonly [value]="data.created.secret" (focus)="$any($event.target).select()" spellcheck="false" autocomplete="off" />
      <button type="button" class="btn" [class.primary]="!copied()" (click)="copy()" cdkFocusInitial>
        <mat-icon [svgIcon]="copied() ? 'pw:check' : 'pw:copy'" aria-hidden="true" />{{ copied() ? 'Copied' : 'Copy' }}
      </button>
    </div>
    <p class="status" role="status" aria-live="polite">
      @if (copyFailed()) {
        Copying isn't allowed here. Select the key and press Ctrl+C.
      }
    </p>

    <div class="row">
      <span class="faint">Prefix {{ data.created.key.prefix }}</span>
      <button type="button" class="btn" (click)="ref.close()">I've saved it</button>
    </div>
  `,
  styles: `
    :host {
      display: block;
      width: 520px;
      padding: var(--sp-5);
    }
    .head {
      display: flex;
      gap: var(--sp-3);
      align-items: center;
      margin-bottom: var(--sp-4);
      h2 {
        font-size: var(--fs-md);
      }
      p {
        font-size: var(--fs-sm);
      }
    }
    .ic {
      display: grid;
      place-items: center;
      width: 34px;
      height: 34px;
      border-radius: var(--r-lg);
      color: var(--accent);
      background: var(--accent-soft);
    }
    .warn {
      display: flex;
      gap: var(--sp-2);
      align-items: flex-start;
      padding: var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--warn-soft);
      border: 1px solid color-mix(in srgb, var(--warn) 30%, transparent);
      border-radius: var(--r-md);
      mat-icon {
        flex: none;
        color: var(--warn);
        margin-top: 1px;
      }
    }
    .secret {
      display: flex;
      gap: var(--sp-2);
      margin-top: var(--sp-4);
      input {
        flex: 1;
        min-width: 0;
        height: 34px;
        padding: 0 var(--sp-3);
        font-size: var(--fs-sm);
        color: var(--text);
        background: var(--bg-sunken);
        border: 1px solid var(--border-strong);
        border-radius: var(--r-md);
        outline: none;
        &:focus {
          border-color: var(--accent);
          box-shadow: 0 0 0 3px var(--accent-soft);
        }
      }
      .btn {
        height: 34px;
      }
    }
    .status {
      min-height: 18px;
      margin-top: var(--sp-1);
      font-size: var(--fs-xs);
      color: var(--warn);
    }
    .row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-top: var(--sp-3);
      font-size: var(--fs-sm);
    }
  `,
})
export class KeySecretDialog {
  protected readonly data = inject<KeySecretData>(DIALOG_DATA);
  protected readonly ref = inject(DialogRef);
  private readonly doc = inject(DOCUMENT);
  protected readonly copied = signal(false);
  protected readonly copyFailed = signal(false);

  protected async copy(): Promise<void> {
    try {
      const clip = this.doc.defaultView?.navigator.clipboard;
      if (!clip) throw new Error('no clipboard');
      await clip.writeText(this.data.created.secret);
      this.copied.set(true);
      this.copyFailed.set(false);
    } catch {
      this.copyFailed.set(true);
      (this.doc.getElementById('pw-key-secret') as HTMLInputElement | null)?.select();
    }
  }
}
