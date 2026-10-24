import { Injectable, signal } from '@angular/core';
import type { CommandRefusal } from '@decryp7/paddock-ui/shell';

/** Whether `e` is the command guard's refusal of a command this window may not call (`paddock-commands`). */
export function isDenial(e: unknown): e is CommandRefusal {
  if (typeof e !== 'object' || e === null) return false;
  const r = e as Partial<CommandRefusal>;
  return r.code === 'denied' && typeof r.command === 'string' && typeof r.message === 'string';
}

/**
 * The commands Rust refused in this window. A dialog calling a command its contract does not declare is a bug of the
 * contract, so the dialog shell shows it (the dialog's own error handling must not be the only place it shows). The app's
 * IPC wrapper reports every failed call here (`report` ignores anything that is not a denial).
 */
@Injectable({ providedIn: 'root' })
export class DeniedCommands {
  private readonly lastDenial = signal<CommandRefusal | null>(null);
  readonly last = this.lastDenial.asReadonly();

  /** Records `e` when it is a denial; returns whether it was. */
  report(e: unknown): boolean {
    if (!isDenial(e)) return false;
    console.error(`[paddock-dialogs] ${e.command} was refused${e.dialog ? ` in dialog ${e.dialog}` : ''}: ${e.message}`);
    this.lastDenial.set(e);
    return true;
  }
}
