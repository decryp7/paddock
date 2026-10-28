import { ChangeDetectionStrategy, Component, input, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CommandService, ContributionRegistry, defineCommand, msg, ShellContextHost, SHELL_IDLE, SHELL_PORTS, answerOf } from '@decryp7/paddock-ui/shell';
import { WindowReady } from '@decryp7/paddock-ui/shell/startup';
import { FakeClock, ManualIdle, provideShellTesting, RecordingShellPorts, render, settle, shellIdle, shellPorts } from './shell-testing';

interface Demo {
  readonly count: ReturnType<typeof signal<number>>;
}

const GREET = defineCommand<Demo>({
  id: 't.greet',
  title: msg('t.greet', 'Greet'),
  run: (ctx) => {
    ctx.count.update((n) => n + 1);
    ctx.notify({ level: 'info', message: `Hello ${ctx.count()}` });
  },
});
const OPEN = defineCommand<Demo>({ id: 't.open', title: msg('t.open', 'Open'), run: async (ctx) => void ctx.notify({ level: 'info', message: String(answerOf(await ctx.openDialog('t.rename', { current: 'x' }))) }) });

@Component({ selector: 'pd-test-greeting', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p class="who">Hello {{ name() }}</p><p class="n">{{ n() }}</p><p class="n">x</p>` })
class Greeting {
  readonly name = input('nobody');
  readonly n = input(0);
}

describe('provideShellTesting', () => {
  it('registers the contributions, loads the context on first use and records what the commands asked', async () => {
    const count = signal(0);
    TestBed.configureTestingModule({ providers: provideShellTesting<Demo>({ contributions: [{ commands: [GREET, OPEN] }], context: () => Promise.resolve(() => ({ count })) }) });
    expect(TestBed.inject(ContributionRegistry).has('commands', 't.greet')).toBe(true);
    expect(TestBed.inject(SHELL_PORTS)).toBe(shellPorts());
    await expect(TestBed.inject(CommandService).run('t.greet')).resolves.toBe(true);
    expect(shellPorts().messages()).toEqual(['Hello 1']);
    shellPorts().dialogAnswers.set('t.rename', 'New');
    await TestBed.inject(CommandService).run('t.open');
    expect(shellPorts().dialogs).toEqual([{ id: 't.rename', inputs: { current: 'x' } }]);
    expect(shellPorts().messages().at(-1)).toBe('New');
    shellPorts().showPage('t.page');
    expect(shellPorts().pages).toEqual(['t.page']);
    expect(TestBed.inject(ShellContextHost).context()).not.toBeNull();
  });

  it('runs idle work by hand and marks the window ready unless asked not to', () => {
    TestBed.configureTestingModule({ providers: provideShellTesting() });
    const ran: string[] = [];
    TestBed.inject(SHELL_IDLE)(() => {
      ran.push('a');
      TestBed.inject(SHELL_IDLE)(() => ran.push('b'));
    });
    expect(shellIdle().pending).toBe(1);
    expect(ran).toEqual([]);
    shellIdle().flush();
    expect(ran).toEqual(['a', 'b']);
    expect(TestBed.inject(WindowReady).ready()).toBe(true);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: provideShellTesting({ ready: false, providers: [{ provide: 'extra', useValue: 1 }] }) });
    expect(TestBed.inject(WindowReady).ready()).toBe(false);
    expect(TestBed.inject('extra' as never)).toBe(1);
    expect(TestBed.inject(ManualIdle)).toBe(shellIdle());
    expect(TestBed.inject(RecordingShellPorts)).toBe(shellPorts());
  });
});

describe('FakeClock', () => {
  it('moves by hand and fires each tick as often as it fell due, until stopped', () => {
    const clock = new FakeClock(0);
    const ticks: number[] = [];
    const stop = clock.every(100, () => ticks.push(clock.now()));
    clock.advance(250);
    expect(ticks).toEqual([100, 200]);
    expect(clock.now()).toBe(250);
    expect(clock.monotonic()).toBe(250);
    expect(clock.ticking).toBe(1);
    expect(clock.periods()).toEqual([100]);
    stop();
    clock.advance(1000);
    expect(ticks).toHaveLength(2);
    expect(new FakeClock().now()).toBe(1_000_000);
  });

  it('fires a one-shot once, in time order with the ticks, and a wall change fires nothing', () => {
    const clock = new FakeClock(5_000);
    const seen: string[] = [];
    clock.every(40, () => seen.push(`tick@${clock.monotonic()}`));
    clock.after(50, () => seen.push(`once@${clock.monotonic()}`));
    const cancelled = clock.after(10, () => seen.push('never'));
    cancelled();
    clock.advance(100);
    expect(seen).toEqual(['tick@40', 'once@50', 'tick@80']);
    clock.setWall(0);
    expect(clock.now()).toBe(0);
    expect(clock.monotonic()).toBe(100);
    expect(seen).toHaveLength(3);
    clock.advance(-5);
    expect(clock.monotonic()).toBe(100);
  });
});

describe('render and settle', () => {
  it('renders with inputs, queries and sets inputs again, all settled on whenStable', async () => {
    TestBed.configureTestingModule({ providers: provideShellTesting() });
    const r = await render(Greeting, { name: 'Ada' });
    expect(r.one('.who').textContent).toBe('Hello Ada');
    expect(r.all('.n')).toHaveLength(2);
    await r.set({ n: 3 });
    expect(r.one('.n').textContent).toBe('3');
    expect(r.component).toBeInstanceOf(Greeting);
    expect(() => r.one('.missing')).toThrow(/^no .missing in /);
    let done = false;
    void Promise.resolve().then(() => (done = true));
    await settle();
    expect(done).toBe(true);
  });
});
