import { DestroyRef, inject, Injector, Provider, signal, Type } from '@angular/core';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell';
import { contributeFrom } from './contributions';
import type { CommandContext, DebriefCommand } from './tokens';

/**
 * What the palette, menus and keys need of a command before its code is loaded: everything but `when` and `run`. A
 * registration file (`*.contributions.ts`) lists these, so the command classes and the stores they inject stay out of
 * the main window's first paint (workbench.md §9, debrief-1-2-api.md §14.3 "lazy by construction").
 */
export type CommandStub = Omit<DebriefCommand, 'when' | 'run'>;

/** A class whose `commands` hold the `when` and `run` of the stubs (an injectable, created in the root injector). */
export type CommandsClass = Type<{ readonly commands: readonly DebriefCommand[] }>;

/**
 * Registers commands whose code loads on first use. The stubs are registered at once; the class loads when the window is
 * idle after start-up, or when one of its commands runs, whichever is first. Until then every stub is offered (its
 * `when` cannot be asked yet, as in the shell's `CommandService`); once loaded, each stub answers with its command's
 * `when`, and readers that track `when` (the registry's `available`) see the change. Running a stub whose id the class
 * does not define fails with that id: `lazyCommands.spec`-style checks keep stubs and classes equal.
 */
export function lazyCommands(load: () => Promise<CommandsClass>, stubs: readonly CommandStub[]): Provider[] {
  return contributeFrom('commands', () => {
    const injector = inject(Injector);
    const idle = inject(SHELL_IDLE);
    // A window (or a test) that closes before the idle load must not create classes in its destroyed injector.
    let alive = true;
    inject(DestroyRef).onDestroy(() => (alive = false));
    const loaded = signal<ReadonlyMap<string, DebriefCommand> | null>(null);
    let pending: Promise<ReadonlyMap<string, DebriefCommand>> | null = null;
    const ensure = (): Promise<ReadonlyMap<string, DebriefCommand>> =>
      (pending ??= load()
        .then((cls) => {
          if (!alive) throw new Error('the window closed before its commands loaded');
          const map = new Map(injector.get(cls).commands.map((c) => [c.id, c] as const));
          loaded.set(map);
          return map;
        })
        .catch((e: unknown) => {
          // A failed load (a missing chunk after an update) is retried on the next use.
          pending = null;
          throw e;
        }));
    idle(() => {
      if (alive) void ensure().catch(() => undefined);
    });
    return stubs.map(
      (stub): DebriefCommand => ({
        ...stub,
        when: (ctx: CommandContext) => {
          const map = loaded();
          const command = map?.get(stub.id);
          return map === null || (command !== undefined && (command.when?.(ctx) ?? true));
        },
        run: async (ctx: CommandContext, args?: unknown) => {
          const command = (await ensure()).get(stub.id);
          if (!command) throw new Error(`the command ${stub.id} is registered but its code does not define it`);
          // Asked again with the code loaded: a command that is not available now does nothing (as the registry does).
          if (command.when && !command.when(ctx)) return;
          await command.run(ctx, args);
        },
      }),
    );
  });
}
