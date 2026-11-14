import { DestroyRef } from '@angular/core';
import { describe, expect, it } from 'vitest';
import { LatestRequest } from './latest-request';

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

function fakeDestroyRef() {
  const callbacks: (() => void)[] = [];
  const ref = { onDestroy: (cb: () => void) => (callbacks.push(cb), () => undefined), destroyed: false } as unknown as DestroyRef;
  return { ref, destroy: () => callbacks.forEach((cb) => cb()) };
}

describe('LatestRequest', () => {
  it('answers the latest request and makes an earlier one stale, its answer and its error alike', async () => {
    const latest = new LatestRequest();
    const first = deferred<number>();
    const second = deferred<number>();
    const a = latest.run(() => first.promise);
    const b = latest.run(() => second.promise);
    second.resolve(2);
    first.reject(new Error('late failure'));
    expect(await a).toEqual({ status: 'stale' });
    expect(await b).toEqual({ status: 'done', value: 2 });
  });

  it('reports the error of the latest request', async () => {
    const latest = new LatestRequest();
    const answer = await latest.run(() => Promise.reject(new Error('engine busy')));
    expect(answer.status).toBe('failed');
    expect(answer.status === 'failed' && (answer.error as Error).message).toBe('engine busy');
  });

  it('cancel makes the request in flight stale and aborts its signal', async () => {
    const latest = new LatestRequest();
    const held = deferred<number>();
    let seen: AbortSignal | undefined;
    const a = latest.run((signal) => ((seen = signal), held.promise));
    expect(latest.busy).toBe(true);
    latest.cancel();
    expect(seen?.aborted).toBe(true);
    expect(latest.busy).toBe(false);
    held.resolve(1);
    expect(await a).toEqual({ status: 'stale' });
  });

  it('cancels on destroy, and every later request is stale at once', async () => {
    const { ref, destroy } = fakeDestroyRef();
    const latest = new LatestRequest(ref);
    const held = deferred<number>();
    const a = latest.run(() => held.promise);
    destroy();
    held.resolve(1);
    expect(await a).toEqual({ status: 'stale' });
    let called = false;
    expect(await latest.run(async () => ((called = true), 2))).toEqual({ status: 'stale' });
    expect(called).toBe(false);
  });

  it('a round (begin) covers several requests until the next round', async () => {
    const latest = new LatestRequest();
    const round = latest.begin();
    const p1 = LatestRequest.settle(round, async () => 1);
    const held = deferred<number>();
    const p2 = LatestRequest.settle(round, () => held.promise);
    expect(await p1).toEqual({ status: 'done', value: 1 });
    latest.begin();
    held.resolve(2);
    expect(await p2).toEqual({ status: 'stale' });
  });
});
