import { DestroyRef, inject, InjectionToken, Injector, Provider, runInInjectionContext, signal, Type } from '@angular/core';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell/contributions';
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

/** A function that builds the commands of the stubs; it runs in the root injector's injection context, so it may `inject()`. */
export type CommandsFactory = () => readonly DebriefCommand[];

/**
 * One lazy registration as `LAZY_COMMAND_SETS` lists it: its stubs and a loader that builds its commands in the root
 * injector. The stubs promise what the palette, menus and keys show; `lazy-commands.spec.ts` loads every set of the
 * real app and compares (`stubMismatches`).
 */
export interface LazyCommandSet {
  readonly stubs: readonly CommandStub[];
  load(): Promise<readonly DebriefCommand[]>;
}

/** Every lazy command registration of the window (for the spec that keeps stubs equal to their code). */
export const LAZY_COMMAND_SETS = new InjectionToken<readonly LazyCommandSet[]>('DEBRIEF_LAZY_COMMAND_SETS');

/**
 * Registers commands whose code loads on first use. The stubs are registered at once; the class loads when the window is
 * idle after start-up, or when one of its commands runs, whichever is first. Until then every stub is offered (its
 * `when` cannot be asked yet, as in the shell's `CommandService`); once loaded, each stub answers with its command's
 * `when`, title and category (a toggle's title changes), and readers that track `when` (the registry's `available`) see
 * the change. Running a stub whose id the class does not define fails with that id: `lazy-commands.spec.ts` keeps stubs
 * and classes equal.
 */
export function lazyCommands(load: () => Promise<CommandsClass>, stubs: readonly CommandStub[]): Provider[] {
  return lazyCommandSet(() => load().then((cls): CommandsFactory => () => inject(cls).commands), stubs);
}

/**
 * `lazyCommands` for a module that exports a factory instead of a class:
 * `lazyCommandFactory(() => import('./jobs.commands').then((m) => m.jobsCommandsFactory), JOBS_COMMAND_STUBS)`.
 */
export function lazyCommandFactory(load: () => Promise<CommandsFactory>, stubs: readonly CommandStub[]): Provider[] {
  return lazyCommandSet(load, stubs);
}

function lazyCommandSet(load: () => Promise<CommandsFactory>, stubs: readonly CommandStub[]): Provider[] {
  const build = async (injector: Injector): Promise<readonly DebriefCommand[]> => {
    const factory = await load();
    return runInInjectionContext(injector, factory);
  };
  return [
    {
      provide: LAZY_COMMAND_SETS,
      multi: true,
      useFactory: (): LazyCommandSet => {
        const injector = inject(Injector);
        return { stubs, load: () => build(injector) };
      },
    },
    ...contributeFrom('commands', () => {
      const injector = inject(Injector);
      const idle = inject(SHELL_IDLE);
      // A window (or a test) that closes before the idle load must not create classes in its destroyed injector.
      let alive = true;
      inject(DestroyRef).onDestroy(() => (alive = false));
      const loaded = signal<ReadonlyMap<string, DebriefCommand> | null>(null);
      let pending: Promise<ReadonlyMap<string, DebriefCommand>> | null = null;
      const ensure = (): Promise<ReadonlyMap<string, DebriefCommand>> =>
        (pending ??= load()
          .then((factory) => {
            if (!alive) throw new Error('the window closed before its commands loaded');
            const map = new Map(runInInjectionContext(injector, factory).map((c) => [c.id, c] as const));
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
      return stubs.map((stub): DebriefCommand => {
        const code = (): DebriefCommand | undefined => loaded()?.get(stub.id);
        return {
          ...stub,
          get title(): string {
            return code()?.title ?? stub.title;
          },
          get category(): string | undefined {
            const command = code();
            return command ? command.category : stub.category;
          },
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
        };
      });
    }),
  ];
}
