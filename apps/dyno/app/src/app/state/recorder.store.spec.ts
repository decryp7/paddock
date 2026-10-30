import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { RecorderStore } from './recorder.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(RecorderStore), notifier: TestBed.inject(Notifier) };
}

describe('RecorderStore', () => {
  it('loads newest first, opens the newest and picks the busiest moment', async () => {
    const { store } = setup();
    await store.load();
    const ids = store.snapshots().map((s) => s.id);
    expect(ids).toEqual([3, 2, 1]);
    expect(store.selectedId()).toBe(3);
    expect(store.snapshot()?.samples.length).toBe(600);
    const cpu = store.snapshot()!.samples.map((s) => s.cpuTotal);
    expect(store.moment()).toBe(cpu.indexOf(Math.max(...cpu)));
  });

  it('keeps the selection on reload and drops it when the snapshot is gone', async () => {
    const { store, mock } = setup();
    await store.load();
    await store.select(2);
    await store.load();
    expect(store.selectedId()).toBe(2);
    vi.spyOn(mock, 'recorderSnapshots').mockResolvedValue([]);
    await store.load();
    expect(store.selectedId()).toBeNull();
    expect(store.snapshot()).toBeNull();
  });

  it('records a list failure and retries it', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'recorderSnapshots').mockRejectedValueOnce({ code: 'internal', message: 'no' });
    await store.load();
    expect(store.listError()?.message).toBe('no');
    expect(store.listLoaded()).toBe(false);
    await store.retry();
    expect(spy).toHaveBeenCalledTimes(2);
    expect(store.listLoaded()).toBe(true);
  });

  it('records a snapshot failure and retries just that snapshot', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'recorderSnapshot').mockRejectedValueOnce({ code: 'notFound', message: 'gone' });
    await store.select(2);
    expect(store.snapshotError()?.message).toBe('gone');
    await store.retry();
    expect(store.snapshotError()).toBeNull();
    expect(store.snapshot()).not.toBeNull();
  });

  it('drops a stale answer for an earlier choice', async () => {
    const { store, mock } = setup();
    await store.load();
    const real = mock.recorderSnapshot.bind(mock);
    let release!: () => void;
    const slow = new Promise<void>((r) => (release = r));
    vi.spyOn(mock, 'recorderSnapshot').mockImplementationOnce(async (id) => {
      await slow;
      return real(id);
    });
    const first = store.select(1);
    await store.select(2);
    release();
    await first;
    expect(store.selectedId()).toBe(2);
    expect(store.snapshot()?.at).toBe(store.snapshots().find((s) => s.id === 2)!.at);
  });

  it('captures, announces and opens the new snapshot; ignores a second click while busy', async () => {
    const { store, mock, notifier } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'capture');
    const a = store.capture();
    const b = store.capture();
    await Promise.all([a, b]);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.snapshots()[0]!.reason).toBe('manual');
    expect(store.selectedId()).toBe(store.snapshots()[0]!.id);
    expect(store.capturing()).toBe(false);
    expect(notifier.recent().length).toBe(1);
  });

  it('reports a failed capture without changing the list', async () => {
    const { store, mock, notifier } = setup();
    await store.load();
    vi.spyOn(mock, 'capture').mockRejectedValue({ code: 'internal', message: 'disk full' });
    await store.capture();
    expect(store.snapshots()).toHaveLength(3);
    expect(notifier.recent()[0]!.text).toBe('disk full');
    expect(store.capturing()).toBe(false);
  });

  it('clamps the picked moment and steps it', async () => {
    const { store } = setup();
    store.setMoment(5);
    store.stepMoment(1);
    store.jumpToPeak();
    expect(store.moment()).toBe(0);
    await store.load();
    store.setMoment(-4);
    expect(store.moment()).toBe(0);
    store.setMoment(99999);
    expect(store.moment()).toBe(599);
    store.setMoment(NaN);
    expect(store.moment()).toBe(599);
    store.stepMoment(10);
    expect(store.moment()).toBe(599);
    store.stepMoment(-10);
    expect(store.moment()).toBe(589);
    store.jumpToPeak();
    expect(store.moment()).toBeLessThan(599);
  });
});
