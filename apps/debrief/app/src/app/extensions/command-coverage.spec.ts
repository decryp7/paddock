import { readdirSync, readFileSync } from 'node:fs';
import { ApplicationRef, EnvironmentInjector, Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { MemoryLogger, provideFakeDock, provideTinyBackendBare } from '../../testing/helpers';
import { FakeCommandContext } from '../../testing/contributions';
import { at } from '../../testing/must';
import { appConfig } from '../app.config';
import { Logger } from '../core/logger';
import { activity } from '../core/activity/runtime';
import { COMMAND_CONTEXT, CommandRegistry, provideDebriefShellCore } from './command-registry';
import { asDebriefCommand, debriefContributionProviders, type DebriefCommandDef } from './contributions';

/**
 * The command half of the activity coverage guard (debrief-1-2-api.md §14.3: M1 moves it to the shell's registry; the
 * backend and `invoke` halves stay in `core/activity/coverage-guard.spec.ts`).
 *
 * - every command in the shell's registry (all built-ins and plugins of the real app) produces exactly one activity entry
 *   when it runs, also when a contribution runs it through the shell (`ctx.runCommand`);
 * - nothing runs a command except Debrief's registry.
 */

const SRC = 'src/app';

/** Lets the app's start-up work finish before the test module is torn down: waits on Angular's pending tasks, not a clock. */
const settle = () => TestBed.inject(ApplicationRef).whenStable();

function sources(): { path: string; text: string }[] {
  return readdirSync(SRC, { recursive: true })
    .filter((p) => p.endsWith('.ts') && !p.endsWith('.spec.ts') && !p.includes('/testing/') && !p.includes('test-support') && !p.includes('test-setup'))
    .map((p) => ({ path: p.replaceAll('\\', '/'), text: readFileSync(`${SRC}/${p}`, 'utf8') }));
}

describe('command coverage guard (shell registry)', () => {
  beforeEach(() => activity.reset());

  it('every command in the shell registry logs exactly one entry when run, directly or through ctx.runCommand', async () => {
    // The real app's providers, with the logger captured; only the command list is read from them.
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    const real = TestBed.inject(ContributionRegistry).commands() as readonly DebriefCommandDef[];
    expect(real.length).toBeGreaterThan(50);

    // Run each through a registry of stubs (the guard is about the path, not about what commands do).
    const ran: string[] = [];
    const stubs = real.map((c) => ({ ...asDebriefCommand(c), when: undefined, run: () => void ran.push(c.id) }));
    const scope = Injector.create({
      providers: [{ provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() }, ...debriefContributionProviders({ commands: stubs }), CommandRegistry, provideDebriefShellCore() as never],
      parent: TestBed.inject(EnvironmentInjector),
    });
    const registry = scope.get(CommandRegistry);
    const logger = TestBed.inject(Logger) as MemoryLogger;
    const ids = [...new Set(stubs.map((c) => c.id))];
    const missing: string[] = [];
    for (const id of ids) {
      const before = logger.entries.filter((e) => e.category === 'Debrief.Activity').length;
      await registry.run(id);
      const after = logger.entries.filter((e) => e.category === 'Debrief.Activity');
      if (after.length !== before + 1 || at(after, after.length - 1).props?.['Operation'] !== id) missing.push(id);
    }
    expect(missing).toEqual([]);
    expect(ran.length).toBe(ids.length);
    await settle();
  });

  it('a command run through the shell (ctx.runCommand) is one activity entry too', async () => {
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: Logger, useFactory: () => new MemoryLogger() },
        ...debriefContributionProviders({ commands: [{ id: 't.via-shell', title: 'Via shell', run: () => undefined }] }),
        provideDebriefShellCore(),
      ],
    });
    const logger = TestBed.inject(Logger) as MemoryLogger;
    const core = TestBed.inject(ShellContextHost).context();
    expect(await core?.runCommand('t.via-shell')).toBe(true);
    const entries = logger.entries.filter((e) => e.category === 'Debrief.Activity');
    expect(entries.map((e) => e.props?.['Operation'])).toEqual(['t.via-shell']);
    await settle();
  });

  it('nothing runs a command except the registry', () => {
    const offenders = sources()
      // The registry, and the adapters it runs through: Debrief's command shape to the shell's, and a lazy stub to its
      // loaded command (each wraps `run` inside the `run` the registry calls).
      .filter((f) => !['extensions/command-registry.ts', 'extensions/contributions.ts', 'extensions/lazy-registration.ts'].includes(f.path))
      .filter(
        (f) =>
          /\b(command|cmd)\.run\(\s*(this\.)?(context|ctx)\b/.test(f.text) ||
          /(SHELL_CONTRIBUTIONS|ContributionRegistry|contributedItems)[\s\S]*\b(command|cmd|def)\.run\(\s*(this\.)?(context|ctx)\b/.test(f.text),
      )
      .map((f) => f.path);
    expect(offenders).toEqual([]);
  });
});
