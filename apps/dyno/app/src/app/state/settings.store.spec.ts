import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { SettingsStore } from './settings.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(SettingsStore), notifier: TestBed.inject(Notifier) };
}

describe('SettingsStore', () => {
  it('load fetches the settings (the appearance is the shell’s, not part of them)', async () => {
    const { store, mock } = setup();
    const s = await mock.settings();
    vi.spyOn(mock, 'settings').mockResolvedValue(s);
    await store.load();
    expect(store.settings()).toEqual(s);
    expect(store.error()).toBeNull();
  });
  it('load failure sets the error', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'settings').mockRejectedValue({ code: 'internal', message: 'bad' });
    await store.load();
    expect(store.error()).toEqual({ code: 'internal', message: 'bad' });
    expect(store.settings()).toBeNull();
  });
  it('loadPlugins fills or fails', async () => {
    const { store, mock } = setup();
    await store.loadPlugins();
    expect(store.pluginsLoaded()).toBe(true);
    expect(store.plugins().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'plugins').mockRejectedValue({ code: 'busy', message: 'b' });
    await store.loadPlugins();
    expect(store.pluginsError()).toEqual({ code: 'busy', message: 'b' });
  });
  it('save stores what the engine returns and tells the user', async () => {
    const { store, mock, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 2000;
    expect(await store.save(s)).toBe(true);
    expect(store.settings()?.sampling.intervalMs).toBe(2000);
    expect(store.saving()).toBe(false);
    expect(notifier.recent().at(-1)?.text).toBe('Settings saved');
  });
  it('a save taken before another change is refused as stale: the store reloads and says so, and nothing is lost', async () => {
    const { store, mock, notifier } = setup();
    await store.load();
    const s0 = structuredClone(store.settings()!);
    // Another window (or the alert-rule editor) changes the settings after this window read them.
    await mock.upsertAlertRule({ id: 'r1', name: 'R1', metric: 'cpuTotal', op: 'above', limit: 90, forSecs: 0, severity: 'warning', enabled: true } as never);
    const loads = vi.spyOn(mock, 'settings');
    const changed = structuredClone(s0);
    changed.sampling.intervalMs = 2000;
    expect(await store.save(changed)).toBe(false);
    expect(loads).toHaveBeenCalled();
    expect(store.settings()?.customAlerts?.map((r) => r.id)).toContain('r1');
    expect(store.settings()?.sampling.intervalMs).not.toBe(2000);
    expect(store.saving()).toBe(false);
    const last = notifier.recent().at(-1);
    expect(last?.text).toMatch(/changed.*reloaded/i);
    expect(last?.tone).toBe('warn');
    // The reloaded copy saves.
    changed.revision = store.settings()!.revision;
    expect(await store.save(changed)).toBe(true);
    expect(store.settings()?.sampling.intervalMs).toBe(2000);
    expect(store.settings()?.customAlerts?.map((r) => r.id)).toContain('r1');
  });
  it('two quick saves from one window do not refuse each other as another window', async () => {
    const { store, mock } = setup();
    await store.load();
    const first = structuredClone(store.settings()!);
    first.sampling.intervalMs = 2000;
    const second = structuredClone(first);
    second.launchAtLogin = !second.launchAtLogin;
    const both = await Promise.all([store.save(first, null), store.save(second, null)]);
    expect(both).toEqual([true, true]);
    expect(store.settings()?.launchAtLogin).toBe(second.launchAtLogin);
    expect((await mock.settings()).revision).toBe(first.revision + 2);
  });
  it('two quick saves taken from the same base that change different fields keep both changes', async () => {
    const { store } = setup();
    await store.load();
    const base = store.settings()!;
    const first = structuredClone(base);
    first.sampling.intervalMs = 2000;
    const second = structuredClone(base);
    second.launchAtLogin = !second.launchAtLogin;
    expect(await Promise.all([store.save(first, null), store.save(second, null)])).toEqual([true, true]);
    expect(store.settings()?.sampling.intervalMs).toBe(2000);
    expect(store.settings()?.launchAtLogin).toBe(second.launchAtLogin);
  });
  it('the stale notice only says what the store did: it reloaded', async () => {
    const { store, mock, notifier } = setup();
    await store.load();
    const s0 = structuredClone(store.settings()!);
    await mock.upsertAlertRule({ id: 'r1', name: 'R1', metric: 'cpuTotal', op: 'above', limit: 90, forSecs: 0, severity: 'warning', enabled: true } as never);
    expect(await store.save(s0)).toBe(false);
    expect(notifier.recent().at(-1)?.text).toBe('The settings changed in another window and were reloaded. Try again.');
  });
  it('a stale refusal keeps what the window tried to save for the page to carry over', async () => {
    const { store, mock } = setup();
    await store.load();
    const s0 = structuredClone(store.settings()!);
    await mock.upsertAlertRule({ id: 'r1', name: 'R1', metric: 'cpuTotal', op: 'above', limit: 90, forSecs: 0, severity: 'warning', enabled: true } as never);
    const attempted = structuredClone(s0);
    attempted.launchAtLogin = !attempted.launchAtLogin;
    expect(await store.save(attempted)).toBe(false);
    expect(store.stale()?.attempted).toEqual(attempted);
    expect(store.stale()?.base?.revision).toBe(s0.revision);
    store.clearStale();
    expect(store.stale()).toBeNull();
  });
  it('save without a message stays quiet', async () => {
    const { store, mock, notifier } = setup();
    expect(await store.save(structuredClone(await mock.settings()), null)).toBe(true);
    expect(notifier.recent()).toEqual([]);
  });
  it('save rejects invalid settings with the engine message', async () => {
    const { store, mock, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 10;
    expect(await store.save(s)).toBe(false);
    expect(store.saving()).toBe(false);
    expect(store.settings()).toBeNull();
    expect(notifier.recent().at(-1)).toMatchObject({ tone: 'bad', text: expect.stringContaining('250 ms') });
  });
  it('a failed save keeps the error, and the next good save clears it', async () => {
    const { store, mock } = setup();
    await store.load();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 10;
    expect(await store.save(s)).toBe(false);
    expect(store.saveError()?.message).toContain('250 ms');
    expect(await store.save(structuredClone(await mock.settings()))).toBe(true);
    expect(store.saveError()).toBeNull();
  });
});
