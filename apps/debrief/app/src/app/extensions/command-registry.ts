import { computed, EnvironmentProviders, inject, Injectable, InjectionToken, makeEnvironmentProviders, signal } from '@angular/core';
import { SHELL_COMMAND_RUNNER, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { WorkspaceStore } from '../core/workspace.store';
import { Logger } from '../core/logger';
import { ActivityLog } from '../core/activity/activity-log';
import { Trigger } from '../core/activity/types';
import { HOST_COMMAND_FORWARD } from '../dialog-windows/host-commands';
import { ModalGate } from '../dialog-windows/modal-gate';
import { fuzzyFilter } from './fuzzy';
import { matchesKeybinding } from './keybinding';
import { applyOverrides } from './keybinding-overrides';
import { KeybindingOverridesStore } from './keybinding-overrides.store';
import { asDebriefCommand, contributedItems, provideDebriefKinds, type DebriefCommandDef } from './contributions';
import { CommandContext, DebriefCommand } from './tokens';
import { debriefCommandContext, debriefShellPorts } from './command-context';

/** The context handed to commands: `ShellContext<DebriefContext>`. Replaceable in tests. */
export const COMMAND_CONTEXT = new InjectionToken<CommandContext>('COMMAND_CONTEXT', {
  providedIn: 'root',
  factory: debriefCommandContext,
});

/**
 * All registered commands (built in and from plugins), their availability and execution. The commands are the shell
 * registry's `commands` (workbench.md §4.3 M1); this registry keeps what Debrief adds until the palette and menus move
 * (M3): the activity log entry per run, the modal gate, forwarding from dialog windows and the user's key bindings.
 */
@Injectable({ providedIn: 'root' })
export class CommandRegistry {
  private readonly contributed = contributedItems<DebriefCommandDef>('commands').map(asDebriefCommand);
  private readonly store = inject(WorkspaceStore);
  private readonly logger = inject(Logger);
  private readonly context = inject(COMMAND_CONTEXT);
  private readonly activity = inject(ActivityLog);
  private readonly overrides = inject(KeybindingOverridesStore);
  private readonly gate = inject(ModalGate);
  private readonly forward = inject(HOST_COMMAND_FORWARD);

  /** Commands once each, with the user's key bindings applied (a cleared binding has no `keybinding`). */
  private readonly effective = computed(() => {
    const seen = new Set<string>();
    return applyOverrides(
      this.contributed.filter((c) => !seen.has(c.id) && !!seen.add(c.id)),
      this.overrides.overrides(),
    );
  });

  /** While true (the shortcut editor is recording a key) key presses run no command. */
  readonly keysSuspended = signal(false);

  /** Commands whose `when` currently holds. Re-evaluated when the workspace changes. */
  readonly available = computed(() => {
    this.store.summary();
    return this.all().filter((c) => this.isAvailable(c));
  });

  /** Every command with its effective key binding. */
  all(): readonly DebriefCommand[] {
    return this.effective();
  }

  /** The binding the command declares itself, ignoring the user's overrides. */
  declaredKeybinding(id: string): string | undefined {
    return this.contributed.find((c) => c.id === id)?.keybinding;
  }

  find(id: string): DebriefCommand | undefined {
    return this.all().find((c) => c.id === id);
  }

  search(query: string): DebriefCommand[] {
    return fuzzyFilter(this.available(), query, (c) => (c.category ? `${c.category}: ${c.title}` : c.title));
  }

  /**
   * Runs a command by id. Failures are shown to the user, never thrown to the caller. Every run is one entry in
   * the activity log (`User <id> via <trigger>`); without a `trigger` it is inferred from the latest user input
   * (menu, palette, shortcut, context menu, button, native menu event), see `core/activity/trigger.ts`.
   */
  async run(id: string, trigger: Trigger | 'auto' = 'auto', args?: unknown): Promise<boolean> {
    // A dialog window hands commands to the window it belongs to.
    if (this.forward(id)) return true;
    const command = this.find(id);
    // A modal dialog window is open over this window: it answers first.
    if (this.gate.blocked() || !command || !this.isAvailable(command)) return false;
    try {
      await this.activity.track(id, trigger, { Title: command.title, Category: command.category }, () => command.run(this.context, args));
      return true;
    } catch (e) {
      this.logger.warn('ui.command', 'Command failed', { Command: id });
      this.store.setError($localize`:@@extensions.commandRegistry.failed:${command.title}:title: failed: ${e instanceof Error ? e.message : String(e)}:detail:`);
      return false;
    }
  }

  /** Runs the command bound to the key combination, if any. Returns true when one ran. */
  runForKey(e: KeyboardEvent): boolean {
    if (this.keysSuspended() || this.gate.blocked()) return false;
    const command = this.available().find((c) => c.keybinding && matchesKeybinding(c.keybinding, e));
    if (!command) return false;
    e.preventDefault();
    void this.run(command.id, 'shortcut');
    return true;
  }

  private isAvailable(c: DebriefCommand): boolean {
    try {
      return c.when ? c.when(this.context) : true;
    } catch {
      return false;
    }
  }
}

/**
 * The shell core in Debrief's main window: `ctx.runCommand` runs through this registry (one activity entry per run, the
 * modal gate) and the shell's notices reach Debrief's banner until M2 moves notifications. Registers Debrief's
 * contribution kinds once. In `app.config.ts`, after the features.
 */
export function provideDebriefShellCore(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideDebriefKinds(),
    {
      provide: SHELL_COMMAND_RUNNER,
      useFactory: () => {
        const registry = inject(CommandRegistry);
        return (id: string, args?: unknown) => registry.run(id, 'auto', args);
      },
    },
    { provide: SHELL_PORTS, useFactory: debriefShellPorts },
  ]);
}
