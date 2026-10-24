import { TestBed } from '@angular/core/testing';
import { ipcBootErrorSender, MOCK_CHECKPOINT_KEY, MOCK_RECOVER_KEY, MockStartupBackend, provideStartupBackend, StartupBackend, TauriStartupBackend, type StartupIpc } from './backend';

function fakeIpc() {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const listeners: { event: string; handler: (p: unknown) => void }[] = [];
  const ipc: StartupIpc = {
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => {
      calls.push(args ? { cmd, args } : { cmd });
      return (cmd === 'window_boot_error' && args?.['kind'] === 'bootstrap' ? Promise.reject(new Error('down')) : Promise.resolve({ recovered: null, ui: null })) as Promise<T>;
    },
    listen: <T>(event: string, handler: (p: T) => void) => {
      listeners.push({ event, handler: handler as (p: unknown) => void });
      return Promise.resolve(() => undefined);
    },
  };
  return { ipc, calls, listeners };
}

describe('TauriStartupBackend', () => {
  it('calls the start-up commands with their arguments', async () => {
    const f = fakeIpc();
    const b = new TauriStartupBackend(f.ipc);
    await expect(b.reportReady()).resolves.toEqual({ recovered: null, ui: null });
    await b.reportBootError('error', 'x');
    await b.checkpointUi({ q: 1 });
    await b.pong(3);
    expect(f.calls).toEqual([
      { cmd: 'window_ready' },
      { cmd: 'window_boot_error', args: { kind: 'error', message: 'x' } },
      { cmd: 'ui_checkpoint', args: { ui: { q: 1 } } },
      { cmd: 'window_pong', args: { seq: 3 } },
    ]);
  });

  it('listens to the ping event and hands on the number', async () => {
    const f = fakeIpc();
    const seen: number[] = [];
    await new TauriStartupBackend(f.ipc).onPing((seq) => seen.push(seq));
    expect(f.listeners.map((l) => l.event)).toEqual(['paddock://ping']);
    f.listeners[0]!.handler({ seq: 9 });
    expect(seen).toEqual([9]);
  });

  it('sends boot errors without ever rejecting', async () => {
    const f = fakeIpc();
    const send = ipcBootErrorSender(f.ipc);
    expect(() => send('bootstrap', 'no providers')).not.toThrow();
    send('error', 'e');
    await Promise.resolve();
    expect(f.calls.map((c) => c.args?.['kind'])).toEqual(['bootstrap', 'error']);
  });
});

describe('MockStartupBackend', () => {
  afterEach(() => sessionStorage.clear());

  it('plays a recovery reload once, with the kept checkpoint', async () => {
    const b = new MockStartupBackend();
    await b.checkpointUi({ note: 'kept' });
    expect(await b.reportReady()).toEqual({ recovered: null, ui: null });
    sessionStorage.setItem(MOCK_RECOVER_KEY, 'hung');
    expect(await b.reportReady()).toEqual({ recovered: 'hung', ui: { note: 'kept' } });
    expect(await b.reportReady()).toEqual({ recovered: null, ui: null });
    sessionStorage.setItem(MOCK_RECOVER_KEY, 'nonsense');
    expect((await b.reportReady()).recovered).toBeNull();
    sessionStorage.setItem(MOCK_CHECKPOINT_KEY, '{broken');
    sessionStorage.setItem(MOCK_RECOVER_KEY, 'terminated');
    expect(await b.reportReady()).toEqual({ recovered: 'terminated', ui: null });
  });

  it('works without session storage', async () => {
    const spy = vi.spyOn(window, 'sessionStorage', 'get').mockImplementation(() => {
      throw new Error('blocked');
    });
    try {
      const b = new MockStartupBackend();
      await b.checkpointUi({});
      expect(await b.reportReady()).toEqual({ recovered: null, ui: null });
    } finally {
      spy.mockRestore();
    }
  });

  it('records boot errors and pongs, and pings its listeners until they stop', async () => {
    const b = new MockStartupBackend();
    await b.reportBootError('rejection', 'r');
    expect(b.bootErrors).toEqual([{ kind: 'rejection', message: 'r' }]);
    const seen: number[] = [];
    const stop = await b.onPing((seq) => seen.push(seq));
    b.ping(1);
    stop();
    b.ping(2);
    await b.pong(1);
    expect(seen).toEqual([1]);
    expect(b.pongs).toEqual([1]);
  });
});

describe('provideStartupBackend', () => {
  it('provides the window backend from a factory', () => {
    const mock = new MockStartupBackend();
    TestBed.configureTestingModule({ providers: [provideStartupBackend(() => mock)] });
    expect(TestBed.inject(StartupBackend)).toBe(mock);
  });
});
