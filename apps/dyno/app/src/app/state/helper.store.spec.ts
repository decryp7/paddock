import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { jobsProviders, mockWithJobs } from '../../testing/job-test-support';
import type { MockHelperVariant } from '../core/mock/helper';
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
});
