import { InjectionToken } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SHELL_IDLE } from '@decryp7/paddock-ui/shell/contributions';
import { FakeCommandContext } from './contributions';
import { provideFakeDock, provideTinyBackendBare } from './helpers';
import { COMMAND_CONTEXT, CommandRegistry } from '../app/extensions/command-registry';
import { lazyCommandFactory } from '../app/extensions/lazy-registration';
import { startupLoadsSettled } from '../app/extensions/startup-loads';
import { settleStartup } from './startup';

// The real clock back after every test, so a test that times out cannot leave fake timers for the next one (ER-12, C154).
afterEach(() => vi.useRealTimers());

/** A lazy command set whose chunk arrives when `release()` is called; start-up begins its load at once (immediate idle). */
function bootGatedSet() {
  const state = { chunkLoaded: false, factoryRuns: 0, release: () => undefined as void };
  const gate = new Promise<void>((resolve) => (state.release = resolve));
  TestBed.configureTestingModule({
    providers: [
      provideTinyBackendBare(),
      provideFakeDock(),
      { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
      { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
      ...lazyCommandFactory(
        async () => {
          await gate;
          state.chunkLoaded = true;
          return () => {
            state.factoryRuns++;
            return [{ id: 'slow.run', title: 'Slow', run: () => undefined }];
          };
        },
        [{ id: 'slow.run', title: 'Slow' }],
      ),
    ],
  });
  TestBed.inject(CommandRegistry).all();
  return state;
}

describe('settleStartup', () => {
  it('waits for a start-up chunk load that is still in flight, however long it takes, and starts no load of its own', async () => {
    const state = bootGatedSet();
    let loadedWhenSettled = false;
    const done = settleStartup().then(() => (loadedWhenSettled = state.chunkLoaded));
    state.release();
    await done;
    expect(loadedWhenSettled).toBe(true);
    expect(state.factoryRuns).toBe(1);
  });

  it('ends under fake timers (the loads are real promises)', async () => {
    vi.useFakeTimers();
    try {
      const state = bootGatedSet();
      const done = startupLoadsSettled();
      state.release();
      await done;
      expect(state.chunkLoaded).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('is already settled when nothing was started, without building a test module', async () => {
    await startupLoadsSettled();
  });
});

/**
 * The shared `afterEach` of test-setup.ts: a test that only boots the app and ends (no settle of its own) must still
 * have its chunk loaded, once, into a live injector, before TestBed tears the module down. The chunk here arrives on a
 * later task than the first test's body.
 */
describe('the shared afterEach for booted start-up work', () => {
  const seen = { factoryRuns: 0, chunkLoads: 0 };

  it('boots a lazy set whose chunk arrives after the test body, and does nothing else', () => {
    seen.factoryRuns = 0;
    seen.chunkLoads = 0;
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        ...lazyCommandFactory(
          () =>
            new Promise((resolve) => {
              seen.chunkLoads++;
              // A slow disk or a loaded machine: the chunk lands on a later task than the test body's end.
              setTimeout(
                () =>
                  resolve(() => {
                    seen.factoryRuns++;
                    return [{ id: 'slow.run', title: 'Slow', run: () => undefined }];
                  }),
                0,
              );
            }),
          [{ id: 'slow.run', title: 'Slow' }],
        ),
      ],
    });
    TestBed.inject(CommandRegistry).all();
  });

  it('had the chunk loaded exactly once, into the live injector, before the module was torn down', () => {
    expect(seen.chunkLoads).toBe(1);
    expect(seen.factoryRuns).toBe(1);
  });

  // `it.fails` passes when its body throws: the hook after it must not turn that failure into the next test's.
  it.fails('a failing test that booted start-up ...', () => {
    bootGatedSet().release();
    throw new Error('this test fails on purpose');
  });

  // A load that never settles: the test body's own `settleStartup()` takes the load from the record and waits on it, so
  // this test's timeout is what ends it, and the hook after it finds nothing to wait on (the record was drained). The
  // next test therefore does not wait on it either.
  it.fails('a start-up load that never settles ...', { timeout: 300 }, async () => {
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        { provide: COMMAND_CONTEXT, useValue: new FakeCommandContext() },
        { provide: SHELL_IDLE, useValue: (work: () => void) => work() },
        ...lazyCommandFactory(() => new Promise(() => undefined), [{ id: 'never.run', title: 'Never' }]),
      ],
    });
    TestBed.inject(CommandRegistry).all();
    await settleStartup();
  });

  it('... leaves the next, clean test green and not waiting on it', async () => {
    await startupLoadsSettled();
  });

  it('... leaves the next, clean test green and with a TestBed it can still configure', () => {
    const clean = new InjectionToken<string>('clean');
    TestBed.configureTestingModule({ providers: [{ provide: clean, useValue: 'ok' }] });
    expect(TestBed.inject(clean)).toBe('ok');
  });
});
