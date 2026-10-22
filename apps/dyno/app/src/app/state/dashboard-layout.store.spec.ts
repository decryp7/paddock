import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { DashboardLayoutStore } from './dashboard-layout.store';
import { SettingsStore } from './settings.store';

async function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  await TestBed.inject(SettingsStore).load();
  return { store: TestBed.inject(DashboardLayoutStore), settings: TestBed.inject(SettingsStore), mock };
}

describe('DashboardLayoutStore', () => {
  it('shows the saved layout and edits a draft until saved', async () => {
    const { store, settings } = await setup();
    expect(store.widgets().length).toBe(15);
    expect(store.editing()).toBe(false);
    store.startEdit();
    store.add('bars', 'swap');
    store.resize('cpu', 'l');
    store.remove('issues');
    store.move(0, 2);
    expect(store.dirty()).toBe(true);
    expect(store.widgets().find((w) => w.id === 'cpu')!.size).toBe('l');
    expect(settings.settings()!.dashboard!.widgets.length).toBe(15);
    expect(await store.save()).toBe(true);
    expect(store.editing()).toBe(false);
    expect(settings.settings()!.dashboard!.widgets.some((w) => w.kind === 'bars')).toBe(true);
  });

  it('cancels without saving, switches preset, resets, and limits the count', async () => {
    const { store } = await setup();
    store.startEdit();
    store.usePreset('gaming');
    expect(store.widgets().map((w) => w.id)).toContain('gpu');
    store.reset();
    expect(store.widgets().map((w) => w.id)).toContain('health');
    store.setMetric('cpu', 'memory');
    expect(store.widgets().find((w) => w.id === 'cpu')!.metric).toBe('memory');
    store.cancel();
    expect(store.editing()).toBe(false);
    expect(store.widgets().find((w) => w.id === 'cpu')!.metric).toBe('cpuTotal');
    store.startEdit();
    for (let i = 0; i < 40; i++) store.add('bigNumber', 'cpuTotal');
    expect(store.widgets().length).toBe(32);
    expect(store.canAdd()).toBe(false);
  });

  it('saves the current widgets as a named preset', async () => {
    const { store, settings } = await setup();
    store.startEdit();
    store.remove('issues');
    expect(await store.saveAsPreset('  My view ')).toBe(true);
    const p = settings.settings()!.dashboard!.presets.find((x) => x.id === 'custom-my-view')!;
    expect(p.builtin).toBe(false);
    expect(p.name).toBe('My view');
    store.startEdit();
    expect(await store.saveAsPreset('   ')).toBe(false);
  });

  it('does nothing without an edit session', async () => {
    const { store } = await setup();
    store.add('graph', 'cpuTotal');
    store.remove('cpu');
    expect(store.widgets().length).toBe(15);
    expect(await store.save()).toBe(false);
  });

  it('copes with settings that have no dashboard', async () => {
    const { store, settings, mock } = await setup();
    const s = { ...settings.settings()! };
    delete s.dashboard;
    await mock.saveSettings(s);
    await settings.load();
    expect(store.widgets()).toEqual([]);
  });
});
