import { makeEnvironmentProviders, provideAppInitializer, type EnvironmentProviders } from '@angular/core';
import { SHELL_PLUGIN, SHELL_VERSION, type VersionReply } from '../generated/shell';
import { SHELL_COMMANDS, type ShellCommand } from '../generated/shell-commands';

/*
 * The shell's Rust side is one Tauri plugin (`paddock-shell`, ADR 0012 §3): its commands are sent as
 * `plugin:paddock-shell|<command>`. The shell's backends name their commands plainly (`health_snapshot`); an app hands
 * them `shellInvoke(invoke)`, which sends the shell's commands to the plugin and everything else (the app's own
 * commands, other plugins) as it is.
 */

/** Tauri's `invoke` (or any function of its shape). */
export type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** Whether `command` is one of the shell plugin's commands. */
export function isShellCommand(command: string): command is ShellCommand {
  return Object.prototype.hasOwnProperty.call(SHELL_COMMANDS, command);
}

/** The plugin's wire name of a shell command (`plugin:paddock-shell|health_snapshot`). */
export function shellCommand(command: ShellCommand): string {
  return `plugin:${SHELL_PLUGIN}|${command}`;
}

/** `invoke` with the shell's commands sent to the plugin; any other command goes as it is. */
export function shellInvoke(send: InvokeFn): InvokeFn {
  // A pass-through: the dialog command lint reads the command names at the backends' call sites, not here.
  return <T>(command: string, args?: Record<string, unknown>) => send<T>(isShellCommand(command) ? shellCommand(command) : command, args);
}

/** What is wrong with the plugin's `shell_version` reply for this package (null: the same version). */
export function shellVersionProblem(reply: unknown, expected: string = SHELL_VERSION): string | null {
  const shell = typeof reply === 'object' && reply !== null ? (reply as Partial<VersionReply>).shell : undefined;
  if (typeof shell !== 'string') return `the paddock-shell plugin did not say its version; this UI is @decryp7/paddock-ui ${expected}`;
  if (shell === expected) return null;
  return `the paddock-shell plugin is ${shell} but @decryp7/paddock-ui is ${expected}: bump both to the same version`;
}

/**
 * Asks the plugin for its version and compares it with this package's (ADR 0012 §3: one version across the boundary).
 * A mismatch, or a plugin that does not answer, is logged as a plain error and returned; it never stops the window.
 */
export async function checkShellVersion(invoke: InvokeFn, expected: string = SHELL_VERSION): Promise<string | null> {
  let reply: unknown;
  try {
    reply = await invoke<unknown>(shellCommand('shell_version'));
  } catch (e) {
    reply = null;
    console.error(`[paddock-shell] shell_version failed: ${e instanceof Error ? e.message : String(e)}`);
  }
  const problem = shellVersionProblem(reply, expected);
  if (problem) console.error(`[paddock-shell] ${problem}`);
  return problem;
}

/** Checks the plugin's version once at start-up, without holding the start-up (the frame paints first). */
export function provideShellVersionCheck(invoke: InvokeFn): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppInitializer(() => void checkShellVersion(invoke))]);
}
