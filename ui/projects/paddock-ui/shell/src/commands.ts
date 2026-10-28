import { providePaddockIcons } from '@decryp7/paddock-ui/icons';
import { computed, EnvironmentProviders, inject, Injectable, makeEnvironmentProviders, Signal } from '@angular/core';
import { describe, SHELL_COMMAND_RUNNER, SHELL_PORTS, ShellContext, ShellContextHost } from './context';
import type { CommandDef, Live } from './define';
import { ContributionRegistry } from './registry';

/**
 * Evaluates a live function defensively: `fallback` when it is absent, when the context is not loaded yet, or when it
 * throws (a throwing `when` hides that one item, never the menu or the bar).
 */
export function evaluate<T extends object, R>(fn: Live<T, R> | undefined, ctx: ShellContext<T> | null, fallback: R): R {
  if (!fn || !ctx) return fallback;
  try {
    return fn(ctx);
  } catch {
    return fallback;
  }
}

/** A condition that defaults to true: absent, or not askable yet (context not loaded), it holds; throwing, it does not. */
function holds<T extends object>(fn: Live<T, boolean> | undefined, ctx: ShellContext<T> | null): boolean {
  return !fn || evaluate(fn, ctx, ctx === null);
}

/** Runs contributed commands and answers which are available (for menus, palette and keys of later packages). */
@Injectable({ providedIn: 'root' })
export class CommandService {
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);
  private readonly ports = inject(SHELL_PORTS);

  /**
   * Commands whose `when` holds. Until the app's context is loaded every command is offered (their `when` cannot be
   * asked yet); running one loads the context and asks again.
   */
  readonly available: Signal<readonly CommandDef[]> = computed(() => {
    const ctx = this.host.context();
    return this.registry.commands().filter((c) => holds(c.when, ctx));
  });

  find(id: string): CommandDef | undefined {
    return this.registry.find<CommandDef>('commands', id);
  }

  /** Whether a command is shown enabled now (`enabled`, default true; a throwing `enabled` disables it). */
  isEnabled(command: CommandDef): boolean {
    return holds(command.enabled, this.host.context());
  }

  /** Whether a command shows a check mark now. */
  isChecked(command: CommandDef): boolean {
    return evaluate(command.checked, this.host.context(), false);
  }

  /**
   * Runs a command by id. Resolves true when it ran; false when it is unknown, unavailable, disabled or failed. A
   * failure is shown to the user as a notice, never thrown to the caller.
   */
  async run(id: string, args?: unknown): Promise<boolean> {
    const command = this.find(id);
    if (!command) return false;
    let ctx: ShellContext;
    try {
      ctx = await this.host.load();
    } catch (e) {
      this.ports.notify({ level: 'error', message: `${command.title.text} failed: ${describe(e)}` });
      return false;
    }
    // Asked again with the loaded context; a throwing `when` or `enabled` makes the command unavailable.
    if (!holds(command.when, ctx) || !holds(command.enabled, ctx)) return false;
    try {
      await command.run(ctx, args);
      return true;
    } catch (e) {
      this.ports.notify({ level: 'error', message: `${command.title.text} failed: ${describe(e)}` });
      return false;
    }
  }
}

/**
 * The shell core of a window: wires `ShellCore.runCommand` to the command service. Every window's bootstrap includes it
 * (later through `provideShellMain` / `provideShellDialog`, plan S6/S14).
 */
export function provideShellCore(): EnvironmentProviders {
  return makeEnvironmentProviders([
    // The `pd:` icons of the frame, in every window (main, dialog, pop-out).
    providePaddockIcons(),
    {
      provide: SHELL_COMMAND_RUNNER,
      useFactory: () => {
        const commands = inject(CommandService);
        return (id: string, args?: unknown) => commands.run(id, args);
      },
    },
  ]);
}
