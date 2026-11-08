import { inject, Injectable, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import { FakeClock } from '@decryp7/paddock-ui/shell/testing';
import { READ_RETRY_CLOCK, READ_RETRY_STEADY_MS } from './read-retry';
import { errorMessage, setError, withRequestStatus } from './request-status';

/** Every read waits for the test to answer it; `reads` holds them in order. */
@Injectable()
class Backend {
  reads: { resolve: (v: readonly string[]) => void; reject: (e: unknown) => void }[] = [];
  /** Extra work `apply` hands back (the storage store's "measure when there is nothing yet"). */
  more: Promise<void> | null = null;
  quiet = false;
  read(): Promise<readonly string[]> {
    return new Promise((resolve, reject) => this.reads.push({ resolve, reject }));
  }
}

const SPACING_MS = 3_000;

const Probe = signalStore(
  withState<{ items: readonly string[] }>({ items: [] }),
  withRequestStatus({ retry: true }),
  withMethods((store, backend = inject(Backend)) => {
    const reads = store._reads.define({
      read: () => backend.read(),
      apply: (items) => {
        patchState(store, { items });
        return backend.more ?? undefined;
      },
      message: (e) => errorMessage(e, 'The probe did not answer.'),
      quiet: () => backend.quiet,
      eventSpacingMs: SPACING_MS,
    });
    return {
      load: reads.load,
      /** A change event (part of the state). */
      change: () => reads.event(),
      /** A snapshot event (the whole state). */
      snapshot(items: readonly string[]): void {
        patchState(store, { items });
        reads.received();
      },
      /** A failure that is not the read (an action). */
      actionFailed: (m: string) => patchState(store, setError(m)),
    };
  }),
  withHooks({
    onInit(store) {
      void store.load();
    },
  }),
);

/** Declares `retry: false`: nothing retries and events start nothing. */
const Plain = signalStore(
  withRequestStatus({ retry: false, kind: 'permanent', why: 'a probe of the opt-out' }),
  withMethods((store, backend = inject(Backend)) => {
    const reads = store._reads.define({ read: () => backend.read(), apply: () => undefined, message: (e) => errorMessage(e, 'no') });
    return { load: reads.load, change: () => reads.event() };
  }),
);

/** A macrotask: every promise continuation queued before it has run once it fires (not a count of turns). */
const drained = (): Promise<void> => new Promise<void>((resolve) => setTimeout(resolve));

function bed() {
  const backend = new Backend();
  const clock = new FakeClock(1_000);
  TestBed.configureTestingModule({
    providers: [provideZonelessChangeDetection(), Probe, Plain, { provide: Backend, useValue: backend }, { provide: READ_RETRY_CLOCK, useValue: clock }],
  });
  return { backend, clock };
}

/** A probe whose first read failed with "down", with its first retry armed. */
async function failedFirst() {
  const { backend, clock } = bed();
  const store = TestBed.inject(Probe);
  await vi.waitFor(() => expect(backend.reads).toHaveLength(1));
  backend.reads[0]?.reject(new Error('down'));
  await vi.waitFor(() => expect(store.requestError()).toBe('down'));
  expect(clock.ticking).toBe(1);
  return { backend, clock, store };
}

/** Fails read `n` with `message` and waits until the next retry is armed. */
async function failRead(backend: Backend, clock: FakeClock, n: number, message = 'still down'): Promise<void> {
  backend.reads[n]?.reject(new Error(message));
  await vi.waitFor(() => expect(clock.ticking).toBe(1));
}

describe('withRequestStatus({ retry: true }): a failed read ends on its own (ADR 0058)', () => {
  it('loads: idle, loading, loaded', async () => {
    const { backend } = bed();
    const store = TestBed.inject(Probe);
    expect(store.requestStatus()).toBe('loading');
    backend.reads[0]?.resolve(['a']);
    await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    expect(store.items()).toEqual(['a']);
  });

  it('retries with a growing delay (2 s, 4 s, 8 s, then every 15 s), keeping the error up without loading', async () => {
    const { backend, clock, store } = await failedFirst();
    const steps: [number, number][] = [
      [2_000, 2],
      [4_000, 3],
      [8_000, 4],
      [15_000, 5],
      [15_000, 6],
    ];
    for (const [delay, reads] of steps) {
      clock.advance(delay - 1);
      expect(backend.reads).toHaveLength(reads - 1);
      clock.advance(1);
      expect(backend.reads).toHaveLength(reads);
      expect(store.isLoading()).toBe(false);
      expect(store.requestError()).not.toBeNull();
      await failRead(backend, clock, reads - 1);
      expect(store.requestError()).toBe('still down');
    }
  });

  it('stops retrying once a timer read succeeds: the error clears and no timer is left', async () => {
    const { backend, clock, store } = await failedFirst();
    clock.advance(2_000);
    backend.reads[1]?.resolve(['a']);
    await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    expect(store.items()).toEqual(['a']);
    expect(clock.ticking).toBe(0);
    clock.advance(120_000);
    expect(backend.reads).toHaveLength(2);
  });

  it('does not set the same error again: the status object stays, so the alert is not announced twice', async () => {
    const { backend, clock, store } = await failedFirst();
    clock.advance(2_000);
    await failRead(backend, clock, 1, 'down');
    const before = store.requestStatus();
    clock.advance(4_000);
    await failRead(backend, clock, 2, 'down');
    expect(store.requestStatus()).toBe(before);
    clock.advance(8_000);
    await failRead(backend, clock, 3, 'gone');
    expect(store.requestStatus()).toEqual({ error: 'gone' });
  });

  it('starts the delays again after a manual Retry that fails, which shows loading', async () => {
    const { backend, clock, store } = await failedFirst();
    clock.advance(2_000);
    await failRead(backend, clock, 1);
    clock.advance(4_000);
    await failRead(backend, clock, 2);
    void store.load();
    expect(store.isLoading()).toBe(true);
    expect(clock.ticking).toBe(0);
    await failRead(backend, clock, 3);
    clock.advance(1_999);
    expect(backend.reads).toHaveLength(4);
    clock.advance(1);
    expect(backend.reads).toHaveLength(5);
  });

  it('keeps data on screen while `quiet`: a Retry shows no loading, the error stays until the answer', async () => {
    const { backend, store } = await failedFirst();
    backend.quiet = true;
    const again = store.load();
    expect(store.requestError()).toBe('down');
    backend.reads[1]?.resolve(['b']);
    expect(await again).toBe(true);
    expect(store.requestStatus()).toBe('loaded');
  });

  it('waits for the work `apply` hands back before `load` resolves', async () => {
    const { backend } = bed();
    const store = TestBed.inject(Probe);
    let finish = (): void => undefined;
    backend.more = new Promise<void>((resolve) => (finish = resolve));
    const second = store.load();
    let done = false;
    void second.then(() => (done = true));
    backend.reads[1]?.resolve(['a']);
    await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    await drained();
    expect(done).toBe(false);
    finish();
    expect(await second).toBe(true);
  });

  describe('a change event while in error', () => {
    it('starts one re-read, keeps the error up (no loading), and clears it on success', async () => {
      const { backend, store } = await failedFirst();
      for (let i = 0; i < 5; i++) store.change();
      expect(backend.reads).toHaveLength(2);
      expect(store.requestError()).toBe('down');
      expect(store.isLoading()).toBe(false);
      backend.reads[1]?.resolve(['a']);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    });

    it('stops the self-retry when the event re-read succeeds: no timer left, no more reads', async () => {
      const { backend, clock, store } = await failedFirst();
      store.change();
      backend.reads[1]?.resolve(['a']);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(backend.reads).toHaveLength(2);
    });

    it('re-reads at most once per spacing after a failed re-read', async () => {
      const { backend, clock, store } = await failedFirst();
      store.change();
      await failRead(backend, clock, 1);
      for (let i = 0; i < 5; i++) store.change();
      expect(backend.reads).toHaveLength(2);
      clock.advance(SPACING_MS - 1);
      store.change();
      expect(backend.reads).toHaveLength(2);
      clock.advance(1);
      store.change();
      expect(backend.reads).toHaveLength(3);
      expect(store.requestError()).toBe('still down');
    });

    it('starts no second read when the timer fires during an event re-read', async () => {
      const { backend, clock, store } = await failedFirst();
      store.change();
      expect(backend.reads).toHaveLength(2);
      clock.advance(2_000);
      expect(backend.reads).toHaveLength(2);
    });

    it('starts nothing while the store is loaded', async () => {
      const { backend } = bed();
      const store = TestBed.inject(Probe);
      backend.reads[0]?.resolve(['a']);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      store.change();
      expect(backend.reads).toHaveLength(1);
    });
  });

  describe('a snapshot that arrives by another path', () => {
    it('ends the error at once and stops the self-retry', async () => {
      const { backend, clock, store } = await failedFirst();
      store.snapshot(['s']);
      expect(store.requestStatus()).toBe('loaded');
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(backend.reads).toHaveLength(1);
    });

    it('drops a read still running: its late failure does not bring the error back', async () => {
      const { backend, clock, store } = await failedFirst();
      clock.advance(2_000);
      expect(backend.reads).toHaveLength(2);
      store.snapshot(['s']);
      backend.reads[1]?.reject(new Error('late'));
      await drained();
      expect(store.requestStatus()).toBe('loaded');
      expect(store.items()).toEqual(['s']);
      expect(clock.ticking).toBe(0);
    });

    it('changes nothing when the store is already loaded', async () => {
      const { backend } = bed();
      const store = TestBed.inject(Probe);
      backend.reads[0]?.resolve(['a']);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      const before = store.requestStatus();
      store.snapshot(['b']);
      expect(store.requestStatus()).toBe(before);
    });
  });

  it('a Retry that fails while a timer re-read hangs still retries on its own', async () => {
    const { backend, clock, store } = await failedFirst();
    clock.advance(2_000);
    expect(backend.reads).toHaveLength(2);
    // Read 1 (the timer's) hangs; the user's Retry fails fast and arms the first delay again.
    void store.load();
    await failRead(backend, clock, 2);
    clock.advance(2_000);
    expect(backend.reads).toHaveLength(4);
    // The hung read settles late: it is stale and changes nothing, and the retrying goes on.
    backend.reads[1]?.reject(new Error('late'));
    await failRead(backend, clock, 3);
    expect(store.requestError()).toBe('still down');
    clock.advance(4_000);
    expect(backend.reads).toHaveLength(5);
  });

  it('a stale re-read that settles late does not unblock a second read beside the current one', async () => {
    const { backend, clock, store } = await failedFirst();
    clock.advance(2_000);
    // Read 1 (a timer re-read) hangs; the Retry (read 2) fails; the next timer starts read 3, which runs.
    void store.load();
    await failRead(backend, clock, 2);
    clock.advance(2_000);
    expect(backend.reads).toHaveLength(4);
    // The stale read 1 rejects: its clean-up must not clear read 3's marker.
    backend.reads[1]?.reject(new Error('late'));
    await drained();
    clock.advance(SPACING_MS);
    store.change();
    clock.advance(READ_RETRY_STEADY_MS);
    expect(backend.reads).toHaveLength(4);
  });

  it('re-arms when the timer finds the current re-read still running, so a hang cannot end the retrying', async () => {
    const { backend, clock, store } = await failedFirst();
    store.change();
    expect(backend.reads).toHaveLength(2);
    clock.advance(2_000);
    expect(backend.reads).toHaveLength(2);
    expect(clock.ticking).toBe(1);
  });

  it('drops an older quiet read when the whole state arrives while loaded', async () => {
    const { backend } = bed();
    const store = TestBed.inject(Probe);
    backend.reads[0]?.resolve(['v1']);
    await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    backend.quiet = true;
    const older = store.load();
    store.snapshot(['v3']);
    backend.reads[1]?.resolve(['v2-older']);
    expect(await older).toBe(false);
    expect(store.items()).toEqual(['v3']);
    expect(store.requestStatus()).toBe('loaded');
  });

  it('drops a stale read that finishes after a newer one (Retry)', async () => {
    const { backend, store } = await failedFirst();
    store.change();
    const retry = store.load();
    expect(backend.reads).toHaveLength(3);
    backend.reads[2]?.resolve(['new']);
    expect(await retry).toBe(true);
    backend.reads[1]?.reject(new Error('late failure'));
    await drained();
    expect(store.requestStatus()).toBe('loaded');
    expect(store.items()).toEqual(['new']);
  });

  it('resolves false for a Retry overtaken by a newer one', async () => {
    const { backend, store } = await failedFirst();
    const first = store.load();
    const second = store.load();
    backend.reads[1]?.resolve(['old']);
    backend.reads[2]?.resolve(['new']);
    expect(await first).toBe(false);
    expect(await second).toBe(true);
    expect(store.items()).toEqual(['new']);
  });

  it('runs a one-off read in place of the declared one', async () => {
    const { store } = await failedFirst();
    expect(await store.load(() => Promise.resolve(['once']))).toBe(true);
    expect(store.items()).toEqual(['once']);
    expect(store.requestStatus()).toBe('loaded');
  });

  it('leaves no timer and starts no read after the store is destroyed', async () => {
    const { backend, clock, store } = await failedFirst();
    TestBed.resetTestingModule();
    expect(clock.ticking).toBe(0);
    clock.advance(120_000);
    store.change();
    store.snapshot(['late']);
    expect(backend.reads).toHaveLength(1);
  });

  it('arms no timer when a read fails after the store was destroyed', async () => {
    const { backend, clock } = await failedFirst();
    clock.advance(2_000);
    expect(backend.reads).toHaveLength(2);
    TestBed.resetTestingModule();
    backend.reads[1]?.reject(new Error('late'));
    await drained();
    expect(clock.ticking).toBe(0);
  });

  it('leaves an action failure alone: no retry is armed for it', async () => {
    const { backend, clock } = bed();
    const store = TestBed.inject(Probe);
    backend.reads[0]?.resolve(['a']);
    await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
    store.actionFailed('the action was refused');
    expect(clock.ticking).toBe(0);
  });
});

describe('withRequestStatus({ retry: false })', () => {
  it('neither retries nor re-reads on events', async () => {
    const { backend, clock } = bed();
    const store = TestBed.inject(Plain);
    void store.load();
    backend.reads[0]?.reject('refused');
    await vi.waitFor(() => expect(store.requestError()).toBe('refused'));
    expect(clock.ticking).toBe(0);
    store.change();
    clock.advance(120_000);
    expect(backend.reads).toHaveLength(1);
  });
});
