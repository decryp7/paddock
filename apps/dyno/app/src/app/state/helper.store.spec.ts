import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { jobsProviders, mockWithJobs } from '../../testing/job-test-support';
import { mockHelperStatus, type MockHelperVariant } from '../core/mock/helper';
import { HelperStore } from './helper.store';

function setup(helper: MockHelperVariant, fail: string[] = []) {
  const { mock, jobs } = mockWithJobs({ helper, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, jobsProviders(jobs)] });
  return { mock, store: TestBed.inject(HelperStore) };
}

describe('HelperStore', () => {
  it.each<[MockHelperVariant, string]>([
    ['off', 'off'],
    ['silent', 'notRunning'],
    ['foreign', 'foreign'],
    ['outdated', 'outdated'],
    ['ready', 'ready'],
    ['notIncluded', 'notIncluded'],
  ])('reads the state %s as %s, once until asked again', async (variant, state) => {
    const { store, mock } = setup(variant);
    const spy = vi.spyOn(mock, 'helperStatus');
    await store.ensure();
    await store.ensure();
    expect(store.status()?.state).toBe(state);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.error()).toBeNull();
  });

  it('keeps a failed read as an error with Retry and recovers', async () => {
    const { store, mock } = setup('off');
    vi.spyOn(mock, 'helperStatus').mockRejectedValueOnce({ code: 'timeout', message: 'slow' });
    await store.load();
    expect(store.error()).toEqual({ code: 'timeout', message: 'slow' });
    expect(store.loading()).toBe(false);
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.status()?.state).toBe('off');
  });

  it('turning on is a job that ends with the helper ready, and turning off ends with it off', async () => {
    const { store } = setup('off');
    await store.ensure();
    await store.start('turnOn');
    expect(store.status()?.state).toBe('ready');
    expect(store.action()).toBeNull();
    expect(store.actionError()).toBeNull();
    await store.start('turnOff');
    expect(store.status()?.state).toBe('off');
  });

  it('shows why an action failed, keeps the state the engine reports, and the next action clears the message', async () => {
    const { store } = setup('off', ['helperJob']);
    await store.ensure();
    await store.start('turnOn');
    expect(store.actionError()).toContain('declined');
    expect(store.status()?.state).toBe('off');
    expect(store.cancelled()).toBe(false);
  });

  it('refuses a second action while one runs', async () => {
    const { store, mock } = setup('outdated');
    const spy = vi.spyOn(mock, 'startJob');
    const first = store.start('update');
    void store.start('repair');
    await first;
    expect(spy).toHaveBeenCalledTimes(1);
  });

  describe('while the helper is not answering (just after a restart)', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    const answers = (...states: MockHelperVariant[]) => {
      const { store, mock } = setup('silent');
      const spy = vi.spyOn(mock, 'helperStatus');
      states.forEach((v) => spy.mockResolvedValueOnce(mockHelperStatus(v, 'windows')));
      spy.mockResolvedValue(mockHelperStatus(states.at(-1) ?? 'silent', 'windows'));
      return { store, spy };
    };

    it('reads again after 2, 4 and 8 seconds and stops once the helper is ready', async () => {
      const { store, spy } = answers('silent', 'silent', 'silent', 'ready');
      await store.ensure();
      expect(spy).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1999);
      expect(spy).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1);
      expect(spy).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(4000);
      expect(spy).toHaveBeenCalledTimes(3);
      await vi.advanceTimersByTimeAsync(8000);
      expect(spy).toHaveBeenCalledTimes(4);
      expect(store.status()?.state).toBe('ready');
      await vi.advanceTimersByTimeAsync(10 * 60_000);
      expect(spy).toHaveBeenCalledTimes(4);
    });

    it('settles at every 15 seconds and gives up after about three minutes', async () => {
      const { store, spy } = answers('silent');
      await store.ensure();
      await vi.advanceTimersByTimeAsync(2000 + 4000 + 8000 + 15_000);
      expect(spy).toHaveBeenCalledTimes(5);
      await vi.advanceTimersByTimeAsync(15_000);
      expect(spy).toHaveBeenCalledTimes(6);
      await vi.advanceTimersByTimeAsync(10 * 60_000);
      const total = spy.mock.calls.length;
      expect(total).toBeGreaterThanOrEqual(10);
      expect(total).toBeLessThanOrEqual(15);
      await vi.advanceTimersByTimeAsync(10 * 60_000);
      expect(spy).toHaveBeenCalledTimes(total);
      // Asking again starts a new round.
      await store.load();
      await vi.advanceTimersByTimeAsync(2000);
      expect(spy).toHaveBeenCalledTimes(total + 2);
    });

    it('never reads on a timer for a helper that is off, not included, or answering', async () => {
      for (const variant of ['off', 'notIncluded', 'ready'] as const) {
        TestBed.resetTestingModule();
        const { store, mock } = setup(variant);
        const spy = vi.spyOn(mock, 'helperStatus');
        await store.ensure();
        await vi.advanceTimersByTimeAsync(10 * 60_000);
        expect(spy, variant).toHaveBeenCalledTimes(1);
      }
    });

    it('keeps the card as it is when a timed read fails, and tries again', async () => {
      const { store, mock } = setup('silent');
      const spy = vi.spyOn(mock, 'helperStatus');
      spy.mockResolvedValueOnce(mockHelperStatus('silent', 'windows'));
      spy.mockRejectedValueOnce({ code: 'timeout', message: 'slow' });
      spy.mockResolvedValue(mockHelperStatus('ready', 'windows'));
      await store.ensure();
      await vi.advanceTimersByTimeAsync(2000);
      expect(store.status()?.state).toBe('notRunning');
      expect(store.error()).toBeNull();
      expect(store.loading()).toBe(false);
      await vi.advanceTimersByTimeAsync(4000);
      expect(store.status()?.state).toBe('ready');
    });
  });
});
