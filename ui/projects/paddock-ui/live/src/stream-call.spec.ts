import { streamCall, tauriChannelFactory, type ChannelFactory } from './stream-call';

/** A factory that keeps the handler so a spec plays the messages Rust sends. */
function fake() {
  let handler: ((m: never) => void) | null = null;
  const factory: ChannelFactory = <T>(onMessage: (m: T) => void) => {
    handler = onMessage as (m: never) => void;
    return { fake: true };
  };
  return { factory, send: (m: unknown) => handler?.(m as never) };
}

/** A command whose answer the spec gives. */
function command<R>() {
  let resolve: (v: R) => void = () => undefined;
  let reject: (e: unknown) => void = () => undefined;
  const promise = new Promise<R>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { start: () => promise, resolve, reject };
}

describe('streamCall', () => {
  it('passes the channel to the command and delivers messages in order until the answer', async () => {
    const f = fake();
    const cmd = command<string>();
    const seen: number[] = [];
    let passed: unknown;
    const call = streamCall<number, string>(f.factory, (c) => ((passed = c), cmd.start()), { onMessage: (m) => seen.push(m) });
    f.send(1);
    f.send(2);
    f.send(3);
    cmd.resolve('done');
    await expect(call).resolves.toBe('done');
    f.send(4);
    expect(seen).toEqual([1, 2, 3]);
    expect(passed).toEqual({ fake: true });
  });

  it('with isLast, waits for the last message even when the answer comes first', async () => {
    const f = fake();
    const cmd = command<string>();
    const seen: string[] = [];
    let settled = false;
    const call = streamCall<string, string>(f.factory, cmd.start, { onMessage: (m) => seen.push(m), isLast: (m) => m === 'end' }).then((v) => ((settled = true), v));
    f.send('a');
    cmd.resolve('answer');
    await Promise.resolve();
    await Promise.resolve();
    expect(settled).toBe(false);
    f.send('b');
    f.send('end');
    await expect(call).resolves.toBe('answer');
    expect(seen).toEqual(['a', 'b', 'end']);
  });

  it('with isLast, resolves when the last message comes before the answer', async () => {
    const f = fake();
    const cmd = command<number>();
    const call = streamCall<string, number>(f.factory, cmd.start, { onMessage: () => undefined, isLast: () => true });
    f.send('x');
    cmd.resolve(7);
    await expect(call).resolves.toBe(7);
  });

  it('rejects at once when the command fails, and drops what comes after', async () => {
    const f = fake();
    const cmd = command<string>();
    const seen: number[] = [];
    const call = streamCall<number, string>(f.factory, cmd.start, { onMessage: (m) => seen.push(m), isLast: () => false });
    f.send(1);
    cmd.reject(new Error('the engine stopped'));
    await expect(call).rejects.toThrow('the engine stopped');
    f.send(2);
    expect(seen).toEqual([1]);
  });

  it('stops delivering when the handler throws and rejects with its error after the command ended', async () => {
    const f = fake();
    const cmd = command<string>();
    const seen: number[] = [];
    const call = streamCall<number, string>(f.factory, cmd.start, {
      onMessage: (m) => {
        seen.push(m);
        if (m === 2) throw new Error('bad row');
      },
    });
    f.send(1);
    f.send(2);
    f.send(3);
    cmd.resolve('ok');
    await expect(call).rejects.toThrow('bad row');
    expect(seen).toEqual([1, 2]);
  });

  it('a handler error with isLast does not wait for a last message that will not come', async () => {
    const f = fake();
    const cmd = command<string>();
    const call = streamCall<number, string>(f.factory, cmd.start, {
      onMessage: () => {
        throw new Error('bad');
      },
      isLast: () => false,
    });
    f.send(1);
    cmd.resolve('ok');
    await expect(call).rejects.toThrow('bad');
  });

  it('aborts: nothing more is delivered and the call rejects with an AbortError', async () => {
    const f = fake();
    const cmd = command<string>();
    const ctl = new AbortController();
    const seen: number[] = [];
    const call = streamCall<number, string>(f.factory, cmd.start, { onMessage: (m) => seen.push(m), signal: ctl.signal });
    f.send(1);
    ctl.abort();
    f.send(2);
    await expect(call).rejects.toMatchObject({ name: 'AbortError' });
    cmd.resolve('late');
    expect(seen).toEqual([1]);
  });

  it('rejects at once for a signal that is already aborted, without starting the command', async () => {
    const f = fake();
    const start = vi.fn();
    const ctl = new AbortController();
    ctl.abort();
    await expect(streamCall(f.factory, start, { onMessage: () => undefined, signal: ctl.signal })).rejects.toMatchObject({ name: 'AbortError' });
    expect(start).not.toHaveBeenCalled();
  });

  it('removes its abort listener when it settles', async () => {
    const f = fake();
    const cmd = command<string>();
    const ctl = new AbortController();
    const remove = vi.spyOn(ctl.signal, 'removeEventListener');
    const call = streamCall(f.factory, cmd.start, { onMessage: () => undefined, signal: ctl.signal });
    cmd.resolve('x');
    await call;
    expect(remove).toHaveBeenCalledWith('abort', expect.any(Function));
  });
});

describe('tauriChannelFactory', () => {
  it('builds a channel of Tauri\'s class whose messages go to the handler', () => {
    class FakeChannel {
      onmessage: (m: never) => void = () => undefined;
    }
    const factory = tauriChannelFactory(FakeChannel);
    const seen: number[] = [];
    const channel = factory<number>((m) => seen.push(m)) as FakeChannel;
    expect(channel).toBeInstanceOf(FakeChannel);
    channel.onmessage(5 as never);
    expect(seen).toEqual([5]);
  });
});
