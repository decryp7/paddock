import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { ThemeService } from '../core/theme.service';
import { makeMock } from '../../testing/test-helpers';
import { SettingsStore } from './settings.store';
import { ToastStore } from './toast.store';

function setup() {
  localStorage.clear();
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { backend, store: TestBed.inject(SettingsStore), toasts: TestBed.inject(ToastStore), theme: TestBed.inject(ThemeService) };
}

const down = { code: 'unavailable', message: 'down', problems: ['p1'] };

describe('SettingsStore', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads settings, app info, renderer, updates and autostart together', async () => {
    const { store } = setup();
    await store.loadAll();
    expect(store.settings().value?.theme).toBe('system');
    expect(store.info().value?.version).toBe('1.0.0');
    expect(store.renderer().value?.kind).toBe('none');
    expect(store.update().value?.available).toBe(true);
    expect(store.autostart()).toBe(false);
  });

  it('keeps going when autostart cannot be read and shows each panel error separately', async () => {
    const { backend, store } = setup();
    vi.spyOn(backend, 'getAutostart').mockRejectedValue(new Error('x'));
    vi.spyOn(backend, 'appInfo').mockRejectedValue(down);
    await store.loadAll();
    expect(store.autostart()).toBeNull();
    expect(store.info().error?.message).toBe('down');
    expect(store.settings().value).not.toBeNull();
  });

  it('applies the loaded theme', async () => {
    const { backend, store, theme } = setup();
    await backend.saveSettings({ ...(await backend.getSettings()), theme: 'dark' });
    await store.loadSettings();
    expect(theme.preference()).toBe('dark');
  });

  it('drops a read that is older than a change made while it ran', async () => {
    const { backend, store } = setup();
    await store.loadSettings();
    const real = backend.getSettings.bind(backend);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'getSettings').mockImplementationOnce(async () => {
      const old = await real();
      await gate;
      return old;
    });
    const slow = store.loadSettings();
    await store.change({ notifications: false });
    release();
    await slow;
    expect(store.settings().value?.notifications).toBe(false);
  });

  it('saves a change and shows the engine reply, applying the theme at once', async () => {
    const { backend, store, theme } = setup();
    await store.loadSettings();
    const p = store.change({ theme: 'dark' });
    expect(store.settings().value?.theme).toBe('dark');
    expect(theme.preference()).toBe('dark');
    await p;
    expect((await backend.getSettings()).theme).toBe('dark');
    expect(store.problems()).toEqual([]);
  });

  it('ignores a change before settings are loaded', async () => {
    const { backend, store } = setup();
    await store.change({ notifications: false });
    expect(backend.calls).toEqual([]);
  });

  it('puts the saved values back and keeps the problems when the engine refuses', async () => {
    const { store, toasts, theme } = setup();
    await store.loadSettings();
    await store.change({ theme: 'dark', defaultIntervalSecs: 5 });
    expect(store.settings().value?.defaultIntervalSecs).toBe(6 * 3600);
    expect(store.settings().value?.theme).toBe('system');
    expect(theme.preference()).toBe('system');
    expect(store.problems()).toEqual(['Use at least one minute.']);
    expect(toasts.toasts().at(-1)).toMatchObject({ title: 'Settings were not saved', tone: 'danger' });
  });

  it('lets the newest of two overlapping changes win, in success and failure', async () => {
    const { backend, store } = setup();
    await store.loadSettings();
    const real = backend.saveSettings.bind(backend);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'saveSettings').mockImplementationOnce(async (s) => {
      await gate;
      return real(s);
    });
    const first = store.change({ notifications: false });
    await store.change({ checkAppUpdates: false });
    release();
    await first;
    expect(store.settings().value).toMatchObject({ checkAppUpdates: false });

    vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce(down).mockResolvedValueOnce(await real({ ...store.settings().value!, digest: 'daily' } as never));
    const failing = store.change({ digest: 'weekly' } as never);
    await store.change({ digest: 'daily' } as never);
    await failing;
    expect(store.settings().value?.digest).toBe('daily');
  });

  it('pauses and resumes notifications with a toast, or reports the failure', async () => {
    const { backend, store, toasts } = setup();
    await store.pauseNotifications(30);
    expect(store.settings().value?.notificationsPausedUntilMs).not.toBeNull();
    expect(toasts.toasts().at(-1)?.title).toBe('Notifications paused');
    await store.pauseNotifications(null);
    expect(toasts.toasts().at(-1)?.title).toBe('Notifications resumed');
    vi.spyOn(backend, 'pauseNotifications').mockRejectedValue(down);
    await store.pauseNotifications(5);
    expect(toasts.toasts().at(-1)).toMatchObject({ title: 'Could not change notifications', body: 'down' });
  });

  it('sets autostart, or reports the failure', async () => {
    const { backend, store, toasts } = setup();
    await store.setAutostart(true);
    expect(store.autostart()).toBe(true);
    vi.spyOn(backend, 'setAutostart').mockRejectedValue(down);
    await store.setAutostart(false);
    expect(store.autostart()).toBe(true);
    expect(toasts.toasts().at(-1)?.title).toBe('Could not change launch at login');
  });

  it('opens targets and reports a failure', async () => {
    const { backend, store, toasts } = setup();
    await store.open('logs');
    expect(backend.calls).toContain('open:logs');
    vi.spyOn(backend, 'open').mockRejectedValue(down);
    await store.open('config');
    expect(toasts.toasts().at(-1)?.title).toBe('Could not open it');
  });

  it('chooses the downloads folder, or does nothing when cancelled or failing', async () => {
    const { backend, store, toasts } = setup();
    await store.loadSettings();
    await store.chooseDownloads();
    expect(store.settings().value?.downloadsFolder).toContain('Spotter-demo');
    vi.spyOn(backend, 'chooseFolder').mockResolvedValueOnce(null);
    await store.chooseDownloads();
    expect(backend.calls.filter((c) => c === 'saveSettings')).toHaveLength(1);
    vi.spyOn(backend, 'chooseFolder').mockRejectedValueOnce(down);
    await store.chooseDownloads();
    expect(toasts.toasts().at(-1)?.title).toBe('Could not open the picker');
  });

  it('installs the renderer, with busy state and failure', async () => {
    const { backend, store, toasts } = setup();
    const p = store.installRenderer();
    expect(store.rendererBusy()).toBe(true);
    await p;
    expect(store.rendererBusy()).toBe(false);
    expect(store.renderer().value?.kind).toBe('chromium-downloaded');
    expect(toasts.toasts().at(-1)?.title).toBe('Browser installed');
    vi.spyOn(backend, 'installRenderer').mockRejectedValue(down);
    await store.installRenderer();
    expect(store.rendererBusy()).toBe(false);
    expect(toasts.toasts().at(-1)?.title).toBe('The browser could not be installed');
  });

  it('checks and installs app updates, resetting busy state on failure', async () => {
    const { backend, store, toasts } = setup();
    await store.checkUpdate();
    expect(store.updateCheck()?.updateAvailable).toBe(true);
    expect(store.updateBusy()).toBe(false);
    await store.installUpdate();
    expect(backend.calls).toContain('installAppUpdate');
    vi.spyOn(backend, 'checkAppUpdate').mockRejectedValue(down);
    vi.spyOn(backend, 'installAppUpdate').mockRejectedValue(down);
    await store.checkUpdate();
    await store.installUpdate();
    expect(store.updateBusy()).toBe(false);
    expect(toasts.toasts().map((t) => t.title)).toEqual(['Could not check for updates', 'The update was not installed']);
  });
});
