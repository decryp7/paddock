import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { CONFIRM_DIALOG_ID, type ConfirmInput } from '@decryp7/paddock-ui/shell/confirm';

/** What deleting the watch `name` asks (both call sites: the list and the detail page). */
export function deleteWatchPrompt(name: string): ConfirmInput {
  return {
    heading: `Delete "${name}"?`,
    message: 'Spotter stops watching it and forgets its history. Downloaded files stay where they are.',
    confirm: { label: 'Delete watch', destructive: true },
  };
}

/**
 * Asks the user to confirm with the shared `pd.confirm` decision (ADR 0026 §5): a real OS window owned by and centred on
 * the main window (rule 17), modal. Resolves false when dismissed.
 */
@Injectable({ providedIn: 'root' })
export class Confirm {
  private readonly ports = inject(SHELL_PORTS);

  async ask(input: ConfirmInput): Promise<boolean> {
    return (await this.ports.openDialog(CONFIRM_DIALOG_ID, input)).kind === 'answered';
  }
}
