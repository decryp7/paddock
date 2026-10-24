import { TestBed } from '@angular/core/testing';
import { SETTINGS_OVERVIEW_FIXTURE } from '../generated/settings-fixtures';
import type { SetResult, SettingsChanged, SettingsOverview } from '../generated/settings';
import { MemorySettingsBackend, SettingsBackend, type SettingsChanges, type Unlisten } from './settings-backend';
import { SettingsDrafts } from '../window/src/settings-drafts';
import { SettingsStore } from './settings.store';
import { settingsBed, settle } from '../window/src/spec-helpers';

/** A backend that fails on demand. */
class FlakyBackend extends SettingsBackend {
  readonly inner = new MemorySettingsBackend();
  failLoad = true;
  failListen = false;
  failChange = false;
  offs = 0;
  overview(): Promise<SettingsOverview> {
    return this.failLoad ? Promise.reject(new Error('the engine is gone')) : this.inner.overview();
  }
  set(section: string, changes: SettingsChanges): Promise<SetResult> {
    return this.failChange ? Promise.reject({ code: 'saveFailed', message: 'The disk is full.', retryable: true }) : this.inner.set(section, changes);
  }
  reset(section: string, key?: string): Promise<SetResult> {
    return this.failChange ? Promise.reject('no') : this.inner.reset(section, key);
  }
  listen(handler: (c: SettingsChanged) => void): Promise<Unlisten> {
    if (this.failListen) return Promise.reject(new Error('no events'));
    return this.inner.listen(handler).then((off) => () => {
      this.offs++;
      off();
    });
  }
}

describe('SettingsStore', () => {
  it('loads every section and applies a change with a notice', async () => {
    settingsBed();
    const store = TestBed.inject(SettingsStore);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.sections().map((s) => s.id)).toEqual(['pd.appearance', 'sd.general', 'sd.storage']);
    expect(await store.apply('sd.general', { repeat: 3 })).toBe(true);
    expect(store.section('sd.general')?.values['repeat']).toBe(3);
    expect(store.notice()).toBe('Repeat saved.');
    expect(await store.apply('sd.general', { repeat: 4, notify: false })).toBe(true);
    expect(store.notice()).toBe('General saved.');
    expect(await store.apply('sd.general', {})).toBe(true);
    expect(await store.apply('sd.none', { x: 1 })).toBe(true);
    store.dismissNotice();
    expect(store.notice()).toBeNull();
  });

  it('shows a page’s own footer notice through setNotice, and clears it again', async () => {
    settingsBed();
    const store = TestBed.inject(SettingsStore);
    await settle();
    store.setNotice('Folder moved.');
    expect(store.notice()).toBe('Folder moved.');
    store.setNotice(null);
    expect(store.notice()).toBeNull();
  });

  it('keeps the engine’s refusal with the value it refused', async () => {
    settingsBed();
    const store = TestBed.inject(SettingsStore);
    await settle();
    expect(await store.apply('sd.general', { greeting: 'Hi!' })).toBe(false);
    expect(store.refusalOf('sd.general', 'greeting', 'Hi!')).toBe('Does not have the expected format ([A-Za-z ,]+).');
    expect(store.refusalOf('sd.general', 'greeting', 'Hi')).toBeNull();
    expect(store.refusalOf('sd.general', 'repeat', 1)).toBeNull();
    expect(store.sectionRefusals('sd.general')).toEqual([]);
    expect(await store.apply('sd.general', { greeting: 'Hi' })).toBe(true);
    expect(store.refusalOf('sd.general', 'greeting', 'Hi!')).toBeNull();
  });

  it('counts restart-only changes and follows other windows', async () => {
    const { backend } = settingsBed();
    const store = TestBed.inject(SettingsStore);
    await settle();
    expect(store.restartPending()).toBe(false);
    await store.apply('sd.general', { volume: 0.8 });
    expect(store.restartPending()).toBe(true);
    expect(store.restartChanges()).toEqual(['Volume']);
    // Another window changes the theme: the event updates this window's values.
    await backend.set('pd.appearance', { theme: 'dark' });
    expect(store.section('pd.appearance')?.values['theme']).toBe('dark');
  });

  it('resets a field or a section', async () => {
    settingsBed();
    const store = TestBed.inject(SettingsStore);
    await settle();
    await store.apply('sd.general', { repeat: 5, greeting: 'Yo' });
    expect(await store.reset('sd.general', 'repeat')).toBe(true);
    expect(store.section('sd.general')?.values).toMatchObject({ repeat: 1, greeting: 'Yo' });
    expect(store.notice()).toBe('Repeat saved.');
    expect(await store.reset('sd.general')).toBe(true);
    expect(store.notice()).toBe('General reset to defaults.');
    expect(store.section('sd.general')?.values['greeting']).toBe('Hello');
    expect(await store.reset('sd.none')).toBe(false);
  });

  it('shows a failed load with Retry, and a failed change as an action error', async () => {
    const backend = new FlakyBackend();
    settingsBed({ backend });
    const store = TestBed.inject(SettingsStore);
    await settle();
    expect(store.requestError()).toBe('the engine is gone');
    backend.failLoad = false;
    await store.load();
    expect(store.isLoaded()).toBe(true);
    backend.failChange = true;
    expect(await store.apply('sd.general', { repeat: 2 })).toBe(false);
    expect(store.actionError()).toBe('The disk is full.');
    expect(await store.reset('sd.general')).toBe(false);
    expect(store.actionError()).toBe('no');
  });

  it('stops listening when the window goes, and still loads when events cannot be heard', async () => {
    const backend = new FlakyBackend();
    backend.failLoad = false;
    settingsBed({ backend });
    TestBed.inject(SettingsStore);
    await settle();
    TestBed.resetTestingModule();
    expect(backend.offs).toBe(1);

    const deaf = new FlakyBackend();
    deaf.failLoad = false;
    deaf.failListen = true;
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    settingsBed({ backend: deaf });
    const store = TestBed.inject(SettingsStore);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('cannot listen for settings changes: no events'));
    warn.mockRestore();
  });

  it('unlistens at once when the window went before listening started', async () => {
    let off = 0;
    let resolve!: (u: Unlisten) => void;
    const slow = new FlakyBackend();
    slow.failLoad = false;
    vi.spyOn(slow, 'listen').mockReturnValue(new Promise<Unlisten>((r) => (resolve = r)));
    settingsBed({ backend: slow });
    TestBed.inject(SettingsStore);
    TestBed.resetTestingModule();
    resolve(() => off++);
    await settle();
    expect(off).toBe(1);
  });
});

describe('SettingsDrafts', () => {
  it('keeps edits across engine changes and knows which sections are not saved', async () => {
    const { backend } = settingsBed();
    const store = TestBed.inject(SettingsStore);
    const drafts = TestBed.inject(SettingsDrafts);
    expect(drafts.formFor('sd.general')).toBeNull();
    await settle();
    const form = drafts.formFor('sd.general')!;
    expect(drafts.formFor('sd.general')).toBe(form);
    expect(form.dirtyKeys()).toEqual([]);
    form.model.update((m) => ({ ...m, greeting: 'Hi' }));
    expect(form.dirtyKeys()).toEqual(['greeting']);
    expect(drafts.dirtySections()).toEqual(['sd.general']);
    // Another window changes another key: it shows, the edit stays.
    await backend.set('sd.general', { repeat: 4 });
    expect(form.model()).toMatchObject({ greeting: 'Hi', repeat: 4 });
    expect(form.changes()).toEqual({ greeting: 'Hi' });
    // Applied: the edit is the value now.
    await store.apply('sd.general', form.changes());
    expect(form.dirtyKeys()).toEqual([]);
    form.model.update((m) => ({ ...m, greeting: 'Yo', repeat: 9 }));
    form.discard('greeting');
    expect(form.changes()).toEqual({ repeat: 9 });
    drafts.discardAll();
    expect(drafts.dirtySections()).toEqual([]);
  });

  it('gives each field the pre-check’s error, then the engine’s refusal of that value', async () => {
    settingsBed();
    const store = TestBed.inject(SettingsStore);
    const drafts = TestBed.inject(SettingsDrafts);
    await settle();
    const form = drafts.formFor('sd.general')!;
    const tree = form.tree as unknown as Record<string, () => { errors(): readonly { kind: string; message?: string }[] }>;
    form.model.update((m) => ({ ...m, repeat: 20 }));
    expect(tree['repeat']!().errors().map((e) => e.kind)).toEqual(['precheck']);
    form.model.update((m) => ({ ...m, repeat: 2, greeting: 'Hi!' }));
    expect(tree['repeat']!().errors()).toEqual([]);
    await store.apply('sd.general', { greeting: 'Hi!' });
    expect(tree['greeting']!().errors()[0]).toMatchObject({ kind: 'engine', message: 'Does not have the expected format ([A-Za-z ,]+).' });
    form.model.update((m) => ({ ...m, greeting: 'Hi' }));
    expect(tree['greeting']!().errors()).toEqual([]);
    expect(SETTINGS_OVERVIEW_FIXTURE.sections).toHaveLength(3);
  });
});
