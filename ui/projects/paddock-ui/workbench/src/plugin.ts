import { WORKBENCH_PLUGIN } from '../generated/workbench';
import { WORKBENCH_COMMANDS, type WorkbenchCommand } from '../generated/workbench-commands';

/*
 * The workbench's Rust side is one Tauri plugin (`paddock-workbench`, plan X50 W7): its commands (the layout store, the
 * pop-out panels and the assistant) are sent as `plugin:paddock-workbench|<command>`. The workbench's backends name their
 * commands plainly (`panel_pop_out`, `assistant_send`); an app hands them `workbenchInvoke(invoke)`, which sends the
 * plugin's commands to the plugin and everything else (the shell's, the app's own, other plugins) as it is. Compose it with
 * the shell's: `workbenchInvoke(shellInvoke(invoke))`.
 */

/** Tauri's `invoke` (or any function of its shape). */
export type WorkbenchInvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** Whether `command` is one of the workbench plugin's commands. */
export function isWorkbenchCommand(command: string): command is WorkbenchCommand {
  return Object.prototype.hasOwnProperty.call(WORKBENCH_COMMANDS, command);
}

/** The plugin's wire name of a workbench command (`plugin:paddock-workbench|panel_pop_out`). */
export function workbenchCommand(command: WorkbenchCommand): string {
  return `plugin:${WORKBENCH_PLUGIN}|${command}`;
}

/** `invoke` with the workbench's commands sent to the plugin; any other command goes as it is. */
export function workbenchInvoke(send: WorkbenchInvokeFn): WorkbenchInvokeFn {
  return <T>(command: string, args?: Record<string, unknown>) => send<T>(isWorkbenchCommand(command) ? workbenchCommand(command) : command, args);
}
