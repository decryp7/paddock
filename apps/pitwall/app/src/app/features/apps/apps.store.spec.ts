import { TestBed } from '@angular/core/testing';
import type { AppPermissions } from '../../core/api.types';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { AppsStore, upsertApp } from './apps.store';

function setup(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [AppsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(AppsStore), toasts: TestBed.inject(Toasts) };
}

const PERMS: AppPermissions = {
  models: ['qwen3-8b-instruct-q4_k_m'],
  maxPriority: 'batch',
  tools: false,
  extendedApis: false,
  limits: { requestsPerMin: 10, tokensPerDay: null, maxConcurrent: 1, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null },
};

describe('AppsStore', () => {
  it('loads apps and local models', async () => {
    const { store } = setup();
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loaded()).toBe(true);
    expect(store.apps().map((a) => a.id)).toContain('debrief');
    expect(store.models().length).toBeGreaterThan(0);
  });

  it('keeps the last good list and a typed error when loading fails', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    const count = store.apps().length;
    vi.spyOn(backend, 'apps').mockRejectedValue({ code: 'serverUnavailable', message: 'down' });
    await store.load();
    expect(store.error()).toEqual({ code: 'serverUnavailable', message: 'down' });
    expect(store.apps().length).toBe(count);
  });

  it('creates an app, a key (secret only in the result), revokes the key and deletes the app', async () => {
    const { store, toasts } = setup();
    await store.load();
    const app = await store.createApp('Raycast');
    expect(app?.id).toBe('raycast');
    expect(store.apps().some((a) => a.id === 'raycast')).toBe(true);
    const created = await store.createKey('raycast');
    expect(created?.secret).toMatch(/^pw_[0-9a-f]{40}$/);
    expect(JSON.stringify(store.apps())).not.toContain(created!.secret);
    expect(store.apps().find((a) => a.id === 'raycast')!.keys.length).toBe(1);
    expect(await store.revokeKey('raycast', created!.key.id)).toBe(true);
    expect(store.apps().find((a) => a.id === 'raycast')!.keys.length).toBe(0);
    expect(await store.deleteApp('raycast')).toBe(true);
    expect(store.apps().some((a) => a.id === 'raycast')).toBe(false);
    expect(toasts.toasts().map((t) => t.tone)).toContain('ok');
  });

  it('saves permissions and replaces the app', async () => {
    const { store } = setup();
    await store.load();
    expect(await store.setPermissions('debrief', PERMS)).toBe(true);
    expect(store.apps().find((a) => a.id === 'debrief')!.permissions).toEqual(PERMS);
  });

  it('shows action errors as toasts and changes nothing', async () => {
    const { store, toasts } = setup();
    await store.load();
    expect(await store.createApp('Debrief')).toBeUndefined();
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'bad' });
    expect(await store.createKey('nope')).toBeUndefined();
    expect(await store.setPermissions('nope', PERMS)).toBe(false);
  });

  it('ignores a second action while the first runs', async () => {
    const backend = new MockBackend({ latencyMs: 5 });
    const { store } = setup(backend);
    await store.load();
    const a = store.revokeKey('debrief', 'debrief-k1');
    expect(store.isBusy('key:debrief-k1')).toBe(true);
    const b = store.revokeKey('debrief', 'debrief-k1');
    expect(await b).toBe(false);
    expect(await a).toBe(true);
    expect(store.isBusy('key:debrief-k1')).toBe(false);
  });

  it('upserts by id', () => {
    const a = { id: 'a', name: 'A' } as never;
    const a2 = { id: 'a', name: 'A2' } as never;
    const b = { id: 'b', name: 'B' } as never;
    expect(upsertApp([a], b)).toEqual([a, b]);
    expect(upsertApp([a, b], a2)).toEqual([a2, b]);
  });
});
