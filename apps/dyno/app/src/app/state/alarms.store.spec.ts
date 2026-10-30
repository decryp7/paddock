import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { AlarmsStore } from './alarms.store';
import { SettingsStore } from './settings.store';

const NOW = 1_700_000_000_000;

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(AlarmsStore), notifier: TestBed.inject(Notifier), settings: TestBed.inject(SettingsStore) };
}

describe('AlarmsStore', () => {
  it('loads the open alarms newest first, shelved ones included', async () => {
    const { store } = setup();
    await store.load();
    expect(store.loaded()).toBe(true);
    expect(store.alarms().map((a) => a.id)).toEqual([6, 7, 3, 5, 4]);
    expect(store.unacknowledged().map((a) => a.id).sort()).toEqual([3, 4, 6, 7]);
  });

  it('keeps the error when loading fails and recovers on retry', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'alarms').mockRejectedValueOnce({ code: 'agentUnavailable', message: 'The agent is not running.' });
    await store.load();
    expect(store.error()?.message).toBe('The agent is not running.');
    expect(store.loaded()).toBe(false);
    await store.load();
    expect(store.error()).toBeNull();
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('acknowledges one alarm and drops a returned alarm from the open list once acknowledged', async () => {
    const { store } = setup();
    await store.load();
    expect(await store.acknowledge([7])).toBe(true);
    expect(store.alarms().find((a) => a.id === 7)?.state).toBe('acknowledged');
    await store.acknowledge([4]);
    expect(store.alarms().find((a) => a.id === 4)).toBeUndefined();
  });

  it('acknowledges everything that waits, and nothing when nothing waits', async () => {
    const { store, mock } = setup();
    await store.load();
    await store.acknowledgeAll();
    expect(store.unacknowledged()).toHaveLength(0);
    const spy = vi.spyOn(mock, 'acknowledgeAlarms');
    expect(await store.acknowledge([])).toBe(true);
    expect(spy).not.toHaveBeenCalled();
  });

  it('shelves, unshelves, mutes and notes through the engine and applies the answers', async () => {
    const { store } = setup();
    await store.load();
    await store.shelve(7, 'untilReboot');
    expect(store.alarms().find((a) => a.id === 7)?.shelved).toBe('untilReboot');
    await store.unshelve(7);
    expect(store.alarms().find((a) => a.id === 7)?.shelved).toBeNull();
    await store.mute('alert:lowDisk', true);
    expect(store.mutedRules().has('alert:lowDisk')).toBe(true);
    await store.mute('alert:lowDisk', false);
    expect(store.mutedRules().size).toBe(0);
    await store.note(7, 'Looking into it');
    expect(store.alarms().find((a) => a.id === 7)?.notes.at(-1)?.text).toBe('Looking into it');
  });

  it('tells the user when a command fails and keeps the list', async () => {
    const { store, notifier } = setup();
    await store.load();
    expect(await store.note(7, '   ')).toBe(false);
    expect(notifier.recent().at(-1)?.text).toBe('Write a note of 1 to 200 characters.');
    expect(store.alarms()).toHaveLength(5);
  });

  it('applies pushed changes to the list, the log and the selected alarm', async () => {
    const { store, mock } = setup();
    await store.load();
    await store.loadHistory();
    await store.select(7);
    const logged = store.history().length;
    const detail = store.detailHistory().length;
    await mock.acknowledgeAlarms([7]);
    expect(store.alarms().find((a) => a.id === 7)?.state).toBe('acknowledged');
    expect(store.history()).toHaveLength(logged + 1);
    expect(store.detailHistory()).toHaveLength(detail + 1);
    await mock.acknowledgeAlarms([6]);
    expect(store.detailHistory()).toHaveLength(detail + 1);
  });

  it('does not collect a log nobody has opened', async () => {
    const { store, mock } = setup();
    await store.load();
    await mock.acknowledgeAlarms([7]);
    expect(store.history()).toEqual([]);
  });

  it('selects an alarm with its change log and clears it on close', async () => {
    const { store } = setup();
    await store.load();
    await store.select(5);
    expect(store.selected()?.id).toBe(5);
    expect(store.detailHistory().every((c) => c.alarm.id === 5)).toBe(true);
    expect(store.detailHistory().length).toBeGreaterThan(0);
    await store.select(null);
    expect(store.selected()).toBeNull();
    expect(store.detailHistory()).toEqual([]);
  });

  it('shows an empty log when the alarm history cannot be read', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'alarmHistory').mockRejectedValue(new Error('x'));
    await store.select(5);
    expect(store.detailHistory()).toEqual([]);
  });

  it('ignores a log that arrives after another alarm was selected', async () => {
    const { store, mock } = setup();
    await store.load();
    let release: (v: never[]) => void = () => undefined;
    vi.spyOn(mock, 'alarmHistory').mockImplementationOnce(() => new Promise((r) => (release = r)));
    const first = store.select(5);
    await store.select(7);
    release([]);
    await first;
    expect(store.selectedId()).toBe(7);
    expect(store.detailHistory().every((c) => c.alarm.id === 7)).toBe(true);
  });

  it('loads the history and the statistics, keeping errors for retry', async () => {
    const { store, mock } = setup();
    await store.loadHistory();
    await store.loadStats();
    expect(store.historyLoaded()).toBe(true);
    expect(store.history().length).toBeGreaterThan(5);
    expect(store.stats()?.noisiest[0]?.rule).toBe('threshold:gpu-load');
    vi.spyOn(mock, 'alarmHistory').mockRejectedValueOnce({ code: 'timeout', message: 'Too slow.' });
    vi.spyOn(mock, 'alarmStats').mockRejectedValueOnce({ code: 'timeout', message: 'Too slow.' });
    await store.loadHistory();
    await store.loadStats();
    expect(store.historyError()?.message).toBe('Too slow.');
    expect(store.statsError()?.message).toBe('Too slow.');
  });

  it('saves settings through the engine and refreshes the settings store', async () => {
    const { store, settings, mock } = setup();
    await settings.load();
    const error = await store.updateSettings((s) => ({ ...s, notifications: !s.notifications }));
    expect(error).toBeNull();
    expect(settings.settings()?.notifications).toBe((await mock.settings()).notifications);
    expect(store.saving()).toBe(false);
  });

  it('answers the engine error instead of throwing, and says when settings are not loaded', async () => {
    const { store, settings, mock } = setup();
    expect((await store.updateSettings((s) => s))?.message).toMatch(/not loaded/);
    await settings.load();
    vi.spyOn(mock, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'Limit must be 0 to 100.' });
    expect((await store.updateSettings((s) => s))?.message).toBe('Limit must be 0 to 100.');
    expect(store.saving()).toBe(false);
  });
});
