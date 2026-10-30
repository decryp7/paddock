import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, DOCUMENT, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import type { CreatedKey } from '../../core/api.types';

export interface KeySecretData {
  appName: string;
  created: CreatedKey;
}

/** Shows a new key's secret exactly once, with Copy and a clear warning. Nothing keeps the secret after it closes. */
@Component({
  selector: 'pw-key-secret-dialog',
  imports: [MatButton, MatFormField, MatIconModule, MatInput],
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
    <div class="secret pitwall-selectable">
      <mat-form-field class="field"><input matInput id="pw-key-secret" class="mono" readonly [value]="data.created.secret" (focus)="$any($event.target).select()" spellcheck="false" autocomplete="off" /></mat-form-field>
      <button type="button" [matButton]="copied() ? 'outlined' : 'filled'" (click)="copy()" cdkFocusInitial>
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
      <button type="button" matButton="outlined" (click)="ref.close()">I've saved it</button>
    </div>
  `,
  styles: `
    :host {
      display: block;
      width: 520px;
      padding: var(--pd-space-4);
    }
    .head {
      display: flex;
      gap: var(--pd-space-3);
      align-items: center;
      margin-bottom: var(--pd-space-4);
      h2 {
        font-size: var(--pd-text-body-lg);
      }
      p {
        font-size: var(--pd-text-label);
      }
    }
    .ic {
      display: grid;
      place-items: center;
      width: 34px;
      height: 34px;
      border-radius: var(--pd-radius-card);
      color: var(--pd-accent);
      background: var(--pd-accent-tint);
    }
    .warn {
      display: flex;
      gap: var(--pd-space-2);
      align-items: flex-start;
      padding: var(--pd-space-3);
      font-size: var(--pd-text-label);
      background: var(--pd-warning-tint);
      border: 1px solid color-mix(in srgb, var(--pd-warning) 30%, transparent);
      border-radius: var(--pd-radius-control);
      mat-icon {
        flex: none;
        color: var(--pd-warning);
        margin-top: 1px;
      }
    }
    .secret {
      display: flex;
      gap: var(--pd-space-2);
      margin-top: var(--pd-space-4);
      align-items: center;
      .field {
        flex: 1;
        min-width: 0;
      }
      input {
        font-size: var(--pd-text-label);
      }
    }
    .status {
      min-height: 18px;
      margin-top: var(--pd-space-1);
      font-size: var(--pd-text-label);
      color: var(--pd-warning);
    }
    .row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-top: var(--pd-space-3);
      font-size: var(--pd-text-label);
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
