import { EnvironmentInjector, Injectable, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CommandService, evaluate } from './commands';
import { ShellContext, ShellContextHost, SHELL_IDLE, SHELL_PORTS, ShellNotice, provideShellContext } from './context';
import { defineCommand, defineContributionKind, msg, provideContributionKinds, provideContributions } from '../public-api';
import { ContributionRegistry } from './registry';
import { loadPart } from './lazy';

interface DemoApp {
  readonly count: () => number;
  bump(): void;
}

@Injectable()
class CounterStore {
  readonly value = signal(0);
}

const notices: ShellNotice[] = [];
const ports = { showPage: vi.fn(), openDialog: vi.fn(() => Promise.resolve('answer')), notify: (n: ShellNotice) => void notices.push(n) };

function setup(...providers: unknown[]) {
  notices.length = 0;
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: SHELL_PORTS, useValue: ports }, ...(providers as never[])] });
}

const hello = defineCommand<DemoApp>({ id: 'sd.hello', title: msg('sd.hello', 'Hello'), run: (ctx) => ctx.bump() });

describe('ContributionRegistry', () => {
  it('keeps the first of duplicate ids and flattens batches in order', () => {
    const second = defineCommand({ id: 'sd.other', title: msg('sd.other', 'Other'), run: () => undefined });
    const dup = defineCommand({ id: 'sd.hello', title: msg('sd.hello2', 'Dup'), run: () => undefined });
    setup(provideContributions({ commands: [hello] }), provideContributions({ commands: [second, dup] }));
    const r = TestBed.inject(ContributionRegistry);
    expect(r.commands().map((c) => c.title.text)).toEqual(['Hello', 'Other']);
    expect(r.raw('commands')).toHaveLength(3);
    expect(r.find('commands', 'sd.other')).toBe(second);
    expect(r.find('nope', 'x')).toBeUndefined();
    expect(r.has('commands', 'sd.hello')).toBe(true);
    expect(r.has('nope', 'x')).toBe(false);
    for (const list of [r.keybindings(), r.menus(), r.pages(), r.statusItems(), r.settingsPages(), r.dialogs(), r.healthServices(), r.notificationKinds()]) expect(list).toEqual([]);
  });

  it('skips items that have no identity', () => {
    setup(provideContributions({ commands: [null as never, { get id(): string { throw new Error('x'); } } as never, hello] }));
    expect(TestBed.inject(ContributionRegistry).commands()).toEqual([hello]);
  });

  it('takes added kinds through the same mechanism and reports unknown and shadowed ones', () => {
    const views = defineContributionKind<{ id: string }>({ name: 'views', label: 'view', identity: (v) => v.id });
    const shadow = defineContributionKind<{ id: string }>({ name: 'commands', label: 'x', identity: (v) => v.id });
    setup(provideContributionKinds(views, shadow), provideContributions({ views: [{ id: 'wb.tree' }], typo: [{ id: 'x' }] } as never));
    const r = TestBed.inject(ContributionRegistry);
    expect(r.items('views')).toEqual([{ id: 'wb.tree' }]);
    expect(r.unknownKinds()).toEqual(['typo']);
    expect(r.shadowedKinds()).toEqual(['commands']);
  });
});

describe('ShellContextHost', () => {
  it('is complete at once when the app adds nothing', async () => {
    setup();
    const host = TestBed.inject(ShellContextHost);
    expect(host.context()).not.toBeNull();
    const ctx = await host.load();
    ctx.showPage('sd.home');
    expect(ports.showPage).toHaveBeenCalledWith('sd.home');
    await expect(ctx.openDialog('sd.ask')).resolves.toBe('answer');
    ctx.notify({ level: 'info', message: 'hi' });
    expect(notices).toEqual([{ level: 'info', message: 'hi' }]);
  });

  it('loads the app part on first use, in the injection context, once', async () => {
    const factory = vi.fn(() => {
      const store = TestBed.inject(CounterStore);
      return { count: () => store.value(), bump: () => store.value.update((v) => v + 1) } satisfies DemoApp;
    });
    const loader = vi.fn(() => Promise.resolve(factory));
    setup(CounterStore, provideShellContext<DemoApp>(loader, { prefetch: 'first-use' }));
    const host = TestBed.inject(ShellContextHost);
    expect(host.context()).toBeNull();
    expect(loader).not.toHaveBeenCalled();
    const [a, b] = await Promise.all([host.load(), host.load()]);
    expect(a).toBe(b);
    expect(loader).toHaveBeenCalledTimes(1);
    const ctx = a as ShellContext<DemoApp>;
    ctx.bump();
    expect(ctx.count()).toBe(1);
    expect('bump' in ctx && 'notify' in ctx).toBe(true);
    expect('missing' in ctx).toBe(false);
    expect(Reflect.set(ctx, 'count', 1)).toBe(false);
    expect(Reflect.defineProperty(ctx, 'x', { value: 1 })).toBe(false);
    expect(Reflect.deleteProperty(ctx, 'count')).toBe(false);
    expect(host.context()).toBe(ctx);
    await expect(host.load()).resolves.toBe(ctx);
  });

  it('prefers the shell part over an app member of the same name and keeps app getters working', async () => {
    class App {
      private n = 2;
      get doubled(): number {
        return this.n * 2;
      }
      notify(): void {
        throw new Error('app notify must not win');
      }
    }
    setup(provideShellContext(() => Promise.resolve(() => new App()), { prefetch: 'first-use' }));
    const ctx = (await TestBed.inject(ShellContextHost).load()) as ShellContext<App>;
    expect(ctx.doubled).toBe(4);
    ctx.notify({ level: 'info', message: 'shell' });
    expect(notices.at(-1)?.message).toBe('shell');
  });

  it('prefetches when idle and reports a failed load once, then retries on use', async () => {
    let idleWork: (() => void) | undefined;
    let fail = true;
    const loader = vi.fn(() => (fail ? Promise.reject(new Error('chunk missing')) : Promise.resolve(() => ({ ok: true }))));
    setup({ provide: SHELL_IDLE, useValue: (w: () => void) => (idleWork = w) }, provideShellContext(loader));
    const host = TestBed.inject(ShellContextHost);
    TestBed.inject(EnvironmentInjector); // runs the environment initializer
    expect(idleWork).toBeDefined();
    idleWork?.();
    await vi.waitFor(() => expect(notices.map((n) => n.message)).toEqual(['The app context failed to load: chunk missing']));
    fail = false;
    await expect(host.load()).resolves.toBeTruthy();
    expect(loader).toHaveBeenCalledTimes(2);
    host.prefetchWhenIdle(); // already loaded: nothing scheduled
  });

  it('uses requestIdleCallback when there is one, else a macrotask', async () => {
    setup();
    const idle = TestBed.inject(SHELL_IDLE);
    const ric = vi.fn((cb: () => void) => (cb(), 1));
    (globalThis as { requestIdleCallback?: unknown }).requestIdleCallback = ric;
    const work = vi.fn();
    idle(work);
    expect(ric).toHaveBeenCalled();
    delete (globalThis as { requestIdleCallback?: unknown }).requestIdleCallback;
    idle(work);
    await vi.waitFor(() => expect(work).toHaveBeenCalledTimes(2));
  });

  it('warns instead of failing when no runner or port is wired', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const ctx = await TestBed.inject(ShellContextHost).load();
    await expect(ctx.runCommand('sd.hello')).resolves.toBe(false);
    ctx.showPage('sd.home');
    await expect(ctx.openDialog('sd.ask')).resolves.toBeUndefined();
    ctx.notify({ level: 'warning', message: 'w' });
    ctx.notify({ level: 'error', message: 'e' });
    expect(warn).toHaveBeenCalledTimes(4);
    expect(error).toHaveBeenCalledTimes(1);
    warn.mockRestore();
    error.mockRestore();
  });
});

describe('CommandService', () => {
  function app(over: Partial<DemoApp> = {}) {
    let n = 0;
    return provideShellContext<DemoApp>(() => Promise.resolve(() => ({ count: () => n, bump: () => void n++, ...over })), { prefetch: 'first-use' });
  }

  it('runs a command with the loaded context, also through ctx.runCommand', async () => {
    setup(app(), provideContributions<DemoApp>({ commands: [hello] }));
    const commands = TestBed.inject(CommandService);
    expect(commands.available().map((c) => c.id)).toEqual(['sd.hello']);
    await expect(commands.run('sd.hello')).resolves.toBe(true);
    const ctx = (await TestBed.inject(ShellContextHost).load()) as ShellContext<DemoApp>;
    await expect(ctx.runCommand('sd.hello')).resolves.toBe(true);
    expect(ctx.count()).toBe(2);
    await expect(commands.run('sd.missing')).resolves.toBe(false);
  });

  it('asks when and enabled defensively and refuses unavailable commands', async () => {
    const hidden = defineCommand({ id: 'sd.hidden', title: msg('sd.hidden', 'Hidden'), when: () => false, run: vi.fn() });
    const throws = defineCommand({ id: 'sd.throws', title: msg('sd.throws', 'Throws'), when: () => { throw new Error('x'); }, run: vi.fn() });
    const off = defineCommand({ id: 'sd.off', title: msg('sd.off', 'Off'), enabled: () => false, checked: () => true, run: vi.fn() });
    const badEnabled = defineCommand({ id: 'sd.bad', title: msg('sd.bad', 'Bad'), enabled: () => { throw new Error('x'); }, run: vi.fn() });
    setup(app(), provideContributions({ commands: [hidden, throws, off, badEnabled] }));
    const commands = TestBed.inject(CommandService);
    // Before the context is loaded every command is offered.
    expect(commands.available()).toHaveLength(4);
    expect(commands.isEnabled(off)).toBe(true);
    await TestBed.inject(ShellContextHost).load();
    expect(commands.available().map((c) => c.id)).toEqual(['sd.off', 'sd.bad']);
    expect(commands.isEnabled(off)).toBe(false);
    expect(commands.isChecked(off)).toBe(true);
    expect(commands.isChecked(hidden)).toBe(false);
    for (const id of ['sd.hidden', 'sd.throws', 'sd.off', 'sd.bad']) await expect(commands.run(id)).resolves.toBe(false);
    for (const c of [hidden, throws, off, badEnabled]) expect(c.run).not.toHaveBeenCalled();
  });

  it('shows a failure as a notice and never throws', async () => {
    const boom = defineCommand({ id: 'sd.boom', title: msg('sd.boom', 'Boom'), run: () => Promise.reject(new Error('disk full')) });
    const thrown = defineCommand({ id: 'sd.thrown', title: msg('sd.thrown', 'Thrown'), run: () => { throw 'plain'; } });
    setup(app(), provideContributions({ commands: [boom, thrown] }));
    const commands = TestBed.inject(CommandService);
    await expect(commands.run('sd.boom')).resolves.toBe(false);
    await expect(commands.run('sd.thrown')).resolves.toBe(false);
    expect(notices).toEqual([
      { level: 'error', message: 'Boom failed: disk full' },
      { level: 'error', message: 'Thrown failed: plain' },
    ]);
  });

  it('reports a context that fails to load', async () => {
    setup(provideShellContext(() => Promise.reject(new Error('offline')), { prefetch: 'first-use' }), provideContributions({ commands: [hello as never] }));
    await expect(TestBed.inject(CommandService).run('sd.hello')).resolves.toBe(false);
    expect(notices).toEqual([{ level: 'error', message: 'Hello failed: offline' }]);
  });

  it('evaluate falls back when absent, unloaded or throwing', () => {
    const ctx = {} as ShellContext;
    expect(evaluate(undefined, ctx, 'd')).toBe('d');
    expect(evaluate(() => 'x', null, 'd')).toBe('d');
    expect(evaluate(() => { throw new Error('x'); }, ctx, 'd')).toBe('d');
    expect(evaluate(() => 'x', ctx, 'd')).toBe('x');
  });
});

describe('loadPart', () => {
  it('loads the component and gives it its own injector only when it declares providers', async () => {
    setup();
    const parent = TestBed.inject(EnvironmentInjector);
    class Comp {}
    const plain = await loadPart({ load: () => Promise.resolve(Comp) }, parent);
    expect(plain).toEqual({ component: Comp, injector: parent });
    const empty = await loadPart({ load: () => Promise.resolve(Comp), providers: () => Promise.resolve([]) }, parent);
    expect(empty.injector).toBe(parent);
    const withStore = await loadPart({ load: () => Promise.resolve(Comp), providers: () => Promise.resolve([CounterStore]) }, parent, 'sd.counter');
    expect(withStore.injector).not.toBe(parent);
    expect(withStore.injector.get(CounterStore).value()).toBe(0);
    expect(parent.get(CounterStore, null)).toBeNull();
    withStore.injector.destroy();
  });
});
