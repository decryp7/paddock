import { TestBed } from '@angular/core/testing';
import type { BackendEvent, PitcrewNotification, StatusReply } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { PreferencesService } from '../core/preferences.service';
import { flush, makeMock } from '../../testing/test-helpers';
import { StatusStore } from './status.store';
import { NoticeLog, recordShell } from '../../testing/test-helpers';

function setup(mock = makeMock()) {
  localStorage.clear();
  TestBed.configureTestingModule({ providers: [...recordShell(), { provide: BACKEND, useValue: mock }] });
  const store = TestBed.inject(StatusStore);
  return { mock, store, toasts: TestBed.inject(NoticeLog) };
}

const note = (over: Partial<PitcrewNotification> = {}): PitcrewNotification => ({
  kind: 'stalled',
  severity: 'warning',
  runner: 'build-win',
  title: 'Stalled',
  body: 'polls timed out',
  at: 1,
  ...over,
});

describe('StatusStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads status on init and learns the connection from the backend', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(s.store.runners().map((r) => r.name)).toEqual(['build-win', 'ui-tests', 'docker-lab', 'legacy-ci']);
    expect(s.store.connection()).toEqual({ state: 'connected', serviceVersion: '0.1.0' });
    expect(s.store.service()?.version).toBe('0.1.0');
    expect(s.store.offline()).toBeNull();
    expect(s.store.loading()).toBe(false);
    expect(s.store.eventCount()).toBeGreaterThan(0);
    expect(s.store.runner('ui-tests')?.name).toBe('ui-tests');
    expect(s.store.runner('zzz')).toBeUndefined();
  });

  it('flags runners that need attention', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(s.store.attention().map((r) => r.name)).toEqual(['docker-lab']);
  });

  it('is offline when the service is unreachable or mismatched', async () => {
    const s = setup(makeMock({ scenario: 'unavailable' }));
    mock = s.mock;
    await flush();
    expect(s.store.offline()?.kind).toBe('unavailable');
    expect(s.store.error()?.code).toBe('serviceUnavailable');
    TestBed.resetTestingModule();
    const m = setup(makeMock({ scenario: 'mismatch' }));
    mock = m.mock;
    await flush();
    expect(m.store.offline()?.kind).toBe('mismatch');
  });

  it('derives offline from an error alone when no connection event told us', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    vi.spyOn(mock, 'status').mockRejectedValueOnce({ code: 'protocolMismatch', message: 'old' });
    await s.store.load();
    expect(s.store.offline()).toEqual({ kind: 'mismatch', message: 'old' });
    vi.spyOn(mock, 'status').mockRejectedValueOnce({ code: 'serviceUnavailable', message: 'down' });
    await s.store.load();
    expect(s.store.offline()).toEqual({ kind: 'unavailable', message: 'down' });
    vi.spyOn(mock, 'status').mockRejectedValueOnce({ code: 'internal', message: 'oops' });
    await s.store.load();
    expect(s.store.offline()).toBeNull();
    expect(s.store.error()?.message).toBe('oops');
  });

  it('applies pushed status events and clears the error', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    const status: StatusReply = { runners: [], service: s.store.service()! };
    s.store.onEvent({ kind: 'status', status });
    expect(s.store.runners()).toEqual([]);
  });

  it('keeps notifications (newest first, capped) and toasts the enabled kinds', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    TestBed.inject(PreferencesService).setEnabled('diskLow', false);
    s.store.onEvent({ kind: 'notification', notification: note({ title: 'one' }) });
    s.store.onEvent({ kind: 'notification', notification: note({ kind: 'diskLow', severity: 'error', title: 'two' }) });
    expect(s.store.notifications().map((n) => n.title)).toEqual(['two', 'one']);
    expect(s.toasts.notices).toHaveLength(1);
    expect(s.toasts.notices[0]?.message).toContain('one');
    expect(s.toasts.notices[0]).toMatchObject({ level: 'warning' });
    for (let i = 0; i < 120; i++) s.store.onEvent({ kind: 'notification', notification: note({ title: `n${i}` }) });
    expect(s.store.notifications()).toHaveLength(100);
    s.store.clearNotifications();
    expect(s.store.notifications()).toEqual([]);
  });

  it('reloads when the connection comes back, not on every connected event', async () => {
    const s = setup(makeMock({ scenario: 'unavailable' }));
    mock = s.mock;
    await flush();
    const spy = vi.spyOn(mock, 'status');
    await mock.installService();
    await flush();
    expect(spy).toHaveBeenCalledTimes(1);
    s.store.onEvent({ kind: 'connection', connection: { state: 'connected', serviceVersion: '0.1.0' } });
    await flush();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(s.store.runners()).toHaveLength(4);
  });

  it('runs runner actions, tracking them in flight and toasting the result', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    const p = s.store.act('ui-tests', 'drain');
    expect(s.store.inFlight()['ui-tests']).toBe('drain');
    expect(await p).toBe(true);
    expect(s.store.inFlight()['ui-tests']).toBeUndefined();
    expect(s.toasts.notices.at(-1)?.message).toContain('Draining ui-tests');
    expect(s.store.runner('ui-tests')?.status).toBe('paused');
  });

  it('toasts a failed action with the reason', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(await s.store.act('ui-tests', 'start')).toBe(false);
    const t = s.toasts.notices.at(-1)!;
    expect(t.level).toBe('error');
    expect(t.message).toContain('ui-tests: that did not work');
    expect(t.message).toContain('already running');
  });

  it('skips, pins and unpins versions', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(await s.store.skipVersion('ui-tests', '5.3.0')).toBe(true);
    expect(s.toasts.notices.at(-1)?.message).toContain('ui-tests will skip 5.3.0');
    expect(await s.store.pinVersion('ui-tests', '5.2.0')).toBe(true);
    expect(s.toasts.notices.at(-1)?.message).toContain('ui-tests is pinned to 5.2.0');
    expect(await s.store.pinVersion('ui-tests', null)).toBe(true);
    expect(s.toasts.notices.at(-1)?.message).toContain('ui-tests follows the latest release again');
    expect(await s.store.pinVersion('ui-tests', 'bad')).toBe(false);
  });

  it('cleans a runner and toasts what was freed', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    const report = await s.store.clean('docker-lab');
    expect(report?.trigger).toBe('manual');
    const t = s.toasts.notices.at(-1)!;
    expect(t.message).toContain('on docker-lab');
    expect(t.level).toBe('warning');
    expect(t.message).toContain('could not be deleted');
    expect(await s.store.clean('nope')).toBeNull();
    vi.spyOn(mock, 'cleanDisk').mockResolvedValueOnce({ trigger: 'manual', at: 1, freedBytes: 0, deleted: 2, failures: [] });
    await s.store.clean('ui-tests');
    expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'success' });
    expect(s.toasts.notices.at(-1)?.message).toContain('2 items deleted.');
  });

  it('removes a runner and reloads', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(await s.store.remove('legacy-ci')).toBe(true);
    expect(s.store.runners().map((r) => r.name)).not.toContain('legacy-ci');
    expect(await s.store.remove('legacy-ci')).toBe(false);
  });

  it('unsubscribes from the backend when destroyed', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    const before = s.store.eventCount();
    TestBed.resetTestingModule();
    mock.start('x').catch(() => undefined);
    await mock.stop('ui-tests');
    expect(s.store.eventCount()).toBe(before);
  });

  it('handles events of every kind through the public handler', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    const events: BackendEvent[] = [{ kind: 'connection', connection: { state: 'mismatch', message: 'm' } }];
    for (const e of events) s.store.onEvent(e);
    expect(s.store.offline()?.kind).toBe('mismatch');
  });
});
