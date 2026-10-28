import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';

export interface ConfirmData {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
}

/**
 * Asks the user to confirm: the shell's `sp.confirm` decision dialog, a real OS window owned by and centred on the main
 * window (rule 17), modal. Resolves false when dismissed.
 */
@Injectable({ providedIn: 'root' })
export class Confirm {
  private readonly ports = inject(SHELL_PORTS);

  async ask(data: ConfirmData): Promise<boolean> {
    const input = { title: data.title, message: data.message, confirmLabel: data.confirmLabel, danger: data.danger ?? false };
    return (await this.ports.openDialog('sp.confirm', input)) === true;
  }
}
