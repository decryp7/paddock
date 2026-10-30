import { TestBed } from '@angular/core/testing';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { dialogsAnswering } from '../../../testing/render';
import { UpdatesStore, resultToast } from './updates.store';

function setup(backend: PitwallBackend = new MockBackend({ modelUpdates: true }), confirm = true) {
  const dialogs = dialogsAnswering(confirm);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, dialogs] });
  return { store: TestBed.inject(UpdatesStore), toasts: TestBed.inject(Toasts), backend, asked: dialogs.opened };
}

const failing = (...m: (keyof PitwallBackend)[]) => new MockBackend({ modelUpdates: true, failing: m });

describe('resultToast', () => {
  it('words each outcome', () => {
    const r = { message: 'm', reposChecked: 1, notices: 0, retryInSecs: null };
    expect(resultToast({ ...r, outcome: 'checked' })).toEqual({ tone: 'ok', text: 'Checked: your models are up to date.' });
    expect(resultToast({ ...r, outcome: 'checked', notices: 1 }).text).toBe('Checked: 1 update available.');
    expect(resultToast({ ...r, outcome: 'checked', notices: 2 }).text).toBe('Checked: 2 updates available.');
    expect(resultToast({ ...r, outcome: 'offline', message: 'Offline mode is on' })).toEqual({ tone: 'info', text: 'Offline mode is on' });
  });
});

describe('UpdatesStore', () => {
  it('loads the notices and the check status', async () => {
    const { store } = setup();
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loaded()).toBe(true);
    expect(store.count()).toBe(2);
    expect(store.status()?.settings).toEqual({ enabled: false, schedule: 'weekly' });
    expect(store.noticesFor()('qwen3.5-9b-q4_k_m').map((n) => n.kind)).toEqual(['newerRevision']);
    expect(store.noticesFor()('nothing')).toEqual([]);
  });

  it('shows a load error and loads once with ensureLoaded', async () => {
    const { store } = setup(failing('updateNotices'));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loaded()).toBe(false);
  });

  it('loads once however often ensureLoaded is called', async () => {
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'updateNotices');
    await store.ensureLoaded();
    await store.ensureLoaded();
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('checks now: an empty library says nothing is on offer, the fixture finds updates, and the result stays visible', async () => {
    const { store, toasts } = setup(new MockBackend());
    await store.load();
    expect(store.count()).toBe(0);
    await store.checkNow();
    expect(store.count()).toBe(2);
    expect(toasts.toasts().at(-1)?.message).toBe('Checked: 2 updates available.');
    expect(store.lastResult()?.outcome).toBe('checked');
    expect(store.checking()).toBe(false);
    expect(store.status()?.lastCheckedAt).not.toBeNull();
    // A second click straight away is refused by the rate limit, with its message.
    await store.checkNow();
    expect(store.lastResult()?.outcome).toBe('rateLimited');
    expect(toasts.toasts().at(-1)?.message).toContain('moments ago');
  });

  it('says offline mode stopped the check', async () => {
    const mock = new MockBackend();
    const s = await mock.settings();
    await mock.saveSettings({ ...s, network: { ...s.network, offline: true } });
    const { store, toasts } = setup(mock);
    await store.load();
    expect(store.status()?.offline).toBe(true);
    await store.checkNow();
    expect(store.lastResult()?.outcome).toBe('offline');
    expect(toasts.toasts().at(-1)?.message).toContain('Offline mode is on');
    expect(store.count()).toBe(0);
  });

  it('reports a failing check as an error toast and does not stay busy', async () => {
    const { store, toasts } = setup(failing('checkUpdatesNow'));
    await store.load();
    await store.checkNow();
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.checking()).toBe(false);
  });

  it('does not run two checks at once', async () => {
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'checkUpdatesNow');
    await Promise.all([store.checkNow(), store.checkNow()]);
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('turns the schedule on and off', async () => {
    const { store, toasts } = setup();
    await store.load();
    expect(await store.setCheck(true, 'daily')).toBe(true);
    expect(store.status()?.settings).toEqual({ enabled: true, schedule: 'daily' });
    expect(toasts.toasts().at(-1)?.message).toBe('Checking every day.');
    await store.setCheck(true, 'weekly');
    expect(toasts.toasts().at(-1)?.message).toBe('Checking every week.');
    await store.setCheck(false, 'weekly');
    expect(toasts.toasts().at(-1)?.message).toBe('Scheduled checks are off.');
    expect(store.saving()).toBe(false);
  });

  it('keeps the status when saving the schedule fails', async () => {
    const { store, toasts } = setup(failing('setUpdateCheck'));
    await store.load();
    expect(await store.setCheck(true, 'daily')).toBe(false);
    expect(store.status()?.settings.enabled).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.saving()).toBe(false);
  });

  it('dismisses a notice, or snoozes it, and updates the count in the status', async () => {
    const { store } = setup();
    await store.load();
    await store.dismiss('qwen3.5-9b-q4_k_m#newerRevision', null);
    expect(store.notices().map((n) => n.id)).toEqual(['qwen3.5-4b-q4_k_m#betterQuant']);
    expect(store.status()?.notices).toBe(1);
    await store.dismiss('qwen3.5-4b-q4_k_m#betterQuant', 7);
    expect(store.count()).toBe(0);
  });

  it('shows the engine\'s message when a dismissal fails (the notice is already gone)', async () => {
    const { store, toasts } = setup();
    await store.load();
    await store.dismiss('gone#betterQuant', null);
    expect(toasts.toasts().at(-1)?.message).toContain('no update notice');
    expect(store.count()).toBe(2);
    expect(store.isBusy()('gone#betterQuant')).toBe(false);
  });

  it('"Get it" asks first, naming the host and the size, and downloads only when confirmed', async () => {
    const { store, backend, asked, toasts } = setup(new MockBackend({ modelUpdates: true }), true);
    await store.load();
    const spy = vi.spyOn(backend, 'downloadModel');
    const n = store.notices()[0]!;
    expect(await store.get(n)).toBe(true);
    expect(asked).toHaveLength(1);
    expect(asked[0]!.inputs['heading']).toBe('Download Qwen3.5-9B-Q4_K_M.gguf?');
    expect(asked[0]!.inputs['message']).toContain('from huggingface.co');
    expect(asked[0]!.inputs['message']).toContain('replaces the copy you have');
    expect(spy).toHaveBeenCalledWith('unsloth/Qwen3.5-9B-GGUF', 'Qwen3.5-9B-Q4_K_M.gguf', null);
    expect(toasts.toasts().at(-1)?.message).toBe('Downloading Qwen3.5-9B-Q4_K_M.gguf.');
    expect(store.notices().some((x) => x.id === n.id)).toBe(false);
  });

  it('downloads nothing when the user says no', async () => {
    const { store, backend } = setup(new MockBackend({ modelUpdates: true }), false);
    await store.load();
    const spy = vi.spyOn(backend, 'downloadModel');
    expect(await store.get(store.notices()[1]!)).toBe(false);
    expect(spy).not.toHaveBeenCalled();
    expect(store.count()).toBe(2);
  });

  it('a better quantisation does not claim to replace the copy you have', async () => {
    const { store, asked } = setup();
    await store.load();
    await store.get(store.notices().find((n) => n.kind === 'betterQuant')!);
    expect(asked[0]!.inputs['message']).not.toContain('replaces');
  });

  it('shows the error when a download is refused (offline mode)', async () => {
    const { store, toasts } = setup(failing('downloadModel'));
    await store.load();
    expect(await store.get(store.notices()[0]!)).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.count()).toBe(2);
  });

  it('reads the notices again when a model update event arrives or a download finishes, and not for other events', async () => {
    const { store, backend } = setup(new MockBackend({ modelUpdates: true }));
    const live = TestBed.inject(LiveStore);
    const state = live as unknown as WritableStateSource<object>;
    const event = (id: number, kind: string) => ({ id, at: 1, kind, severity: 'info', message: 'x', modelId: null, appId: null, reason: null });
    await store.load();
    TestBed.tick();
    const spy = vi.spyOn(backend, 'updateNotices');
    patchState(state, { events: [event(1, 'modelLoaded')] });
    TestBed.tick();
    await Promise.resolve();
    expect(spy).not.toHaveBeenCalled();
    patchState(state, { events: [event(2, 'modelUpdate'), event(1, 'modelLoaded')] });
    TestBed.tick();
    await new Promise((r) => setTimeout(r, 0));
    expect(spy).toHaveBeenCalledTimes(1);
    const done = { id: 'dl-9', kind: 'model', name: 'f.gguf', host: 'huggingface.co', bytesDone: 1, bytesTotal: 1, speedBps: 0, state: 'done', error: null, startedAt: 1 };
    patchState(state, { downloads: [done] });
    TestBed.tick();
    await new Promise((r) => setTimeout(r, 0));
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('does not read anything before it was loaded once', async () => {
    const { backend } = setup();
    const live = TestBed.inject(LiveStore);
    const spy = vi.spyOn(backend, 'updateNotices');
    patchState(live as unknown as WritableStateSource<object>, { downloads: [{ id: 'd', kind: 'model', name: 'f', host: 'h', bytesDone: 1, bytesTotal: 1, speedBps: 0, state: 'done', error: null, startedAt: 1 }] });
    TestBed.tick();
    await Promise.resolve();
    expect(spy).not.toHaveBeenCalled();
  });
});
