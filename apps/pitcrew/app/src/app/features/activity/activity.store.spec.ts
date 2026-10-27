import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { ACTIVITY_DAYS, ActivityStore } from './activity.store';

function setup() {
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, ActivityStore] });
  return { mock, store: TestBed.inject(ActivityStore) };
}

it('offers 1, 7 and 30 days', () => {
  expect(ACTIVITY_DAYS).toEqual([1, 7, 30]);
});

describe('ActivityStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads each shown runner once for the same selection', async () => {
    const s = setup();
    mock = s.mock;
    const spy = vi.spyOn(mock, 'activity');
    await s.store.show(['build-win', 'ui-tests']);
    expect(Object.keys(s.store.activity())).toEqual(['build-win', 'ui-tests']);
    expect(spy).toHaveBeenCalledWith('build-win', 7);
    await s.store.show(['build-win', 'ui-tests']);
    expect(spy).toHaveBeenCalledTimes(2);
    await s.store.show(['build-win']);
    expect(spy).toHaveBeenCalledTimes(3);
    expect(Object.keys(s.store.activity())).toEqual(['build-win']);
  });

  it('reloads for a new range', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.show(['build-win']);
    const spy = vi.spyOn(mock, 'activity');
    await s.store.setDays(30);
    expect(s.store.days()).toBe(30);
    expect(spy).toHaveBeenCalledWith('build-win', 30);
  });

  it('retries after an error and keeps old data meanwhile', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.show(['build-win']);
    vi.spyOn(mock, 'activity').mockRejectedValueOnce({ code: 'internal', message: 'bad' });
    await s.store.retry();
    expect(s.store.activity()['build-win']?.error?.message).toBe('bad');
    expect(s.store.activity()['build-win']?.value).not.toBeNull();
    await s.store.retry();
    expect(s.store.activity()['build-win']?.error).toBeNull();
  });
});
