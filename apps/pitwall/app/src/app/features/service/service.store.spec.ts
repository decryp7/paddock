import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { DOWN_POLL_MS, MAX_DOWN_CHECKS, REPAIR_POLL_MS, ServiceStore } from './service.store';

describe('ServiceStore', () => {
  function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}, backend?: PitwallBackend) {
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend ?? new MockBackend(options) }] });
    return { store: TestBed.inject(ServiceStore), toasts: TestBed.inject(Toasts) };
  }

  it('knows nothing before the first refresh and does not ask', () => {
    const { store } = setup();
    expect(store.service()).toBeNull();
    expect(store.autostart()).toBeNull();
    expect(store.askConsent()).toBe(false);
  });

  it('refresh reads the service, start at log-in and the update status', async () => {
    const { store } = setup();
    await store.refresh();
    expect(store.service()?.installed).toBe(true);
    expect(store.autostart()).toBe(false);
    expect(store.updateStatus()?.available).toBe(false);
    expect(store.askConsent()).toBe(false);
  });

  it('asks the first-run question when the engine says the user was not asked', async () => {
    const { store } = setup({ askConsent: true });
    await store.refresh();
    expect(store.askConsent()).toBe(true);
  });

  it('one failing part does not hide the others', async () => {
    const { store } = setup({ failing: ['serviceState'] });
    await store.refresh();
    expect(store.service()).toBeNull();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.autostart()).toBe(false);
    expect(store.updateStatus()).not.toBeNull();
  });

  it('keeps the last known values when a later refresh fails', async () => {
    const backend = new MockBackend();
    const { store } = setup({}, backend);
    await store.refresh();
    const failing = new MockBackend({ failing: ['serviceState', 'autostart', 'appUpdateStatus'] });
    TestBed.resetTestingModule();
    const again = setup({}, failing).store;
    await again.refresh();
    expect(again.service()).toBeNull();
    expect(store.service()).not.toBeNull();
  });

  it('install registers the service, clears the question and says so', async () => {
    const { store, toasts } = setup({ askConsent: true });
    await store.refresh();
    expect(await store.install()).toBe(true);
    expect(store.service()).toMatchObject({ installed: true, running: true, askConsent: false });
    expect(store.askConsent()).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toBe('Pitwall now runs as a service.');
  });

  it('install that leaves the server not yet answering says it is starting', async () => {
    const backend = new MockBackend({ askConsent: true });
    vi.spyOn(backend, 'installService').mockResolvedValue({ platform: 'linux', installed: true, running: false, canInstall: true, askConsent: false, message: null, repair: null, disabledInLoginItems: false, startFailed: false, notInLoginItems: false });
    const { store, toasts } = setup({}, backend);
    await store.install();
    expect(toasts.toasts().at(-1)?.message).toBe('The service is installed; it is starting.');
  });

  it('a failed install keeps the state, records the error and toasts it', async () => {
    const { store, toasts } = setup({ askConsent: true, failing: ['installService'] });
    await store.refresh();
    expect(await store.install()).toBe(false);
    expect(store.service()?.installed).toBe(false);
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.isBusy()('install')).toBe(false);
  });

  it('ignores a second install while one is running', async () => {
    const { store } = setup({ latencyMs: 5 });
    const first = store.install();
    expect(store.isBusy()('install')).toBe(true);
    expect(await store.install()).toBe(false);
    expect(await first).toBe(true);
  });

  it('uninstall removes it', async () => {
    const { store, toasts } = setup();
    await store.refresh();
    expect(await store.uninstall()).toBe(true);
    expect(store.service()?.installed).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toBe('The service was removed.');
  });

  it('decline stops the question for good, later only for now', async () => {
    const a = setup({ askConsent: true });
    await a.store.refresh();
    await a.store.decline();
    expect(a.store.askConsent()).toBe(false);
    TestBed.resetTestingModule();
    const b = setup({ askConsent: true });
    await b.store.refresh();
    b.store.later();
    expect(b.store.askConsent()).toBe(false);
  });

  it('decline and later before the first answer do nothing', async () => {
    const { store } = setup();
    await store.decline();
    store.later();
    expect(store.service()).toBeNull();
  });

  it('turns start at log-in on and off from what the OS reports', async () => {
    const { store } = setup();
    await store.setAutostart(true);
    expect(store.autostart()).toBe(true);
    await store.setAutostart(false);
    expect(store.autostart()).toBe(false);
  });

  it('keeps start at log-in as it was when the change fails', async () => {
    const { store, toasts } = setup({ failing: ['setAutostart'] });
    await store.refresh();
    await store.setAutostart(true);
    expect(store.autostart()).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
  });

  it('checks for an update only when asked and installs the one it found', async () => {
    const { store } = setup({ updateAvailable: true });
    await store.refresh();
    expect(store.update()).toBeNull();
    await store.checkUpdate();
    expect(store.update()?.version).toBe('0.2.0');
    expect(store.upToDate()).toBe(false);
    await store.applyUpdate();
    expect(store.error()).toBeNull();
  });

  it('says up to date when the check finds nothing', async () => {
    const backend = new MockBackend({ updateAvailable: true });
    vi.spyOn(backend, 'checkAppUpdate').mockResolvedValue(null);
    const { store } = setup({}, backend);
    await store.checkUpdate();
    expect(store.update()).toBeNull();
    expect(store.upToDate()).toBe(true);
  });

  it('a failed check records the error and installing without a found update does nothing', async () => {
    const { store } = setup();
    await store.checkUpdate();
    expect(store.error()?.code).toBe('notAvailable');
    await store.applyUpdate();
    expect(store.isBusy()('apply')).toBe(false);
  });

  describe('macOS Login Items and the repair', () => {
    const working = { state: 'working' as const, message: 'Re-registering…' };

    it('opens Login Items through the backend and counts it', async () => {
      const backend = new MockBackend({ loginItemOff: true });
      const { store } = setup({}, backend);
      await store.refresh();
      expect(store.service()?.disabledInLoginItems).toBe(true);
      await store.openLoginItems();
      expect(backend.loginItemsOpened).toBe(1);
    });

    it('a failed open is toasted, not thrown', async () => {
      const { store, toasts } = setup({ failing: ['openLoginItems'] });
      await store.openLoginItems();
      expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    });

    it('reads again while the repair works and reports how it ended', async () => {
      vi.useFakeTimers();
      try {
        const backend = new MockBackend({ repairing: true, latencyMs: 0 });
        const { store, toasts } = setup({}, backend);
        const base = await backend.serviceState();
        const spy = vi.spyOn(backend, 'serviceState');
        spy.mockResolvedValueOnce({ ...base, repair: working });
        spy.mockResolvedValueOnce({ ...base, repair: { state: 'failed', message: 'Switch Pitwall off and on.' } });
        await store.refresh();
        expect(store.service()?.repair?.state).toBe('working');
        await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS);
        expect(store.service()?.repair?.state).toBe('failed');
        expect(toasts.toasts().at(-1)?.message).toContain('could not be re-registered');
        expect(toasts.toasts().at(-1)?.message).toContain('Switch Pitwall off and on.');
        spy.mockResolvedValueOnce({ ...base, repair: working });
        spy.mockResolvedValueOnce({ ...base, repair: { state: 'succeeded', message: 'Service is running again' } });
        await store.refresh();
        await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS);
        expect(toasts.toasts().at(-1)?.message).toBe('Service is running again');
        // Nothing is polled once the repair ended.
        const calls = spy.mock.calls.length;
        await vi.advanceTimersByTimeAsync(REPAIR_POLL_MS * 3);
        expect(spy.mock.calls.length).toBe(calls);
      } finally {
        vi.useRealTimers();
      }
    });

    it('Repair works with the server down, through the backend, and the state then shows the service running', async () => {
      const backend = new MockBackend({ serverDown: true, startFailed: true });
      const { store } = setup({}, backend);
      await store.refresh();
      expect(store.service()).toMatchObject({ running: false, startFailed: true });
      expect(store.canRepair()).toBe(true);
      await store.repair();
      expect(backend.repairs).toBe(1);
      expect(store.service()).toMatchObject({ running: true, startFailed: false });
      expect(store.canRepair()).toBe(false);
    });

    it('a failed repair is toasted, not thrown', async () => {
      const { store, toasts } = setup({ failing: ['repairService'], startFailed: true });
      await store.refresh();
      await store.repair();
      expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    });

    it('nothing repairs what the user switched off in Login Items', async () => {
      const { store } = setup({ loginItemOff: true, serverDown: true });
      await store.refresh();
      expect(store.canRepair()).toBe(false);
    });

    it('keeps looking while macOS has the service down, a bounded number of times', async () => {
      vi.useFakeTimers();
      try {
        const backend = new MockBackend({ serverDown: true, startFailed: true, latencyMs: 0 });
        const { store } = setup({}, backend);
        const spy = vi.spyOn(backend, 'serviceState');
        await store.refresh();
        await vi.advanceTimersByTimeAsync(DOWN_POLL_MS * (MAX_DOWN_CHECKS + 5));
        expect(spy.mock.calls.length).toBe(MAX_DOWN_CHECKS + 1);
      } finally {
        vi.useRealTimers();
      }
    });
  });
});
