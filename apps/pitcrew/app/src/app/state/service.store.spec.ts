import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { DOWN_POLL_MS, MAX_DOWN_CHECKS, REPAIR_POLL_MS, ServiceStore } from './service.store';
import { StatusStore } from './status.store';
import { NoticeLog, recordShell } from '../../testing/test-helpers';

function setup(scenario: 'demo' | 'notInstalled' = 'notInstalled') {
  const mock = makeMock({ scenario });
  TestBed.configureTestingModule({ providers: [...recordShell(), { provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ServiceStore), status: TestBed.inject(StatusStore), toasts: TestBed.inject(NoticeLog) };
}

describe('ServiceStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads the registration', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load();
    expect(s.store.registration().value).toMatchObject({ installed: false, canInstall: true });
    expect(s.store.registration().loading).toBe(false);
  });

  it('installs, toasts, reloads status and registration', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    expect(s.status.offline()).not.toBeNull();
    const p = s.store.install();
    expect(s.store.working()).toBe('install');
    await p;
    expect(s.store.working()).toBeNull();
    expect(s.toasts.notices.at(-1)?.message).toContain('The Pitcrew service is installed and running');
    expect(s.store.registration().value?.installed).toBe(true);
    expect(s.status.runners().length).toBe(4);
    expect(s.status.offline()).toBeNull();
  });

  describe('the macOS repair', () => {
    const base = { platform: 'macos', installed: true, running: false, canInstall: true, message: null, repair: null, disabledInLoginItems: false, startFailed: false, notInLoginItems: false } as const;
    const working = { state: 'working', message: 'Re-registering the background service with macOS… (this can take a minute)' } as const;

    afterEach(() => vi.useRealTimers());

    it('looks again every few seconds while it works, then toasts "Service is running again"', async () => {
      const s = setup('demo');
      mock = s.mock;
      await flush();
      vi.useFakeTimers();
      const spy = vi.spyOn(mock, 'serviceInstall');
      spy.mockResolvedValueOnce({ ...base, repair: working });
      await s.store.load();
      expect(s.store.registration().value?.repair).toEqual(working);
      spy.mockResolvedValueOnce({ ...base, repair: working });
      await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS);
      expect(spy).toHaveBeenCalledTimes(2);
      spy.mockResolvedValueOnce({ ...base, running: true, repair: { state: 'succeeded', message: 'Service is running again' } });
      await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS);
      expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'success' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Service is running again');
      await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS * 3);
      expect(spy).toHaveBeenCalledTimes(3);
    });

    it('shows a failed repair with its next steps as an error', async () => {
      const s = setup('demo');
      mock = s.mock;
      await flush();
      vi.useFakeTimers();
      const spy = vi.spyOn(mock, 'serviceInstall');
      spy.mockResolvedValueOnce({ ...base, repair: working });
      await s.store.load();
      spy.mockResolvedValueOnce({ ...base, repair: { state: 'failed', message: 'Open System Settings ▸ General ▸ Login Items.' } });
      await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS);
      expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'error' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Open System Settings ▸ General ▸ Login Items.');
    });

    it('repair calls the app command, reloads the state and brings the connection back', async () => {
      const s = setup('demo');
      mock = s.mock;
      await flush();
      vi.spyOn(mock, 'serviceInstall').mockResolvedValueOnce({ ...base });
      await s.store.load();
      expect(s.store.canRepair()).toBe(true);
      await s.store.repair();
      expect(mock.repairs).toBe(1);
      expect(s.store.working()).toBeNull();
      expect(s.store.registration().value).toMatchObject({ running: true, startFailed: false });
      expect(s.store.canRepair()).toBe(false);
    });

    it('reports a repair that could not start', async () => {
      const s = setup('demo');
      mock = s.mock;
      vi.spyOn(mock, 'repairService').mockRejectedValueOnce({ code: 'permissionDenied', message: 'switched off' });
      await s.store.repair();
      expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'error' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Repair did not start');
    expect(s.toasts.notices.at(-1)?.message).toContain('switched off');
      expect(s.store.working()).toBeNull();
    });

    it('canRepair is off when the item is switched off in Login Items, on Windows, or while a repair works', async () => {
      const s = setup('demo');
      mock = s.mock;
      vi.useFakeTimers();
      const spy = vi.spyOn(mock, 'serviceInstall');
      for (const [patch, expected] of [
        [{ disabledInLoginItems: true }, false],
        [{ platform: 'windows' as const }, false],
        [{ repair: working }, false],
        [{ running: true }, false],
        [{}, true],
      ] as const) {
        spy.mockResolvedValueOnce({ ...base, ...patch });
        await s.store.load();
        expect(s.store.canRepair()).toBe(expected);
      }
    });

    it('keeps looking while it is down on macOS, a bounded number of times', async () => {
      const s = setup('demo');
      mock = s.mock;
      await flush();
      vi.useFakeTimers();
      const spy = vi.spyOn(mock, 'serviceInstall').mockResolvedValue({ ...base });
      await s.store.load();
      await vi.advanceTimersByTimeAsync(DOWN_POLL_MS * (MAX_DOWN_CHECKS + 5));
      expect(spy).toHaveBeenCalledTimes(MAX_DOWN_CHECKS + 1);
    });

    it('opens Login Items, and reports when that fails', async () => {
      const s = setup('demo');
      mock = s.mock;
      await s.store.openLoginItems();
      expect(mock.loginItemsOpened).toBe(1);
      vi.spyOn(mock, 'openLoginItems').mockRejectedValueOnce({ code: 'internal', message: 'no pane' });
      await s.store.openLoginItems();
      expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'error' });
    expect(s.toasts.notices.at(-1)?.message).toContain('no pane');
    });
  });

  it('uninstalls', async () => {
    const s = setup('demo');
    mock = s.mock;
    await flush();
    await s.store.uninstall();
    expect(s.toasts.notices.at(-1)?.message).toContain('The Pitcrew service was removed');
    expect(s.store.registration().value?.installed).toBe(false);
  });

  it('shows a cancelled prompt neutrally and other failures as errors', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    vi.spyOn(mock, 'installService').mockRejectedValueOnce({ code: 'cancelled', message: 'declined' });
    await s.store.install();
    expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'info' });
    expect(s.toasts.notices.at(-1)?.message).toContain('Cancelled');
    expect(s.toasts.notices.at(-1)?.message).toContain('declined');
    vi.spyOn(mock, 'installService').mockRejectedValueOnce({ code: 'permissionDenied', message: 'no rights' });
    await s.store.install();
    expect(s.toasts.notices.at(-1)).toMatchObject({ level: 'error' });
    expect(s.toasts.notices.at(-1)?.message).toContain('That did not work');
    expect(s.toasts.notices.at(-1)?.message).toContain('no rights');
    expect(s.store.working()).toBeNull();
  });
});
