/*
 * The dialog command recorder (ADR 0026 §3): every IPC command a dialog sends is recorded at the invoke layer, tagged
 * with the dialog's id. A command its contract does not declare is reported at once (the run-time guard in Rust refuses
 * it too); the commands seen across every run feed the nightly "declared but never sent" check
 * (`scripts/dialog-commands.mjs`). The ledger lives on `globalThis` so the boot harness, `shellInvoke` and an e2e
 * (`page.evaluate`) read the same one.
 */

/** What the recorder holds. */
export interface DialogCommandLedger {
  /** The dialog this window shows (a dialog window's route id), or null in a main window. */
  active: string | null;
  /** What the active dialog may call (its contract and the base sets); null: unknown, nothing is checked. */
  allowed: readonly string[] | null;
  /** The commands each dialog sent, by dialog id, each once. */
  readonly seen: Record<string, string[]>;
  /** `<dialog>: <command>` for every command a dialog sent that its contract does not declare. */
  readonly undeclared: string[];
}

const KEY = '__PADDOCK_DIALOG_COMMANDS__';

/** The window's ledger (created on first use). */
export function dialogCommandLedger(): DialogCommandLedger {
  const host = globalThis as { [KEY]?: DialogCommandLedger };
  host[KEY] ??= { active: null, allowed: null, seen: {}, undeclared: [] };
  return host[KEY];
}

/** Whether `command` is covered by a declared entry: an exact name, or a `prefix*`. */
function covered(declared: readonly string[], command: string): boolean {
  return declared.some((d) => (d.endsWith('*') ? command.startsWith(d.slice(0, -1)) : d === command));
}

/**
 * Records that dialog `dialog` sent `command` (plain name; other plugins' commands are theirs and not recorded). With
 * `allowed`, a command outside it is reported as undeclared; returns false then.
 */
export function recordDialogCommand(dialog: string, command: string, allowed: readonly string[] | null = null): boolean {
  if (command.startsWith('plugin:')) return true;
  const ledger = dialogCommandLedger();
  const list = (ledger.seen[dialog] ??= []);
  if (!list.includes(command)) list.push(command);
  if (allowed && !covered(allowed, command)) {
    const line = `${dialog}: ${command}`;
    if (!ledger.undeclared.includes(line)) ledger.undeclared.push(line);
    console.error(`[paddock-dialogs] dialog ${dialog} sent ${command}, which its contract does not declare`);
    return false;
  }
  return true;
}

/** Tags this window's commands with dialog `id` and what it may call (a dialog window's bootstrap). */
export function setActiveDialog(id: string | null, allowed: readonly string[] | null = null): void {
  const ledger = dialogCommandLedger();
  ledger.active = id;
  ledger.allowed = allowed;
}

/** Forgets everything recorded (between specs). */
export function resetDialogCommands(): void {
  const ledger = dialogCommandLedger();
  ledger.active = null;
  ledger.allowed = null;
  for (const k of Object.keys(ledger.seen)) delete ledger.seen[k];
  ledger.undeclared.length = 0;
}

/** The ledger's `seen` as the file the nightly check reads (`dialog-commands-seen.json`): ids and commands sorted. */
export function dialogCommandsSeenJson(): string {
  const seen = dialogCommandLedger().seen;
  const sorted = Object.fromEntries(
    Object.keys(seen)
      .sort()
      .map((id) => [id, [...(seen[id] ?? [])].sort()]),
  );
  return `${JSON.stringify({ version: 1, seen: sorted }, null, 2)}\n`;
}
