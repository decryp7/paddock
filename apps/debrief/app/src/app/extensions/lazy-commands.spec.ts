import { readdirSync, readFileSync } from 'node:fs';
import { TestBed } from '@angular/core/testing';
import { stubMismatches } from '../../testing/contributions';
import { MemoryLogger } from '../../testing/helpers';
import { appConfig } from '../app.config';
import { Logger } from '../core/logger';
import { LAZY_COMMAND_SETS } from './lazy-registration';

/**
 * Commands lazy by construction (workbench.md §9, debrief-1-2-api.md §14.3): every `lazyCommands` registration of the real
 * app is loaded here and its stubs compared with the commands its code builds (id, title, category, key, view, in order),
 * so a stub can never promise what its class does not do. `lazy-registration.spec.ts` tests the mechanism.
 */
describe('lazy command registrations of the real app', () => {
  it('each set’s stubs equal the commands its code builds', async () => {
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    const sets = TestBed.inject(LAZY_COMMAND_SETS);
    expect(sets.length).toBeGreaterThan(41);
    const problems: string[] = [];
    for (const set of sets) {
      const where = set.stubs.map((s) => s.id).join(', ');
      try {
        problems.push(...stubMismatches(set.stubs, await set.load()).map((p) => `[${where}] ${p}`));
      } catch (e) {
        problems.push(`[${where}] its code did not load: ${e instanceof Error ? e.message : String(e)}`);
      }
    }
    expect(problems).toEqual([]);
  });
});

/**
 * Files that still register commands eagerly (`contributeFrom('commands', …)`: the class is imported by the registration
 * and lands in the first paint). Each entry says why its commands cannot be stubs. This list only shrinks: a new eager
 * registration fails, and so does an entry whose file no longer has one (delete the entry).
 */
const EAGER_COMMAND_FILES: Readonly<Record<string, string>> = {
  'builtin/provide-builtins.ts': 'BuiltinCommands builds one "Open <view>" command per registered view (built in and plugins): its ids and count depend on the registry at run time',
  'undo/provide-undo.ts': 'UndoCommands is not constructed elsewhere and its constructor starts an effect that shows UndoStack failures (UndoStack.execute, called by stores that never run these commands); that must not wait for idle',
};

describe('eager command registrations', () => {
  it('are only the listed ones', () => {
    const SRC = 'src/app';
    const eager = readdirSync(SRC, { recursive: true })
      .map((p) => String(p).replaceAll('\\', '/'))
      .filter((p) => p.endsWith('.ts') && !p.endsWith('.spec.ts') && p !== 'extensions/lazy-registration.ts')
      .filter((p) => /contributeFrom\(\s*'commands'/.test(readFileSync(`${SRC}/${p}`, 'utf8')))
      .sort();
    expect(eager).toEqual(Object.keys(EAGER_COMMAND_FILES).sort());
  });
});
