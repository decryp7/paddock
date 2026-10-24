import { Injector, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideContributions, SHELL_IDLE, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { SETUP_EVENT, type ItemView, type SetupState } from '../generated/setup';
import { SETUP_COMMANDS, type SetupCommand } from '../generated/setup-commands';
import { demoSetupItems, memoryItem, MemorySetupBackend, SetupBackend, TauriSetupBackend, type MemorySetupOptions, type SetupIpc } from './backend';
import { provideShellSetup } from './provide-setup';
import { defineSetup, SETUP_DIALOG_COMMANDS, SETUP_DIALOG_ID, SETUP_SETTINGS_PAGE, SETUP_CONTRIBUTIONS } from './setup.contributions';
import { SetupStore } from './setup.store';
import { statusView } from './status-view';
import { asSetupError, parseSetupState, setupMessage } from './wire';

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

/** The engine's steps wait until `next()`/`all()`: a test sees each phase. */
function held(options: MemorySetupOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemorySetupBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) await this.next();
      await settle();
    },
  };
}

function storeBed(backend: SetupBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellSetup({ backend: () => backend })] });
  return TestBed.inject(SetupStore);
}

function state(items: ItemView[] = demoSetupItems(), over: Partial<SetupState> = {}): SetupState {
  return { items, finished: false, showOnStart: true, ...over };
}

describe('the wire', () => {
  it('accepts a state and rebuilds it from known fields', () => {
    const s = state();
    expect(parseSetupState({ ...s, extra: 1 })).toEqual(s);
    const withHint = state([memoryItem('a', 'A', 'd', 'Go', { hint: 'Compare 123456', message: 'm' })]);
    expect(parseSetupState(withHint)).toEqual(withHint);
    const { message: _m, hint: _h, needsAi: _n, ...bare } = memoryItem('a', 'A', 'd', 'Go');
    expect(parseSetupState({ items: [bare], finished: false, showOnStart: false })?.items[0]).toMatchObject({ message: null, hint: null, needsAi: null });
  });

  it('drops anything that is not a state', () => {
    const item = memoryItem('a', 'A', 'd', 'Go');
    const bad: unknown[] = [
      null,
      [],
      'x',
      {},
      { items: 'x', finished: false, showOnStart: false },
      { items: [], finished: 'no', showOnStart: false },
      { items: [], finished: false, showOnStart: 1 },
      { items: [null], finished: false, showOnStart: false },
      { items: [{ ...item, id: 1 }], finished: false, showOnStart: false },
      { items: [{ ...item, title: 1 }], finished: false, showOnStart: false },
      { items: [{ ...item, status: 'exploded' }], finished: false, showOnStart: false },
      { items: [{ ...item, status: 3 }], finished: false, showOnStart: false },
      { items: [{ ...item, skipped: 'y' }], finished: false, showOnStart: false },
      { items: [{ ...item, working: null }], finished: false, showOnStart: false },
      { items: [{ ...item, message: 3 }], finished: false, showOnStart: false },
      { items: [{ ...item, hint: 3 }], finished: false, showOnStart: false },
      { items: [{ ...item, needsAi: 'x' }], finished: false, showOnStart: false },
    ];
    for (const b of bad) expect(parseSetupState(b)).toBeNull();
  });

  it('reads refusals and makes a message from anything', () => {
    expect(asSetupError({ code: 'busy', message: 'Wait.', retryable: true })).toEqual({ code: 'busy', message: 'Wait.', retryable: true });
    expect(asSetupError({ code: 'busy', message: 'Wait.' })).toEqual({ code: 'busy', message: 'Wait.', retryable: false });
    for (const bad of [null, { code: 'nope', message: 'x' }, { code: 'busy' }, { code: 1, message: 'x' }, new Error('x')]) expect(asSetupError(bad)).toBeNull();
    expect(setupMessage({ code: 'busy', message: 'Wait.' }, 'f')).toBe('Wait.');
    expect(setupMessage(new Error('boom'), 'f')).toBe('boom');
    expect(setupMessage('plain', 'f')).toBe('plain');
    expect(setupMessage('', 'f')).toBe('f');
    expect(setupMessage(undefined, 'f')).toBe('f');
  });

  it('gives every status a word and a role', () => {
    expect(statusView({ status: 'done', skipped: false, working: false })).toMatchObject({ status: 'success' });
    expect(statusView({ status: 'problem', skipped: false, working: false })).toMatchObject({ status: 'danger' });
    expect(statusView({ status: 'todo', skipped: false, working: false })).toMatchObject({ status: 'warning' });
    expect(statusView({ status: 'todo', skipped: true, working: false })).toMatchObject({ status: 'neutral', label: 'Skipped' });
    expect(statusView({ status: 'todo', skipped: false, working: true })).toMatchObject({ status: 'info', label: 'Working' });
  });
});

describe('the Tauri backend', () => {
  function ipc(answers: Partial<Record<SetupCommand, unknown>>) {
    const calls: [SetupCommand, Record<string, unknown> | undefined][] = [];
    let handler: ((p: unknown) => void) | undefined;
    const port: SetupIpc = {
      invoke: <T>(command: SetupCommand, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(answers[command] as T);
      },
      listen: (event, h) => {
        expect(event).toBe(SETUP_EVENT);
        handler = h;
        return Promise.resolve(() => (handler = undefined));
      },
    };
    return { port, calls, emit: (p: unknown) => handler?.(p), listening: () => handler !== undefined };
  }

  it('calls the commands of the table with the step’s id and parses what comes back', async () => {
    const s = state();
    const { port, calls } = ipc({ setup_state: s, setup_skip: s, setup_finish: s });
    const backend = new TauriSetupBackend(port);
    expect(await backend.state()).toEqual(s);
    await backend.act('service');
    await backend.repair('service');
    expect(await backend.skip('login', true)).toEqual(s);
    expect(await backend.finish()).toEqual(s);
    expect(calls).toEqual([
      ['setup_state', undefined],
      ['setup_act', { id: 'service' }],
      ['setup_repair', { id: 'service' }],
      ['setup_skip', { id: 'login', skipped: true }],
      ['setup_finish', undefined],
    ]);
    expect(new Set(calls.map(([c]) => c))).toEqual(new Set(Object.keys(SETUP_COMMANDS)));
  });

  it('refuses a state the engine should not have sent and passes only valid events on', async () => {
    const { port, emit } = ipc({ setup_state: { nope: 1 } });
    const backend = new TauriSetupBackend(port);
    await expect(backend.state()).rejects.toThrow('no valid set-up state');
    const heard: SetupState[] = [];
    const off = await backend.listen((x) => heard.push(x));
    emit({ nope: 1 });
    emit(state());
    expect(heard).toHaveLength(1);
    off();
  });
});

describe('the in-memory engine', () => {
  it('walks a step through working to done, and the state says when the checklist opens by itself', async () => {
    const e = held();
    const states: SetupState[] = [];
    await e.memory.listen((s) => states.push(s));
    expect((await e.memory.state()).showOnStart).toBe(true);
    await e.memory.act('service');
    await e.next();
    expect(states.at(-1)?.items[0]).toMatchObject({ id: 'service', working: true, status: 'todo' });
    await e.next();
    expect(states.at(-1)?.items[0]).toMatchObject({ working: false, status: 'done' });
    // A second act while one runs is refused as busy; an unknown id as unknown.
    await e.memory.act('login');
    await expect(e.memory.act('login')).rejects.toMatchObject({ code: 'busy', retryable: true });
    await expect(e.memory.act('zz')).rejects.toMatchObject({ code: 'unknown' });
    await expect(e.memory.repair('zz')).rejects.toMatchObject({ code: 'unknown' });
    await expect(e.memory.skip('zz', true)).rejects.toMatchObject({ code: 'unknown' });
    await e.all();
  });

  it('fails a step with a message that leaves Repair, hangs one on request and fails the first asks', async () => {
    const e = held({ outcomes: { notifications: 'fail', pitwall: 'hang' }, stateFailures: 1 });
    await expect(e.memory.state()).rejects.toThrow('could not be asked');
    await e.memory.act('notifications');
    await e.all();
    const after = await e.memory.state();
    expect(after.items.find((i) => i.id === 'notifications')).toMatchObject({ status: 'problem', working: false });
    expect(after.items.find((i) => i.id === 'notifications')?.message).toContain('system said no');
    await e.memory.act('pitwall');
    await e.all();
    expect((await e.memory.state()).items.find((i) => i.id === 'pitwall')?.working).toBe(true);
    // Repair works as the engine's repair does.
    await e.memory.repair('notifications');
    await e.all();
    expect((await e.memory.state()).items.find((i) => i.id === 'notifications')?.status).toBe('done');
  });

  it('does not open by itself once everything is done, skipped or finished', async () => {
    const m = new MemorySetupBackend({ items: [memoryItem('a', 'A', 'd', 'Go')] });
    expect((await m.skip('a', true)).showOnStart).toBe(false);
    expect((await m.skip('a', false)).showOnStart).toBe(true);
    const finished = await m.finish();
    expect(finished).toMatchObject({ finished: true, showOnStart: false });
    expect((await new MemorySetupBackend({ finished: true }).state()).showOnStart).toBe(false);
    const off = await m.listen(() => undefined);
    off();
  });
});

describe('the store', () => {
  it('shows the first look as loading, then the steps, and folds the events in', async () => {
    const e = held();
    const store = storeBed(e.memory);
    expect(store.requestStatus()).toBe('loading');
    await settle();
    expect(store.items().map((i) => i.id)).toEqual(['service', 'login', 'notifications', 'pitwall']);
    expect(store.open()).toHaveLength(4);
    expect(store.allSettled()).toBe(false);
    expect(store.showOnStart()).toBe(true);

    await store.act('service');
    expect(store.isWorking('service')).toBe(true);
    await e.next();
    expect(store.isWorking('service')).toBe(true);
    await e.next();
    expect(store.isWorking('service')).toBe(false);
    expect(store.items()[0]?.status).toBe('done');
    expect(store.open()).toHaveLength(3);
  });

  it('skips and takes up again, and finishes', async () => {
    const store = storeBed(new MemorySetupBackend());
    await settle();
    for (const id of ['service', 'login', 'notifications', 'pitwall']) await store.skip(id, true);
    expect(store.allSettled()).toBe(true);
    expect(store.open()).toHaveLength(0);
    await store.skip('login', false);
    expect(store.open().map((i) => i.id)).toEqual(['login']);
    await store.finish();
    expect(store.finished()).toBe(true);
  });

  it('keeps a failed step on screen with its message and Repair runs it', async () => {
    const e = held({ outcomes: { service: 'fail' } });
    const store = storeBed(e.memory);
    await settle();
    await store.act('service');
    await e.all();
    expect(store.items()[0]).toMatchObject({ status: 'problem' });
    await store.repair('service');
    await e.all();
    expect(store.items()[0]?.status).toBe('done');
  });

  it('shows a refused request as a failure and Retry runs the same request again', async () => {
    const e = held();
    const store = storeBed(e.memory);
    await settle();
    await store.act('zz');
    expect(store.failure()).toMatchObject({ id: 'zz', action: 'act', error: { code: 'unknown' } });
    expect(store.isWorking('zz')).toBe(false);
    await store.retry();
    expect(store.failure()?.action).toBe('act');
    store.dismissFailure();
    expect(store.failure()).toBeNull();
    await store.retry();
    // A failed skip is retried as a skip; a failed finish as a finish.
    vi.spyOn(e.memory, 'skip').mockRejectedValueOnce({ code: 'notSaved', message: 'x', retryable: true });
    await store.skip('login', true);
    expect(store.failure()).toMatchObject({ id: 'login', action: 'skip' });
    await store.retry();
    expect(store.failure()).toBeNull();
    expect(store.items().find((i) => i.id === 'login')?.skipped).toBe(true);
    vi.spyOn(e.memory, 'finish').mockRejectedValueOnce(new Error('disk'));
    await store.finish();
    expect(store.failure()?.error.message).toBe('disk');
    await store.retry();
    expect(store.finished()).toBe(true);
  });

  it('does not retry what the engine says cannot be retried', async () => {
    const m = new MemorySetupBackend();
    const store = storeBed(m);
    await settle();
    const act = vi.spyOn(m, 'act').mockRejectedValueOnce({ code: 'failed', message: 'no', retryable: false });
    await store.act('service');
    expect(act).toHaveBeenCalledTimes(1);
    await store.retry();
    expect(act).toHaveBeenCalledTimes(1);
  });

  it('ignores a second ask for a step that is already requested', async () => {
    const e = held();
    const store = storeBed(e.memory);
    await settle();
    const act = vi.spyOn(e.memory, 'act');
    await store.act('login');
    await store.act('login');
    expect(act).toHaveBeenCalledTimes(1);
    await e.all();
  });

  it('looks at the state again when the engine says a step is busy (another window started it)', async () => {
    const m = new MemorySetupBackend();
    const store = storeBed(m);
    await settle();
    const state = vi.spyOn(m, 'state');
    vi.spyOn(m, 'act').mockRejectedValueOnce({ code: 'busy', message: 'running', retryable: true });
    await store.act('service');
    expect(store.failure()?.error.code).toBe('busy');
    expect(state).toHaveBeenCalledTimes(1);
  });

  it('shows a failed first ask as an inline error that Retry asks again', async () => {
    const store = storeBed(new MemorySetupBackend({ stateFailures: 1 }));
    await settle();
    expect(store.requestError()).toContain('could not be asked');
    expect(store.setup()).toBeNull();
    await store.load();
    expect(store.items()).toHaveLength(4);
    // A refresh that fails keeps the steps on screen.
    expect(store.requestError()).toBeNull();
  });

  it('stops listening when the window closes, and when it closes before the listener is ready', async () => {
    let unlisten = 0;
    class Slow extends MemorySetupBackend {
      override async listen(h: (s: SetupState) => void) {
        const off = await super.listen(h);
        return () => {
          unlisten++;
          off();
        };
      }
    }
    storeBed(new Slow());
    await settle();
    TestBed.resetTestingModule();
    expect(unlisten).toBe(1);

    class Late extends MemorySetupBackend {
      override listen(h: (s: SetupState) => void) {
        return new Promise<() => void>((resolve) => void Promise.resolve().then(() => super.listen(h)).then((off) => resolve(() => (unlisten++, off()))));
      }
    }
    storeBed(new Late());
    TestBed.resetTestingModule();
    await settle();
    expect(unlisten).toBe(2);

    class Broken extends MemorySetupBackend {
      override listen(): Promise<() => void> {
        return Promise.reject(new Error('no events'));
      }
    }
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    storeBed(new Broken());
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });

  it('falls back to a plain refusal for an error that is not the engine’s', async () => {
    const m = new MemorySetupBackend();
    const store = storeBed(m);
    await settle();
    vi.spyOn(m, 'repair').mockRejectedValueOnce(undefined);
    await store.repair('service');
    expect(store.failure()?.error).toMatchObject({ code: 'failed', retryable: true });
  });
});

describe('the contributions', () => {
  const setup = () => defineSetup({ providers: () => Promise.resolve([]), loadPage: () => Promise.resolve(class {}), loadDialog: () => Promise.resolve(class {}) });

  it('declare a modeless tool window, a settings page and the five commands of the table', () => {
    const { dialog, page } = setup();
    expect(dialog.id).toBe(SETUP_DIALOG_ID);
    expect(dialog.kind).toBe('tool');
    expect([...dialog.commands].sort()).toEqual(Object.keys(SETUP_COMMANDS).sort());
    expect([...SETUP_DIALOG_COMMANDS].sort()).toEqual(Object.keys(SETUP_COMMANDS).sort());
    expect(page.id).toBe(SETUP_SETTINGS_PAGE);
    expect(page.group).toBe('application');
    // The registry lists the window and the page; the command and the menu item come from the batch.
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ ...SETUP_CONTRIBUTIONS, dialogs: [dialog], settingsPages: [page] })] });
    assertContributionsValid(TestBed.inject(Injector));
    expect(SETUP_CONTRIBUTIONS.commands?.[0]?.id).toBe('pd.setup.open');
  });

  it('open the window from the command', () => {
    const opened: string[] = [];
    const command = SETUP_CONTRIBUTIONS.commands?.[0];
    command?.run({ openDialog: (id: string) => (opened.push(id), Promise.resolve(undefined)) } as never);
    expect(opened).toEqual([SETUP_DIALOG_ID]);
  });
});

describe('the main window’s first-run check', () => {
  function mainBed(memory: MemorySetupBackend, ready: () => ReturnType<typeof signal<boolean>> | null) {
    const opened: string[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: (id: string) => (opened.push(id), Promise.resolve(undefined)), notify: () => undefined } },
        provideShellSetup({ backend: () => memory, main: true, ready: ready as never }),
      ],
    });
    TestBed.inject(Injector);
    return opened;
  }

  it('waits for the ready signal, opens the window once on a first run and marks the checklist as shown', async () => {
    const ready = signal(false);
    const memory = new MemorySetupBackend();
    const finish = vi.spyOn(memory, 'finish');
    const opened = mainBed(memory, () => ready);
    TestBed.tick();
    await settle();
    expect(opened).toEqual([]);
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(opened).toEqual([SETUP_DIALOG_ID]);
    expect(finish).toHaveBeenCalledTimes(1);
    ready.set(false);
    ready.set(true);
    TestBed.tick();
    await settle();
    expect(opened).toHaveLength(1);
    // The next start finds it shown: nothing opens.
    expect((await memory.state()).showOnStart).toBe(false);
  });

  it('opens nothing when the steps are done, and says so in the console when the ask fails', async () => {
    const done = new MemorySetupBackend({ finished: true });
    const opened = mainBed(done, () => null);
    await settle();
    expect(opened).toEqual([]);
    TestBed.resetTestingModule();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const failing = new MemorySetupBackend({ stateFailures: 1 });
    mainBed(failing, () => null);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('first-run check'));
    warn.mockRestore();
  });

  it('does not stop a first run when marking it shown fails', async () => {
    const memory = new MemorySetupBackend();
    vi.spyOn(memory, 'finish').mockRejectedValueOnce(new Error('disk'));
    const opened = mainBed(memory, () => null);
    await settle();
    expect(opened).toEqual([SETUP_DIALOG_ID]);
  });
});
