import { TestBed } from '@angular/core/testing';
import { provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { makeMock } from '../../testing/test-helpers';
import { SettingsStore } from './settings.store';

function setup() {
  localStorage.clear();
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, ...provideShellTesting()] });
  return { backend, store: TestBed.inject(SettingsStore), notices: () => shellPorts().notices };
}

const down = { code: 'unavailable', message: 'down', problems: ['p1'] };

describe('SettingsStore', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads settings, app info, renderer and autostart together', async () => {
    const { store } = setup();
    await store.loadAll();
    expect(store.settings().value?.notifications).toBe(true);
    expect(store.info().value?.version).toBe('1.0.0');
    expect(store.renderer().value?.kind).toBe('none');
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

  it('saves a change and shows it at once, then the engine reply', async () => {
    const { backend, store } = setup();
    await store.loadSettings();
    const p = store.change({ digest: 'daily' });
    expect(store.settings().value?.digest).toBe('daily');
    await p;
    expect((await backend.getSettings()).digest).toBe('daily');
    expect(store.problems()).toEqual([]);
  });

  it('ignores a change before settings are loaded', async () => {
    const { backend, store } = setup();
    await store.change({ notifications: false });
    expect(backend.calls).toEqual([]);
  });

  it('puts the saved values back and keeps the problems when the engine refuses', async () => {
    const { store, notices } = setup();
    await store.loadSettings();
    await store.change({ digest: 'daily', defaultIntervalSecs: 5 });
    expect(store.settings().value?.defaultIntervalSecs).toBe(6 * 3600);
    expect(store.settings().value?.digest).toBe('off');
    expect(store.problems()).toEqual(['Use at least one minute.']);
    expect(notices().at(-1)).toMatchObject({ level: 'error', message: 'Settings were not saved. The default interval is too short.' });
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
    await store.change({ allowOsWebview: true });
    release();
    await first;
    expect(store.settings().value).toMatchObject({ allowOsWebview: true });

    vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce(down).mockResolvedValueOnce(await real({ ...store.settings().value!, digest: 'daily' } as never));
    const failing = store.change({ digest: 'weekly' } as never);
    await store.change({ digest: 'daily' } as never);
    await failing;
    expect(store.settings().value?.digest).toBe('daily');
  });

  it('pauses and resumes notifications with a toast, or reports the failure', async () => {
    const { backend, store, notices } = setup();
    await store.pauseNotifications(30);
    expect(store.settings().value?.notificationsPausedUntilMs).not.toBeNull();
    expect(notices().at(-1)?.message).toBe('Notifications paused');
    await store.pauseNotifications(null);
    expect(notices().at(-1)?.message).toBe('Notifications resumed');
    vi.spyOn(backend, 'pauseNotifications').mockRejectedValue(down);
    await store.pauseNotifications(5);
    expect(notices().at(-1)).toMatchObject({ level: 'error', message: 'Could not change notifications. down' });
  });

  it('sets autostart, or reports the failure', async () => {
    const { backend, store, notices } = setup();
    await store.setAutostart(true);
    expect(store.autostart()).toBe(true);
    vi.spyOn(backend, 'setAutostart').mockRejectedValue(down);
    await store.setAutostart(false);
    expect(store.autostart()).toBe(true);
    expect(notices().at(-1)?.message).toBe('Could not change launch at login. down');
  });

  it('opens targets and reports a failure', async () => {
    const { backend, store, notices } = setup();
    await store.open('logs');
    expect(backend.calls).toContain('open:logs');
    vi.spyOn(backend, 'open').mockRejectedValue(down);
    await store.open('config');
    expect(notices().at(-1)?.message).toBe('Could not open it. down');
  });

  it('chooses the downloads folder, or does nothing when cancelled or failing', async () => {
    const { backend, store, notices } = setup();
    await store.loadSettings();
    await store.chooseDownloads();
    expect(store.settings().value?.downloadsFolder).toContain('Downloads/Spotter');
    vi.spyOn(backend, 'chooseFolder').mockResolvedValueOnce(null);
    await store.chooseDownloads();
    expect(backend.calls.filter((c) => c === 'saveSettings')).toHaveLength(1);
    vi.spyOn(backend, 'chooseFolder').mockRejectedValueOnce(down);
    await store.chooseDownloads();
    expect(notices().at(-1)?.message).toBe('Could not open the picker. down');
  });

  it('installs the renderer, with busy state and failure', async () => {
    const { backend, store, notices } = setup();
    const p = store.installRenderer();
    expect(store.rendererBusy()).toBe(true);
    await p;
    expect(store.rendererBusy()).toBe(false);
    expect(store.renderer().value?.kind).toBe('chromium-downloaded');
    expect(notices().at(-1)?.message).toBe('Browser installed');
    vi.spyOn(backend, 'installRenderer').mockRejectedValue(down);
    await store.installRenderer();
    expect(store.rendererBusy()).toBe(false);
    expect(notices().at(-1)?.message).toBe('The browser could not be installed. down');
  });

  it('a cancelled installation says nothing was installed instead of failing', async () => {
    const { backend, store, notices } = setup();
    vi.spyOn(backend, 'installRenderer').mockRejectedValue({ code: 'cancelled', message: 'Cancelled.', problems: [] });
    await store.installRenderer();
    expect(store.rendererBusy()).toBe(false);
    expect(notices().at(-1)).toMatchObject({ level: 'info', message: 'Installation cancelled. Nothing was installed.' });
  });
});
