import { inject, Injectable, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell';
import { FakeCommandContext } from '../../testing/contributions';
import { provideFakeDock, provideTinyBackendBare } from '../../testing/helpers';
import { COMMAND_CONTEXT, CommandRegistry } from './command-registry';
import { WorkspaceStore } from '../core/workspace.store';
import { LAZY_COMMAND_SETS, lazyCommandFactory, lazyCommands, type CommandStub } from './lazy-registration';
import type { DebriefCommand } from './tokens';

const shown = signal(false);
const following = signal(false);
const runs: unknown[] = [];

@Injectable({ providedIn: 'root' })
class TestCommands {
  readonly commands: DebriefCommand[] = [
    { id: 't.open', title: 'Open thing', category: 'Test', keybinding: 'F8', run: (ctx, args) => void runs.push(['open', args, ctx.workspace.window]) },
    { id: 't.close', title: 'Close thing', when: () => shown(), run: () => void runs.push(['close']) },
    { id: 't.back', title: 'Back', keybinding: 'Alt+Left', when: () => true, run: () => void runs.push(['back']) },
  ];
}

/** A factory module's export: runs in the root injector, so it may inject. */
const followCommands = (): DebriefCommand[] => {
  const ctx = inject(COMMAND_CONTEXT);
  return [
    {
      id: 't.follow',
      get title() {
        return following() ? 'Stop following' : 'Follow';
      },
      category: 'Test',
      run: () => void runs.push(['follow', ctx.workspace.window]),
    },
  ];
};

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

  it('follows the loaded command’s title once the code is there (a toggle’s title changes)', async () => {
    const registry = setup();
    following.set(false);
    for (const work of idle) work();
    await registry.run('t.open');
    expect(registry.find('t.open')?.title).toBe('Open thing');
    TestBed.resetTestingModule();
    idle = [];
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => void idle.push(work) },
        ...lazyCommandFactory(async () => followCommands, [{ id: 't.follow', title: 'Follow', category: 'Test' }]),
      ],
    });
    const second = TestBed.inject(CommandRegistry);
    expect(second.find('t.follow')?.title).toBe('Follow');
    expect(await second.run('t.follow')).toBe(true);
    following.set(true);
    expect(second.find('t.follow')?.title).toBe('Stop following');
    expect(runs).toContainEqual(['follow', 'main']);
  });

  it('lists each lazy set with a loader that builds its commands, so one spec compares every stub with its code', async () => {
    setup();
    const sets = TestBed.inject(LAZY_COMMAND_SETS);
    expect(sets).toHaveLength(1);
    expect(sets[0]?.stubs).toBe(STUBS);
    expect((await sets[0]!.load()).map((c) => c.id)).toEqual(['t.open', 't.close', 't.back']);
  });

  it('lets the key of a gated stub fall through before its code loads, and starts the load', async () => {
    const registry = setup([...STUBS, { id: 't.back', title: 'Back', keybinding: 'Alt+Left', gated: true }]);
    const key = () => new KeyboardEvent('keydown', { key: 'ArrowLeft', code: 'ArrowLeft', altKey: true, cancelable: true });
    const first = key();
    expect(registry.runForKey(first)).toBe(false);
    expect(first.defaultPrevented).toBe(false);
    expect(loads).toBe(1);
    await vi.waitFor(() => expect(registry.find('t.back')).toBeDefined());
    // Loaded, and the class's `when` holds: the key now runs it.
    await vi.waitFor(() => expect(registry.runForKey(key())).toBe(true));
    await vi.waitFor(() => expect(runs).toContainEqual(['back']));
  });

  it('loads a set with a keyed gated stub as soon as its commands are read, not when the window is idle', async () => {
    const registry = setup([...STUBS, { id: 't.back', title: 'Back', keybinding: 'Alt+Left', gated: true }]);
    registry.all();
    await vi.waitFor(() => expect(loads).toBe(1));
    expect(idle.length).toBe(1);
    const key = new KeyboardEvent('keydown', { key: 'ArrowLeft', code: 'ArrowLeft', altKey: true, cancelable: true });
    await vi.waitFor(() => expect(registry.runForKey(key)).toBe(true));
  });

  it('answers false when the loaded command is not available after all', async () => {
    const registry = setup();
    expect(registry.available().map((c) => c.id)).toContain('t.close');
    expect(await registry.run('t.close')).toBe(false);
    expect(runs).toEqual([]);
  });

  it('shows a failed load as a failed run and loads again on the next run', async () => {
    let fail = true;
    loads = 0;
    idle = [];
    runs.length = 0;
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => void idle.push(work) },
        ...lazyCommands(async () => {
          loads++;
          if (fail) {
            fail = false;
            throw new Error('chunk missing');
          }
          return TestCommands;
        }, STUBS),
      ],
    });
    const registry = TestBed.inject(CommandRegistry);
    expect(await registry.run('t.open', 'auto', 1)).toBe(false);
    expect(TestBed.inject(WorkspaceStore).error()).toContain('chunk missing');
    expect(await registry.run('t.open', 'auto', 2)).toBe(true);
    expect(runs).toEqual([['open', 2, 'main']]);
    expect(loads).toBe(2);
  });
});
