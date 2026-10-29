import { Dialog, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { ServiceStore } from './service.store';

/** What the first-run question answers with. */
export type ConsentAnswer = 'install' | 'later' | 'decline';

/** The platform's own wording for what registering the service does and who approves it. */
export function consentDetail(platform: string): string {
  switch (platform) {
    case 'windows':
      return 'Windows asks you to approve this once (an administrator prompt). The service starts with Windows, before you sign in.';
    case 'macos':
      return 'macOS asks you to allow Pitwall under System Settings, General, Login Items. The service starts when you sign in.';
    default:
      return 'Pitwall adds a systemd user service; nothing needs administrator rights. It starts when you sign in.';
  }
}

/**
 * The first-run question: should Pitwall's server run as a service, so apps can use it without this window? Nothing is
 * installed unless the user says yes; "Not now" asks again next time, "No thanks" never asks again.
 */
@Component({
  selector: 'pw-service-consent',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-dialog', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'pw-consent-title', 'aria-describedby': 'pw-consent-msg' },
  template: `
    <h2 id="pw-consent-title">Run Pitwall's server in the background?</h2>
    <p id="pw-consent-msg">
      Apps on this computer can then use your models even when this window is closed and after a restart. Pitwall never sends anything off
      this computer.
    </p>
    <p class="faint">{{ detail }}</p>
    <div class="row">
      <button type="button" class="btn" (click)="ref.close('decline')">No thanks</button>
      <button type="button" class="btn" (click)="ref.close('later')">Not now</button>
      <button type="button" class="btn primary" cdkFocusInitial (click)="ref.close('install')">Run in the background</button>
    </div>
  `,
  styles: `
    :host {
      display: block;
      width: 460px;
      padding: var(--pd-space-4);
    }
    h2 {
      font-size: var(--pd-text-body-lg);
      margin-bottom: var(--pd-space-2);
    }
    p {
      color: var(--pd-text-muted);
      margin-bottom: var(--pd-space-2);
    }
    .faint {
      font-size: var(--pd-text-label);
    }
    .row {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
      margin-top: var(--pd-space-4);
    }
  `,
})
export class ServiceConsent {
  protected readonly ref = inject<DialogRef<ConsentAnswer>>(DialogRef);
  protected readonly store = inject(ServiceStore);
  protected readonly detail = consentDetail(this.store.service()?.platform ?? 'linux');
}

/** Opens the question and carries out the answer. */
@Injectable({ providedIn: 'root' })
export class ServiceConsentPrompt {
  private readonly dialog = inject(Dialog);
  private readonly store = inject(ServiceStore);
  private open = false;

  /** Asks once at a time; does nothing when the store has no question pending. */
  async ask(): Promise<void> {
    if (this.open || !this.store.askConsent()) return;
    this.open = true;
    try {
      const ref = this.dialog.open<ConsentAnswer>(ServiceConsent, { backdropClass: 'pw-backdrop', panelClass: 'pw-dialog-panel' });
      const answer = (await firstValueFrom(ref.closed)) ?? 'later';
      if (answer === 'install') await this.store.install();
      else if (answer === 'decline') await this.store.decline();
      else this.store.later();
    } finally {
      this.open = false;
    }
  }
}
