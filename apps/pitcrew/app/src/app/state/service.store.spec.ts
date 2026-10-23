import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { REPAIR_POLL_MS, ServiceStore } from './service.store';
import { StatusStore } from './status.store';
import { ToastStore } from './toast.store';

function setup(scenario: 'demo' | 'notInstalled' = 'notInstalled') {
  const mock = makeMock({ scenario });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ServiceStore), status: TestBed.inject(StatusStore), toasts: TestBed.inject(ToastStore) };
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
    expect(s.toasts.toasts().at(-1)?.title).toBe('The Pitcrew service is installed and running');
    expect(s.store.registration().value?.installed).toBe(true);
    expect(s.status.runners().length).toBe(4);
    expect(s.status.offline()).toBeNull();
  });

  describe('the macOS repair', () => {
    const base = { platform: 'macos', installed: true, running: false, canInstall: true, message: null, disabledInLoginItems: false } as const;
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
      expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'success', title: 'Service is running again' });
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
      expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'danger', body: 'Open System Settings ▸ General ▸ Login Items.' });
    });

    it('opens Login Items, and reports when that fails', async () => {
      const s = setup('demo');
      mock = s.mock;
      await s.store.openLoginItems();
      expect(mock.loginItemsOpened).toBe(1);
      vi.spyOn(mock, 'openLoginItems').mockRejectedValueOnce({ code: 'internal', message: 'no pane' });
      await s.store.openLoginItems();
      expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'danger', body: 'no pane' });
    });
  });

  it('uninstalls', async () => {
    const s = setup('demo');
    mock = s.mock;
    await flush();
    await s.store.uninstall();
    expect(s.toasts.toasts().at(-1)?.title).toBe('The Pitcrew service was removed');
    expect(s.store.registration().value?.installed).toBe(false);
  });

  it('shows a cancelled prompt neutrally and other failures as errors', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    vi.spyOn(mock, 'installService').mockRejectedValueOnce({ code: 'cancelled', message: 'declined' });
    await s.store.install();
    expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'neutral', title: 'Cancelled', body: 'declined' });
    vi.spyOn(mock, 'installService').mockRejectedValueOnce({ code: 'permissionDenied', message: 'no rights' });
    await s.store.install();
    expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'danger', title: 'That did not work', body: 'no rights' });
    expect(s.store.working()).toBeNull();
  });
});
