import { DIALOG_BASE_COMMANDS, type DialogDef } from '@decryp7/paddock-ui/shell';

/*
 * Reading a dialog's contract (ADR 0026 §3). The contracts are Rust's (`paddock_dialogs::dialogs!`); TypeScript reads the
 * generated copy through `defineDialog` and never writes a contract file.
 */

/** Whether `command` is covered by a declared entry: an exact name, or a `prefix*`. */
export function commandCovered(declared: readonly string[], command: string): boolean {
  return declared.some((d) => (d.endsWith('*') ? command.startsWith(d.slice(0, -1)) : d === command));
}

/** Everything a dialog's window may call: the shell's base set, the app's base set and its own list. */
export function allowedCommands(def: Pick<DialogDef, 'commands'>, base: readonly string[] = []): readonly string[] {
  return [...DIALOG_BASE_COMMANDS, ...base, ...def.commands];
}
