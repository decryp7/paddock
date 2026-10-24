import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { signalStore } from '@ngrx/signals';
import { LIVE_CONVERSATION } from '../generated/live-fixtures';
import type { LiveFrame, LiveLink } from './fold';
import { withLiveFeed } from './live-feed.store';
import { LIVE_TRANSPORT, MemoryLiveTransport } from './live-transport';

interface Overview {
  memory: number;
  events: string[];
  count: number;
}
type Change = { kind: 'memory'; bytes: number } | { kind: 'event'; text: string } | { kind: 'bump' };

const empty: Overview = { memory: 0, events: [], count: 0 };
const apply = (s: Overview, d: Change): Overview =>
  d.kind === 'memory' ? { ...s, memory: d.bytes } : d.kind === 'event' ? { ...s, events: [...s.events, d.text] } : { ...s, count: s.count + 1 };

const OverviewStore = signalStore(withLiveFeed<Overview, Change>({ channel: 'overview', empty, apply }));

const connected: LiveLink = { state: 'connected' };
const snapshot = (seq: number, state: Overview = empty, link: LiveLink = connected): LiveFrame<Overview, Change> => ({ kind: 'snapshot', seq, state, link });
const delta = (from: number, seq: number, d: Change): LiveFrame<Overview, Change> => ({ kind: 'delta', from, seq, delta: d });
const bump = (n: number) => delta(n, n, { kind: 'bump' });

/** A snapshot answer the spec resolves or rejects when it says. */
function held() {
  const asks: { resolve: (v: unknown) => void; reject: (e: unknown) => void }[] = [];
  return {
    answer: () => new Promise<unknown>((resolve, reject) => asks.push({ resolve, reject })),
    asks,
  };
}

function setup(answer: () => Promise<unknown> = () => Promise.resolve(snapshot(0))) {
  const transport = new MemoryLiveTransport();
  transport.setSnapshot('overview', answer);
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: LIVE_TRANSPORT, useValue: transport }, OverviewStore] });
  return { transport, store: TestBed.inject(OverviewStore) };
}

describe('withLiveFeed', () => {
  it('starts empty and idle, then listens and loads the first snapshot', async () => {
    const h = held();
    const { transport, store } = setup(h.answer);
    expect(store.requestStatus()).toBe('idle');
    expect(store.live()).toEqual(empty);
    expect(store.liveStale()).toBe(true);
    const connecting = store.connectLive();
    expect(store.isLoading()).toBe(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(transport.listenerCount('overview')).toBe(1);
    h.asks[0]?.resolve(snapshot(4, { memory: 5, events: ['a'], count: 1 }));
    await connecting;
    expect(store.isLoaded()).toBe(true);
    expect(store.live()).toEqual({ memory: 5, events: ['a'], count: 1 });
    expect(store.liveSeq()).toBe(4);
    expect(store.liveConnected()).toBe(true);
    expect(store.liveStale()).toBe(false);
  });

  it('connects once however often it is asked', async () => {
    const { transport, store } = setup();
    const first = store.connectLive();
    expect(store.connectLive()).toBe(first);
    await first;
    expect(transport.listenerCount('overview')).toBe(1);
    expect(transport.snapshotCalls).toEqual(['overview']);
  });

  it('folds the pushed deltas in order, with the same result as Rust', async () => {
    const { transport, store } = setup(() => Promise.resolve(snapshot(0)));
    await store.connectLive();
    for (const frame of LIVE_CONVERSATION.frames) transport.push('overview', frame);
    expect(store.live()).toEqual(LIVE_CONVERSATION.finalState);
    expect(store.liveSeq()).toBe(LIVE_CONVERSATION.finalSeq);
    expect(store.liveLink()).toEqual(LIVE_CONVERSATION.finalLink);
    expect(store.liveStale()).toBe(true);
    expect(transport.snapshotCalls).toHaveLength(1);
  });

  it('ignores a delta it already has and a payload that is not a frame', async () => {
    const { transport, store } = setup(() => Promise.resolve(snapshot(2, { ...empty, count: 2 })));
    await store.connectLive();
    transport.push('overview', bump(2));
    transport.push('overview', { nonsense: true });
    transport.push('overview', null);
    expect(store.live().count).toBe(2);
    transport.push('overview', bump(3));
    expect(store.live().count).toBe(3);
    expect(transport.snapshotCalls).toHaveLength(1);
  });

  it('asks for one snapshot when numbers are skipped, however many frames follow the gap', async () => {
    const h = held();
    const { transport, store } = setup(() => Promise.resolve(snapshot(0)));
    await store.connectLive();
    transport.setSnapshot('overview', h.answer);
    transport.push('overview', bump(5));
    transport.push('overview', bump(6));
    transport.push('overview', bump(7));
    expect(transport.snapshotCalls).toHaveLength(2);
    expect(store.isLoading()).toBe(true);
    expect(store.live().count).toBe(0);
    h.asks[0]?.resolve(snapshot(7, { ...empty, count: 7 }));
    await store.resyncLive();
    expect(store.live().count).toBe(7);
    expect(store.liveSeq()).toBe(7);
    expect(store.isLoaded()).toBe(true);
    transport.push('overview', bump(8));
    expect(store.live().count).toBe(8);
  });

  it('keeps what events already folded when the fetched snapshot is older', async () => {
    const h = held();
    const { transport, store } = setup(h.answer);
    const connecting = store.connectLive();
    await Promise.resolve();
    await Promise.resolve();
    transport.push('overview', snapshot(10, { ...empty, count: 10 }));
    expect(store.liveSeq()).toBe(10);
    h.asks[0]?.resolve(snapshot(6, { ...empty, count: 6 }));
    await connecting;
    expect(store.liveSeq()).toBe(10);
    expect(store.live().count).toBe(10);
  });

  it('takes a pushed snapshot (Rust reseeded) in place of everything', async () => {
    const { transport, store } = setup(() => Promise.resolve(snapshot(5, { ...empty, count: 5 })));
    await store.connectLive();
    transport.push('overview', snapshot(1, { ...empty, memory: 1 }));
    expect(store.live()).toEqual({ ...empty, memory: 1 });
    expect(store.liveSeq()).toBe(1);
  });

  it('shows a failed load as an error and loads again on retry', async () => {
    let fail = true;
    const { store } = setup(() => (fail ? Promise.reject(new Error('the server is gone')) : Promise.resolve(snapshot(3))));
    await store.connectLive();
    expect(store.requestStatus()).toEqual({ error: 'the server is gone' });
    expect(store.requestError()).toBe('the server is gone');
    fail = false;
    await store.resyncLive();
    expect(store.isLoaded()).toBe(true);
    expect(store.liveSeq()).toBe(3);
  });

  it('refuses an answer that is not a snapshot', async () => {
    const { store } = setup(() => Promise.resolve({ kind: 'link', link: connected }));
    await store.connectLive();
    expect(store.requestError()).toContain('not a snapshot');
  });

  it('reports a listen that fails and can connect again', async () => {
    const transport = new MemoryLiveTransport();
    transport.setSnapshot('overview', () => Promise.resolve(snapshot(1)));
    let broken = true;
    const listen = transport.listen.bind(transport);
    transport.listen = (c, f) => (broken ? Promise.reject(new Error('no events')) : listen(c, f));
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: LIVE_TRANSPORT, useValue: transport }, OverviewStore] });
    const store = TestBed.inject(OverviewStore);
    await store.connectLive();
    expect(store.requestError()).toBe('no events');
    broken = false;
    await store.connectLive();
    expect(store.isLoaded()).toBe(true);
    expect(transport.listenerCount('overview')).toBe(1);
  });

  it('uses a default message for a failure that says nothing', async () => {
    const { store } = setup(() => Promise.reject(undefined));
    await store.connectLive();
    expect(store.requestError()).toBe('The live feed did not answer');
  });

  it('stops listening on disconnect and when the store is destroyed', async () => {
    const { transport, store } = setup();
    await store.connectLive();
    store.disconnectLive();
    expect(transport.listenerCount('overview')).toBe(0);
    transport.push('overview', bump(1));
    expect(store.live().count).toBe(0);
    await store.connectLive();
    expect(transport.listenerCount('overview')).toBe(1);
    TestBed.resetTestingModule();
    expect(transport.listenerCount('overview')).toBe(0);
  });

  it('undoes a listen that finishes after a disconnect', async () => {
    const transport = new MemoryLiveTransport();
    transport.setSnapshot('overview', () => Promise.resolve(snapshot(1)));
    const stop = vi.fn();
    let finish: (u: () => void) => void = () => undefined;
    transport.listen = () => new Promise((resolve) => (finish = resolve));
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: LIVE_TRANSPORT, useValue: transport }, OverviewStore] });
    const store = TestBed.inject(OverviewStore);
    const connecting = store.connectLive();
    store.disconnectLive();
    finish(stop);
    await connecting;
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
