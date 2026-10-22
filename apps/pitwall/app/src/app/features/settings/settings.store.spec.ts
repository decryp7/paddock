import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { demoSettings } from '../../core/mock-data';
import { SettingsStore, editSettings } from './settings.store';

describe('editSettings', () => {
  it('merges values into one section and leaves the others alone', () => {
    const s = demoSettings();
    const next = editSettings(s, 'server', { port: 12000 });
    expect(next.server.port).toBe(12000);
    expect(next.server.maxConcurrent).toBe(s.server.maxConcurrent);
    expect(next.network).toEqual(s.network);
    expect(s.server.port).toBe(11500);
  });

  it('makes the preset custom when a memory value changes, but not when the preset itself is set', () => {
    const s = demoSettings();
    expect(editSettings(s, 'memory', { maxHotModels: 2 }).memory.preset).toBe('custom');
    expect(editSettings(s, 'memory', { preset: 'performance' }).memory.preset).toBe('performance');
    expect(editSettings(s, 'general', { startAtLogin: false }).memory.preset).toBe('balanced');
  });
});

describe('SettingsStore', () => {
  function setup(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [SettingsStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(SettingsStore), toasts: TestBed.inject(Toasts), backend };
  }

  it('is empty and clean before loading', () => {
    const { store } = setup();
    expect(store.saved()).toBeNull();
    expect(store.draft()).toBeNull();
    expect(store.dirty()).toBe(false);
  });

  it('loads settings, a separate draft copy and the update status', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.loading()).toBe(false);
    expect(store.saved()?.server.port).toBe(11500);
    expect(store.draft()).toEqual(store.saved());
    expect(store.draft()).not.toBe(store.saved());
    expect(store.update()?.available).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('reports a load error and keeps going without update status', async () => {
    const { store } = setup(new MockBackend({ failing: ['settings', 'appUpdateStatus'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.draft()).toBeNull();
    expect(store.update()).toBeNull();
    expect(store.loading()).toBe(false);
  });

  it('keeps the settings when only the update check fails', async () => {
    const { store } = setup(new MockBackend({ failing: ['appUpdateStatus'] }));
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.draft()).not.toBeNull();
    expect(store.update()).toBeNull();
  });

  it('tracks dirty state across edits and discard', async () => {
    const { store } = setup();
    await store.load();
    store.edit('general', { startAtLogin: false });
    expect(store.dirty()).toBe(true);
    expect(store.saved()?.general.startAtLogin).toBe(true);
    store.discard();
    expect(store.dirty()).toBe(false);
    expect(store.draft()?.general.startAtLogin).toBe(true);
    store.edit('general', { startAtLogin: false });
    store.edit('general', { startAtLogin: true });
    expect(store.dirty()).toBe(false);
  });

  it('ignores edits before load and discard with nothing saved', () => {
    const { store } = setup();
    store.edit('server', { port: 2000 });
    expect(store.draft()).toBeNull();
    store.discard();
    expect(store.draft()).toBeNull();
  });

  it('saves the draft, adopts what the server kept and toasts', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.edit('server', { port: 12000 });
    expect(await store.save()).toBe(true);
    expect(store.saved()?.server.port).toBe(12000);
    expect(store.dirty()).toBe(false);
    expect(store.saving()).toBe(false);
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'ok', message: 'Settings saved.' });
  });

  it('does not save before load or while a save is running', async () => {
    const { store } = setup();
    expect(await store.save()).toBe(false);
    await store.load();
    store.edit('server', { port: 12001 });
    const first = store.save();
    expect(await store.save()).toBe(false);
    expect(await first).toBe(true);
  });

  it('keeps the draft and shows the error when the server refuses a save', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.edit('server', { port: 80 });
    expect(await store.save()).toBe(false);
    expect(store.saveError()?.code).toBe('badRequest');
    expect(store.draft()?.server.port).toBe(80);
    expect(store.dirty()).toBe(true);
    expect(store.saving()).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    store.edit('server', { port: 12000 });
    expect(store.saveError()).toBeNull();
  });

  it('reports a failing save call', async () => {
    const { store } = setup(new MockBackend({ failing: ['saveSettings'] }));
    await store.load();
    store.edit('general', { startAtLogin: false });
    expect(await store.save()).toBe(false);
    expect(store.saveError()?.code).toBe('serverUnavailable');
    store.discard();
    expect(store.saveError()).toBeNull();
  });

  it('applies a preset: server memory values are shown and other unsaved edits stay', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.edit('general', { startAtLogin: false });
    await store.applyPreset('lowMemory');
    expect(store.draft()?.memory.preset).toBe('lowMemory');
    expect(store.draft()?.memory.maxHotModels).toBe(1);
    expect(store.draft()?.general.startAtLogin).toBe(false);
    expect(store.saved()?.memory.preset).toBe('lowMemory');
    expect(toasts.toasts().at(-1)?.message).toBe('Low memory preset applied.');
  });

  it('applies a preset before the draft exists by adopting the saved settings', async () => {
    const { store } = setup();
    await store.applyPreset('performance');
    expect(store.draft()?.memory.preset).toBe('performance');
  });

  it('reports a failing preset', async () => {
    const { store, toasts } = setup(new MockBackend({ failing: ['applyPreset'] }));
    await store.load();
    await store.applyPreset('balanced');
    expect(store.saving()).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
  });

  it('ignores a second preset while one is being applied', async () => {
    const { store } = setup();
    await store.load();
    const first = store.applyPreset('performance');
    await store.applyPreset('lowMemory');
    await first;
    expect(store.saved()?.memory.preset).toBe('performance');
  });
});
