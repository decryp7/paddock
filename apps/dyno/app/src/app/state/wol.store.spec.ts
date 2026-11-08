import { TestBed } from '@angular/core/testing';
import type { WolDeviceInput } from '../core/api.types';
import { BACKEND } from '../core/backend';
import type { MockBackend } from '../core/mock-backend';
import { jobsProviders, mockWithJobs, pump } from '../../testing/job-test-support';
import { WolStore } from './wol.store';

const NOW = 1_700_000_000_000;

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const { mock, jobs } = mockWithJobs({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, jobsProviders(jobs)] });
  return { mock, store: TestBed.inject(WolStore) };
}
const input = (p: Partial<WolDeviceInput> = {}): WolDeviceInput => ({ id: null, name: 'Test box', mac: '02:00:5e:10:00:77', lastIp: null, interface: null, secureOn: 'keep', ...p });

describe('WolStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the saved computers', async () => {
    const { store } = setup();
    await store.load();
    expect(store.view()?.devices.map((d) => d.name)).toEqual(['Media PC', 'Office NAS']);
    expect(store.error()).toBeNull();
  });

  it('keeps the engine message and recovers on retry', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'wolDevices').mockRejectedValueOnce({ code: 'internal', message: 'The saved computers could not be read.' });
    await store.load();
    expect(store.error()?.message).toBe('The saved computers could not be read.');
    await store.load();
    expect(store.error()).toBeNull();
  });

  it('adds, changes and removes, following the list the engine pushes', async () => {
    const { store } = setup();
    await store.load();
    expect(await store.save(input())).toBe(true);
    expect(store.view()?.devices.map((d) => d.name)).toContain('Test box');
    const added = store.view()!.devices.find((d) => d.name === 'Test box')!;
    expect(await store.save(input({ id: added.id, name: 'Renamed' }))).toBe(true);
    expect(store.view()?.devices.map((d) => d.name)).toContain('Renamed');
    expect(await store.remove(added.id)).toBe(true);
    expect(store.view()?.devices.map((d) => d.name)).not.toContain('Renamed');
    expect(store.busy()).toBe(false);
  });

  it('keeps the list and the engine message when a save is refused', async () => {
    const { store } = setup();
    await store.load();
    expect(await store.save(input({ mac: 'nope' }))).toBe(false);
    expect(store.actionError()?.message).toContain('six pairs');
    expect(await store.save(input({ mac: '02:00:5e:10:00:01' }))).toBe(false);
    expect(store.actionError()?.message).toContain('already saved as "Media PC"');
    expect(store.view()?.devices).toHaveLength(2);
    store.clearActionError();
    expect(store.actionError()).toBeNull();
  });

  it('reads the neighbours on demand, with their own error and empty states', async () => {
    const { store } = setup();
    expect(store.candidates()).toBeNull();
    await store.loadCandidates();
    expect(store.candidates()?.map((n) => n.ip)).toEqual(['192.0.2.50', '192.0.2.51', '192.0.2.52']);
    expect(store.candidatesLoading()).toBe(false);
    TestBed.resetTestingModule();
    const failing = setup({ wol: 'neighboursFail' });
    await failing.store.loadCandidates();
    expect(failing.store.candidatesError()?.message).toContain('could not be read');
  });

  it('reads this PC\'s readiness with its own error', async () => {
    const { store, mock } = setup();
    await store.loadReadiness();
    expect(store.readiness()?.items.length).toBeGreaterThan(2);
    vi.spyOn(mock, 'wolReadiness').mockRejectedValueOnce({ code: 'internal', message: 'No.' });
    await store.loadReadiness();
    expect(store.readinessError()?.message).toBe('No.');
  });

  it('wakes as a job and keeps the outcome and the last wake', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'startJob');
    await store.wake(1, false);
    expect(spy).toHaveBeenCalledWith({ wolWake: { id: 1, test: false } });
    const run = store.runs()[1]!;
    expect(run.running).toBe(false);
    expect(run.outcome?.message).toContain('Wake packets sent');
    expect(store.view()?.devices[0]?.lastWake?.summary).toContain('Wake packets sent');
  });

  it('a test wake says it woke, or that it did not and what to do', async () => {
    const { store } = setup();
    await store.load();
    await store.wake(1, true);
    expect(store.runs()[1]?.outcome).toMatchObject({ tested: true, answered: { how: 'ping' } });
    TestBed.resetTestingModule();
    const silent = setup({ wol: 'timeout' });
    await silent.store.load();
    await silent.store.wake(1, true);
    expect(silent.store.runs()[1]?.outcome?.message).toContain('off at the wall');
  });

  it('shows a refused wake and lets the next one run', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'startJob').mockRejectedValueOnce({ code: 'notFound', message: 'That computer is no longer in the list.' });
    await store.wake(1, false);
    expect(store.runs()[1]?.error?.message).toContain('no longer in the list');
    expect(store.runs()[1]?.running).toBe(false);
    await store.wake(1, false);
    expect(store.runs()[1]?.error).toBeNull();
  });

  it('Cancel stops a wake in progress and says so', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    await store.load();
    mock.jobSteps = 10;
    mock.jobStepMs = 500;
    const done = store.wake(1, true);
    await pump(500);
    expect(store.progress(1)?.snapshot).toMatchObject({ title: 'Testing a wake', subject: 'Media PC', state: 'running', phase: { id: 'sending' } });
    await store.cancel(1);
    await pump(500);
    await done;
    expect(store.runs()[1]).toMatchObject({ running: false, cancelled: true, outcome: null });
  });

  it('never runs two wakes of one computer at once', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'startJob');
    const first = store.wake(1, false);
    await store.wake(1, false);
    await first;
    expect(spy).toHaveBeenCalledTimes(1);
  });
});
