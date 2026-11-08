import { TestBed } from '@angular/core/testing';
import type { WakeInput } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { MockBackend, type MockOptions } from '../core/mock-backend';
import { WakesStore } from './wakes.store';

const NOW = 1_700_000_000_000;
function setup(options: MockOptions = {}) {
  const mock = new MockBackend({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(WakesStore) };
}
const input = (p: Partial<WakeInput> = {}): WakeInput => ({ id: null, name: 'Evening', minuteOfDay: 1080, repeat: 'daily', stayAwakeMinutes: 20, wakeDevices: [], job: null, enabled: true, ...p });

describe('WakesStore', () => {
  it('loads the schedules and who sets the wake', async () => {
    const { store } = setup({ wake: 'on' });
    await store.load();
    expect(store.view()?.rows.map((r) => r.entry.name)).toEqual(['Morning inventory', 'Wake the NAS for the weekend', 'Once, tomorrow']);
    expect(store.view()?.route).toBe('helperReady');
    expect(store.view()?.rows[0]?.state).toEqual({ armed: { route: 'helper' } });
    // The next wake is the schedule's own local time: 07:00 on a weekday.
    const next = new Date(store.view()!.rows[0]!.entry.nextAt!);
    expect([next.getHours(), next.getMinutes()]).toEqual([7, 0]);
    expect(next.getTime()).toBeGreaterThan(NOW);
    expect([0, 6]).not.toContain(next.getDay());
    const sat = new Date(store.view()!.rows[1]!.entry.nextAt!);
    expect([sat.getDay(), sat.getHours(), sat.getMinutes()]).toEqual([6, 18, 0]);
  });

  it('keeps the engine message and recovers on retry', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'powerWakes').mockRejectedValueOnce({ code: 'internal', message: 'The scheduled wakes could not be read.' });
    await store.load();
    expect(store.error()?.message).toBe('The scheduled wakes could not be read.');
    await store.load();
    expect(store.error()).toBeNull();
  });

  it('adds, changes and cancels', async () => {
    const { store } = setup({ wake: 'on' });
    await store.load();
    expect(await store.save(input())).toBe(true);
    const added = store.view()!.rows.find((r) => r.entry.name === 'Evening')!;
    expect(await store.save(input({ id: added.entry.id, name: 'Evening 2' }))).toBe(true);
    expect(store.view()?.rows.map((r) => r.entry.name)).toContain('Evening 2');
    expect(await store.cancel(added.entry.id)).toBe(true);
    expect(store.view()?.rows.map((r) => r.entry.name)).not.toContain('Evening 2');
    expect(store.busy()).toBe(false);
  });

  it('keeps the list and the engine message when a save is refused', async () => {
    const { store } = setup({ wake: 'on' });
    await store.load();
    expect(await store.save(input({ name: ' ' }))).toBe(false);
    expect(store.actionError()?.message).toBe('Give the schedule a name.');
    expect(await store.save(input({ stayAwakeMinutes: null }))).toBe(false);
    expect(store.actionError()?.message).toContain('what happens');
    expect(store.view()?.rows).toHaveLength(3);
    store.clearActionError();
    expect(store.actionError()).toBeNull();
  });

  it('follows the schedules another window changes', async () => {
    const { store, mock } = setup({ wake: 'on' });
    await store.load();
    await mock.powerSetWake(input({ name: 'From another window' }));
    expect(store.view()?.rows.map((r) => r.entry.name)).toContain('From another window');
    await mock.powerCancelWake(1);
    expect(store.view()?.rows.map((r) => r.entry.id)).not.toContain(1);
  });

  it('shows every schedule as paused, with no next wake armed, while scheduled wake is off', async () => {
    const { store } = setup();
    await store.load();
    expect(store.view()?.rows.map((r) => r.state)).toEqual(['paused', 'paused', 'off']);
  });

  it('says honestly when the helper is needed or absent', async () => {
    const { store } = setup({ wake: 'needsHelper' });
    await store.load();
    expect(store.view()?.rows[0]?.state).toBe('needsHelper');
    expect(store.view()?.route).toMatchObject({ needsHelper: { reason: expect.stringContaining('administrator helper') } });
    TestBed.resetTestingModule();
    const other = setup({ wake: 'unavailable' });
    await other.store.load();
    expect(other.store.view()?.rows[0]?.state).toMatchObject({ failed: { why: expect.stringContaining('this build has none') } });
  });
});
