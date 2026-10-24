import { DIALOG_BASE_COMMANDS, dialogMinSize, type DialogContract, type DialogContractFile, type DialogDef } from '@decryp7/paddock-ui/shell';

/*
 * The contract file Rust reads (ADR 0009 §2): the registry is the source, `dialog-contracts.json` next to the app's
 * `tauri.conf.json` is its generated copy, parsed once by `paddock_dialogs::Contracts::parse`. An app's spec writes it in
 * write mode and compares it on every run, so a command added to a dialog shows up in review as a widening of what that
 * window may do.
 */

/** The conventional file name (`src-tauri/dialog-contracts.json`). */
export const DIALOG_CONTRACTS_FILE = 'dialog-contracts.json';

/** Plugin commands (`plugin:<name>|<command>`) are scoped by the plugin's own permissions, not by the dialog guard. */
const PLUGIN_COMMAND = /^plugin:/;

const byString = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
const sortedUnique = (list: readonly string[]): string[] => [...new Set(list)].sort(byString);

export interface DialogContractOptions {
  /**
   * The app's commands every dialog window may call besides the shell's base set (`DIALOG_BASE_COMMANDS`): what its
   * dialog bootstrap sends (logging, the theme, the start-up contract). `paddock-check-dialog-commands` checks them.
   */
  readonly base?: readonly string[];
}

/** The contract of one dialog as Rust reads it. */
export function dialogContract(def: DialogDef): DialogContract {
  return {
    kind: def.kind,
    modality: def.modality,
    requiredChoice: def.requiredChoice === true,
    instanced: def.instanced === true,
    minSize: dialogMinSize(def) ?? undefined,
    commands: sortedUnique(def.commands.filter((c) => !PLUGIN_COMMAND.test(c))),
  };
}

/** The contract file of `defs`: dialogs by id, every list sorted, so the checked-in copy has a stable diff. */
export function dialogContractFile(defs: readonly DialogDef[], options: DialogContractOptions = {}): DialogContractFile {
  const dialogs: Record<string, DialogContract> = {};
  for (const d of [...defs].sort((a, b) => byString(a.id, b.id))) dialogs[d.id] = dialogContract(d);
  const shell = new Set<string>(DIALOG_BASE_COMMANDS);
  return { version: 1, base: sortedUnique((options.base ?? []).filter((c) => !shell.has(c) && !PLUGIN_COMMAND.test(c))), dialogs };
}

/** The file's text: two-space JSON with a final newline. */
export function serializeDialogContracts(file: DialogContractFile): string {
  return `${JSON.stringify(file, null, 2)}\n`;
}

/** Whether `command` is covered by a declared entry: an exact name, or a `prefix*`. */
export function commandCovered(declared: readonly string[], command: string): boolean {
  return declared.some((d) => (d.endsWith('*') ? command.startsWith(d.slice(0, -1)) : d === command));
}

/** Everything a dialog's window may call: the shell's base set, the app's base set and its own list. */
export function allowedCommands(def: Pick<DialogDef, 'commands'>, base: readonly string[] = []): readonly string[] {
  return [...DIALOG_BASE_COMMANDS, ...base, ...def.commands];
}

