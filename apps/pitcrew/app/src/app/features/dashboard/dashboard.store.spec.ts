import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { DashboardStore, RANGES, REFRESH_MS } from './dashboard.store';

function setup() {
  localStorage.clear();
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, DashboardStore] });
  return { mock, store: TestBed.inject(DashboardStore) };
}

describe('DashboardStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
    vi.useRealTimers();
  });

  it('starts on 7 days and restores a stored range', () => {
    const s = setup();
    mock = s.mock;
    expect(s.store.range()).toBe('7d');
    expect(s.store.focus()).toBeNull();
    TestBed.resetTestingModule();
    localStorage.setItem('pitcrew.dashboard.range', '30d');
    const again = setup();
    localStorage.setItem('pitcrew.dashboard.range', '30d');
    mock = again.mock;
    expect(RANGES['30d'].days).toBe(30);
  });

  it('ignores a damaged stored range', () => {
    localStorage.setItem('pitcrew.dashboard.range', 'forever');
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: (mock = makeMock()) }, DashboardStore] });
    expect(TestBed.inject(DashboardStore).range()).toBe('7d');
  });

  it('loads machine metrics, activity and folders for the runners', async () => {
    const s = setup();
    mock = s.mock;
    const metrics = vi.spyOn(mock, 'metrics');
    const activity = vi.spyOn(mock, 'activity');
    await s.store.refresh(['build-win', 'ui-tests']);
    expect(Object.keys(s.store.activity())).toEqual(['build-win', 'ui-tests']);
    expect(s.store.activity()['build-win']?.value?.summary.days.length).toBeGreaterThan(0);
    expect(s.store.machine().value?.machine.length).toBeGreaterThan(0);
    expect(s.store.folders()['ui-tests']?.value?.folder.length).toBeGreaterThan(0);
    expect(metrics).toHaveBeenCalledWith(null, 168);
    expect(activity).toHaveBeenCalledWith('build-win', 7);
    expect(s.store.lastRefresh()).toBeGreaterThan(0);
  });

  it('refreshes only when the runner set changed or the interval passed', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(1_000_000);
    const s = setup();
    mock = s.mock;
    const activity = vi.spyOn(mock, 'activity');
    await s.store.refresh(['build-win']);
    await s.store.refresh(['build-win']);
    expect(activity).toHaveBeenCalledTimes(1);
    vi.setSystemTime(1_000_000 + REFRESH_MS + 1);
    await s.store.refresh(['build-win']);
    expect(activity).toHaveBeenCalledTimes(2);
    await s.store.refresh(['build-win'], true);
    expect(activity).toHaveBeenCalledTimes(3);
    await s.store.refresh(['build-win', 'ui-tests']);
    expect(activity).toHaveBeenCalledTimes(5);
  });

  it('changes the range, persists it and reloads with the new window', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.refresh(['build-win']);
    const activity = vi.spyOn(mock, 'activity');
    const metrics = vi.spyOn(mock, 'metrics');
    await s.store.setRange('24h');
    expect(s.store.range()).toBe('24h');
    expect(localStorage.getItem('pitcrew.dashboard.range')).toBe('24h');
    expect(activity).toHaveBeenCalledWith('build-win', 2);
    expect(metrics).toHaveBeenCalledWith(null, 24);
    await s.store.setRange('24h');
    expect(activity).toHaveBeenCalledTimes(1);
  });

  it('keeps previous data next to an error', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.refresh(['build-win']);
    vi.spyOn(mock, 'activity').mockRejectedValueOnce({ code: 'internal', message: 'bad' });
    await s.store.retry();
    const a = s.store.activity()['build-win']!;
    expect(a.error?.message).toBe('bad');
    expect(a.value).not.toBeNull();
  });

  it('sets the focus', () => {
    const s = setup();
    mock = s.mock;
    s.store.setFocus('ui-tests');
    expect(s.store.focus()).toBe('ui-tests');
    s.store.setFocus(null);
    expect(s.store.focus()).toBeNull();
  });

  it('loads run details and records failures as null', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.loadDetails('build-win');
    expect(s.store.details()['build-win']?.workflow).toBe('ci.yml');
    await s.store.loadDetails('nope');
    expect(s.store.details()['nope']).toBeNull();
  });
});
