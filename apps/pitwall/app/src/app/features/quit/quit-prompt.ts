import { Dialog, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, Injectable, inject, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { BACKEND, type Unsubscribe } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

/** What the dialog answers with: nothing (cancel), or whether to stop serving and whether to remember the choice. */
export type QuitAnswer = { stopServing: boolean; remember: boolean } | null;

/** The "When you quit Pitwall" question, shown when the setting is "Ask each time". */
@Component({
  selector: 'pw-quit-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-dialog', role: 'alertdialog', 'aria-modal': 'true', 'aria-labelledby': 'pw-quit-title', 'aria-describedby': 'pw-quit-msg' },
  template: `
    <h2 id="pw-quit-title">Quit Pitwall?</h2>
    <p id="pw-quit-msg">
      Apps on this computer can keep using your models while Pitwall runs in the background. Stopping serving unloads every model and
      ends the engine processes; Pitwall serves again when you open it.
    </p>
    <label class="remember"><input type="checkbox" [checked]="remember()" (change)="remember.set($any($event.target).checked)" /> Remember my choice</label>
    <div class="row">
      <button type="button" class="btn" (click)="ref.close(null)">Cancel</button>
      <button type="button" class="btn" (click)="ref.close({ stopServing: true, remember: remember() })">Quit and stop serving</button>
      <button type="button" class="btn primary" cdkFocusInitial (click)="ref.close({ stopServing: false, remember: remember() })">Quit, keep serving</button>
    </div>
  `,
  styles: `
    :host {
      display: block;
      width: 480px;
      padding: var(--pd-space-4);
    }
    h2 {
      font-size: var(--pd-text-body-lg);
      margin-bottom: var(--pd-space-2);
    }
    p {
      color: var(--pd-text-muted);
      margin-bottom: var(--pd-space-3);
    }
    .remember {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
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
export class QuitDialog {
  protected readonly ref = inject<DialogRef<QuitAnswer>>(DialogRef);
  protected readonly remember = signal(false);
}

/** Shows the dialog when the app asks for it (and when a quit was asked for before the window existed). */
@Injectable({ providedIn: 'root' })
export class QuitPrompt {
  private readonly dialog = inject(Dialog);
  private readonly backend = inject(BACKEND);
  private readonly toasts = inject(Toasts);
  private open = false;

  /** Listens for the app's request; returns the function that stops. A request made before this window loaded is shown now. */
  watch(): Unsubscribe {
    const stop = this.backend.onQuitAsk(() => void this.ask());
    void this.backend.takeQuitAsk().then(
      (pending) => {
        if (pending) void this.ask();
      },
      () => undefined,
    );
    return stop;
  }

  /** Asks once at a time and carries out the answer (the app quits). */
  async ask(): Promise<void> {
    if (this.open) return;
    this.open = true;
    try {
      const ref = this.dialog.open<QuitAnswer>(QuitDialog, { backdropClass: 'pw-backdrop', panelClass: 'pw-dialog-panel' });
      const answer = await firstValueFrom(ref.closed);
      if (!answer) return;
      try {
        await this.backend.quitApp(answer.stopServing, answer.remember);
      } catch (e) {
        this.toasts.error(toCommandError(e));
      }
    } finally {
      this.open = false;
    }
  }
}
