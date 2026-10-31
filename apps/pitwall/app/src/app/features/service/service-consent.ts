import { ChangeDetectionStrategy, Component, Injectable, computed, inject, input, signal } from '@angular/core';
import { MatCheckbox } from '@angular/material/checkbox';
import { DIALOG_REF, DialogService, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import type { Platform } from '../../core/api.types';
import { ServiceStore } from './service.store';

/** The id of the first-run service question (`src-tauri/src/dialogs.rs`). */
export const CONSENT_DIALOG_ID = 'pw.service-consent';

/** What the first-run question answers with: run the service now, or not now (and maybe never ask again). */
export interface ConsentAnswer {
  readonly install: boolean;
  readonly dontAsk: boolean;
}

/** The platform's own wording for what registering the service does and who approves it. */
export function consentDetail(platform: string): string {
  switch (platform) {
    case 'windows':
      return 'Pitwall registers a Windows service. Windows asks you to approve it once (an administrator prompt). It starts with Windows, before you sign in.';
    case 'macos':
      return 'Pitwall adds a launch agent. macOS asks you to allow it in System Settings › General › Login Items. It starts when you sign in.';
    default:
      return 'Pitwall adds a systemd user unit; nothing needs administrator rights. It starts when you sign in.';
  }
}

/**
 * `pw.service-consent`, the first-run question: should Pitwall's server run as a service, so apps can use it without this
 * window? The footer is the frame's: "Not now" (the way out, Escape too) and "Run in the background". "Don't ask again" in the
 * body turns any "not now" into never asking again. It calls nothing: the main window installs or declines (rule 17).
 */
@Component({
  selector: 'pw-service-consent',
  imports: [MatCheckbox],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="msg" data-testid="consent-message">
      Apps on this computer can then use your models when this window is closed and after a restart. Prompts and answers stay on this
      computer; the service makes no network requests except the ones you turn on, such as checks for model updates.
    </p>
    <p class="detail" data-testid="consent-detail">{{ detail() }}</p>
    <mat-checkbox class="never" data-testid="consent-never" [checked]="dontAsk()" (change)="dontAsk.set($event.checked)">Don't ask again</mat-checkbox>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-3); min-width: 0; }
    p { margin: 0; overflow-wrap: anywhere; }
    .msg { color: var(--pd-text-muted); }
    .detail { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .never { margin-inline-start: calc(-1 * var(--pd-space-2)); }
  `,
})
export class ServiceConsentDialog implements DialogContent<ConsentAnswer> {
  /** The OS the service is registered with (its wording); the opener passes it. */
  readonly platform = input<Platform | string>('linux');
  protected readonly dontAsk = signal(false);
  private readonly ref = inject(DIALOG_REF);

  protected readonly detail = computed(() => consentDetail(this.platform()));

  readonly dialogHeading = signal<string | null>("Run Pitwall's server in the background?");

  readonly dialogActions = computed<readonly DialogAction<ConsentAnswer>[]>(() => [
    { kind: 'dismiss', id: 'not-now', label: 'Not now' },
    { kind: 'answer', id: 'install', label: 'Run in the background', role: 'primary', answer: () => ({ install: true, dontAsk: false }) },
  ]);

  /** "Not now" (or Escape, or the window's close) with "Don't ask again" ticked is an answer: never ask again. */
  dialogCanClose(): boolean {
    if (!this.dontAsk()) return true;
    this.ref.answer({ install: false, dontAsk: true });
    return false;
  }
}

/** Opens the question in its window and carries out the answer. */
@Injectable({ providedIn: 'root' })
export class ServiceConsentPrompt {
  private readonly dialogs = inject(DialogService);
  private readonly store = inject(ServiceStore);
  private open = false;

  /** Asks once at a time; does nothing when the store has no question pending. */
  async ask(): Promise<void> {
    if (this.open || !this.store.askConsent()) return;
    this.open = true;
    try {
      const outcome = await this.dialogs.open<ConsentAnswer>(CONSENT_DIALOG_ID, {
        inputs: { platform: this.store.service()?.platform ?? 'linux' },
        title: 'Run in the background',
      }).closed;
      const answer = outcome.kind === 'answered' ? outcome.value : null;
      if (answer?.install) await this.store.install();
      else if (answer?.dontAsk) await this.store.decline();
      else this.store.later();
    } finally {
      this.open = false;
    }
  }
}
