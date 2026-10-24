import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';

/** The registration wizard's dialog id (`pc.register` in the contributions). */
export const REGISTER_DIALOG_ID = 'pc.register';

/** Opens the registration wizard: a modal window of its own, loaded on first use. */
@Injectable({ providedIn: 'root' })
export class RegisterLauncher {
  private readonly ports = inject(SHELL_PORTS);

  /** Resolves true when a runner was registered. */
  async open(): Promise<boolean> {
    return (await this.ports.openDialog(REGISTER_DIALOG_ID)) === true;
  }
}
