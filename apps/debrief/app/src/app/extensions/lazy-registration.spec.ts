import { Injectable, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell';
import { FakeCommandContext } from '../../testing/contributions';
import { provideFakeDock, provideTinyBackendBare } from '../../testing/helpers';
import { COMMAND_CONTEXT, CommandRegistry } from './command-registry';
import { lazyCommands, type CommandStub } from './lazy-registration';
import type { DebriefCommand } from './tokens';

const shown = signal(false);
const runs: unknown[] = [];

@Injectable({ providedIn: 'root' })
class TestCommands {
  readonly commands: DebriefCommand[] = [
    { id: 't.open', title: 'Open thing', category: 'Test', keybinding: 'F8', run: (ctx, args) => void runs.push(['open', args, ctx.workspace.window]) },
    { id: 't.close', title: 'Close thing', when: () => shown(), run: () => void runs.push(['close']) },
  ];
}

const STUBS: readonly CommandStub[] = [
  { id: 't.open', title: 'Open thing', category: 'Test', keybinding: 'F8' },
  { id: 't.close', title: 'Close thing' },
];

describe('lazyCommands', () => {
  let loads: number;
  let idle: (() => void)[];

  function setup(stubs = STUBS) {
    loads = 0;
    idle = [];
    runs.length = 0;
    shown.set(false);
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => void idle.push(work) },
        ...lazyCommands(async () => {
          loads++;
          return TestCommands;
        }, stubs),
      ],
    });
    return TestBed.inject(CommandRegistry);
  }

  it('registers the stubs (titles, category, key) without loading the commands’ code', () => {
    const registry = setup();
    expect(registry.all().map((c) => [c.id, c.title, c.category, c.keybinding])).toEqual([
      ['t.open', 'Open thing', 'Test', 'F8'],
      ['t.close', 'Close thing', undefined, undefined],
    ]);
    expect(loads).toBe(0);
  });

  it('offers every stub until the code is loaded, then asks the loaded command’s when', async () => {
    const registry = setup();
    expect(registry.available().map((c) => c.id)).toEqual(['t.open', 't.close']);
    for (const work of idle) work();
    await TestBed.inject(CommandRegistry).run('t.open');
    expect(loads).toBe(1);
    expect(registry.available().map((c) => c.id)).toEqual(['t.open']);
    shown.set(true);
    expect(registry.available().map((c) => c.id)).toEqual(['t.open', 't.close']);
  });

  it('loads the code when the window is idle, once', async () => {
    setup();
    expect(idle).toHaveLength(1);
    idle[0]?.();
    idle[0]?.();
    await Promise.resolve();
    expect(loads).toBe(1);
  });

  it('runs the loaded command with the context and the arguments', async () => {
    const registry = setup();
    expect(await registry.run('t.open', 'auto', 42)).toBe(true);
    expect(runs).toEqual([['open', 42, 'main']]);
    expect(loads).toBe(1);
  });

  it('fails a stub its class does not define, naming the id, instead of running something else', async () => {
    const registry = setup([...STUBS, { id: 't.gone', title: 'Gone' }]);
    expect(await registry.run('t.gone')).toBe(false);
    expect(runs).toEqual([]);
  });
});
