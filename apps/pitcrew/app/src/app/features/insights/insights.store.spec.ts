import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { INSIGHTS_DAYS, InsightsStore } from './insights.store';

describe('InsightsStore', () => {
  let mock: MockBackend;
  function setup() {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, InsightsStore] });
    return TestBed.inject(InsightsStore);
  }
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('offers 7, 14, 30 and 90 days and starts at 30', () => {
    const store = setup();
    expect(INSIGHTS_DAYS).toEqual([7, 14, 30, 90]);
    expect(store.days()).toBe(30);
    expect(store.reply().value).toBeNull();
  });

  it('loads the selected range', async () => {
    const store = setup();
    const spy = vi.spyOn(mock, 'insights');
    await store.load();
    expect(spy).toHaveBeenCalledWith(30);
    expect(store.reply().value?.insights.rangeDays).toBe(30);
    expect(store.reply().loading).toBe(false);
  });

  it('reloads for a new range and ignores re-selecting the loaded one', async () => {
    const store = setup();
    const spy = vi.spyOn(mock, 'insights');
    await store.load();
    await store.setDays(30);
    expect(spy).toHaveBeenCalledTimes(1);
    await store.setDays(7);
    expect(spy).toHaveBeenLastCalledWith(7);
    expect(store.reply().value?.insights.rangeDays).toBe(7);
  });

  it('keeps the last good value next to an error and recovers on retry', async () => {
    const store = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'insights').mockRejectedValueOnce(new Error('boom'));
    await store.retry();
    expect(store.reply().error).not.toBeNull();
    expect(store.reply().value).not.toBeNull();
    spy.mockRestore();
    await store.retry();
    expect(store.reply().error).toBeNull();
  });
});
