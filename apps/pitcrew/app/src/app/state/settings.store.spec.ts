import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { SettingsStore } from './settings.store';
import { ToastStore } from './toast.store';

function setup(scenario: 'demo' | 'unavailable' = 'demo') {
  const mock = makeMock({ scenario });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(SettingsStore), toasts: TestBed.inject(ToastStore) };
}

describe('SettingsStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('has defaults before loading', () => {
    const s = setup();
    mock = s.mock;
    expect(s.store.config()).toBeNull();
    expect(s.store.readOnly()).toBe(false);
    expect(s.store.problems()).toEqual([]);
    expect(s.store.runnerConfig('build-win')).toBeNull();
  });

  it('loads the config, problems and read-only flag', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load();
    expect(s.store.config()?.runners).toHaveLength(4);
    expect(s.store.problems()).toHaveLength(1);
    expect(s.store.runnerConfig('docker-lab')?.watchdog?.threshold).toBe(4);
    expect(s.store.runnerConfig('nope')).toBeNull();
  });

  it('keeps an error when loading fails', async () => {
    const s = setup('unavailable');
    mock = s.mock;
    await s.store.load();
    expect(s.store.settings().error?.code).toBe('serviceUnavailable');
    expect(s.store.config()).toBeNull();
  });

  it('saves, toasts and reloads', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load();
    const config = structuredClone(s.store.config()!);
    config.defaults.logs.keep = 9;
    expect(await s.store.save(config)).toBe(true);
    expect(s.store.saving()).toBe(false);
    expect(s.toasts.toasts().at(-1)?.title).toBe('Settings saved');
    expect(s.store.config()?.defaults.logs.keep).toBe(9);
    expect(await s.store.save(config, 'Custom')).toBe(true);
    expect(s.toasts.toasts().at(-1)?.title).toBe('Custom');
  });

  it('reports a refused save', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load();
    const config = structuredClone(s.store.config()!);
    config.defaults.watchdog.threshold = 0;
    expect(await s.store.save(config)).toBe(false);
    expect(s.toasts.toasts().at(-1)).toMatchObject({ tone: 'danger', title: 'The settings were not saved' });
    expect(s.store.saving()).toBe(false);
  });

  it('saves one runner changed by a function without touching the others', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load();
    const ok = await s.store.saveRunner('ui-tests', (r) => ({ ...r, labels: ['only'] }), 'Labels saved');
    expect(ok).toBe(true);
    await flush();
    expect(s.store.runnerConfig('ui-tests')?.labels).toEqual(['only']);
    expect(s.store.runnerConfig('build-win')?.labels).toEqual(['windows', 'x64', 'msvc']);
    expect(s.toasts.toasts().at(-1)?.title).toBe('Labels saved');
  });

  it('refuses to save a runner before the config is loaded', async () => {
    const s = setup();
    mock = s.mock;
    expect(await s.store.saveRunner('ui-tests', (r) => r, 'x')).toBe(false);
  });
});
