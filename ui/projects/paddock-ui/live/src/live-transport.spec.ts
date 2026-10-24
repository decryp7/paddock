import { LIVE_COMMANDS } from '../generated/live-commands';
import { liveEvent, MemoryLiveTransport, tauriLiveTransport, type LiveIpc } from './live-transport';

describe('the Tauri transport', () => {
  it('listens on the channel event and asks the snapshot command with the channel', async () => {
    const calls: unknown[] = [];
    const handlers = new Map<string, (p: unknown) => void>();
    const stop = vi.fn();
    const ipc: LiveIpc = {
      invoke: (command, args) => {
        calls.push([command, args]);
        return Promise.resolve({ kind: 'snapshot' } as never);
      },
      listen: (event, handler) => {
        handlers.set(event, handler);
        return Promise.resolve(stop);
      },
    };
    const transport = tauriLiveTransport(ipc);
    const seen: unknown[] = [];
    const unlisten = await transport.listen('overview', (p) => seen.push(p));
    handlers.get('paddock://live/overview')?.({ kind: 'link' });
    expect(seen).toEqual([{ kind: 'link' }]);
    unlisten();
    expect(stop).toHaveBeenCalled();
    await expect(transport.snapshot('overview')).resolves.toEqual({ kind: 'snapshot' });
    expect(calls).toEqual([['live_snapshot', { channel: 'overview' }]]);
  });

  it('names the event as Rust does and knows the command table', () => {
    expect(liveEvent('overview')).toBe('paddock://live/overview');
    expect(Object.keys(LIVE_COMMANDS)).toEqual(['live_snapshot']);
    expect(LIVE_COMMANDS.live_snapshot.kind).toBe('query');
  });
});

describe('MemoryLiveTransport', () => {
  it('delivers pushes to the listeners of a channel until they stop', async () => {
    const t = new MemoryLiveTransport();
    const a: unknown[] = [];
    const b: unknown[] = [];
    const stopA = await t.listen('one', (p) => a.push(p));
    await t.listen('two', (p) => b.push(p));
    expect(t.listenerCount('one')).toBe(1);
    t.push('one', 1);
    t.push('two', 2);
    t.push('none', 3);
    stopA();
    t.push('one', 4);
    expect([a, b]).toEqual([[1], [2]]);
    expect(t.listenerCount('one')).toBe(0);
    expect(t.listenerCount('nobody')).toBe(0);
  });

  it('answers the snapshot it is given, records the asks, and refuses a channel nobody set up', async () => {
    const t = new MemoryLiveTransport();
    t.setSnapshot('one', () => Promise.resolve({ kind: 'snapshot' }));
    await expect(t.snapshot('one')).resolves.toEqual({ kind: 'snapshot' });
    await expect(t.snapshot('two')).rejects.toThrow('not set up');
    expect(t.snapshotCalls).toEqual(['one', 'two']);
  });
});
