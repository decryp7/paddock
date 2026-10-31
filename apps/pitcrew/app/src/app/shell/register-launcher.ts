import { Injectable, inject, signal } from '@angular/core';
import { answerOf, SHELL_PORTS } from '@decryp7/paddock-ui/shell';

/** The registration wizard's dialog id (`pc.register` in the contributions). */
export const REGISTER_DIALOG_ID = 'pc.register';

/** Opens the registration wizard: a modal window of its own, loaded on first use. */
@Injectable({ providedIn: 'root' })
export class RegisterLauncher {
  private readonly ports = inject(SHELL_PORTS);

  private readonly openNow = signal(false);
  /** Whether the registration window is open (the main window follows the job it starts only while it is). */
  readonly isOpen = this.openNow.asReadonly();

  /** Resolves true when a runner was registered. */
  async open(): Promise<boolean> {
    this.openNow.set(true);
    try {
      return answerOf(await this.ports.openDialog(REGISTER_DIALOG_ID)) === true;
    } finally {
      this.openNow.set(false);
    }
  }
}
