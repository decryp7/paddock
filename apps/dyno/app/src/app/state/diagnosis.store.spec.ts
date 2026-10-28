import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { DiagnosisStore, MAX_DIAGNOSIS_HISTORY } from './diagnosis.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(DiagnosisStore), notifier: TestBed.inject(Notifier), activity: TestBed.inject(ActivityStore) };
}

describe('DiagnosisStore', () => {
  it('run stores the diagnosis', async () => {
    const { store } = setup();
    await store.run();
    expect(store.diagnosis()?.headline).toBeTruthy();
    expect(store.running()).toBe(false);
  });
  it('keeps the diagnoses of this session, newest first, up to the limit', async () => {
    const { store, mock } = setup();
    expect(store.history()).toEqual([]);
    for (let i = 0; i < MAX_DIAGNOSIS_HISTORY + 2; i++) {
      vi.spyOn(mock, 'whySlow').mockResolvedValueOnce({ at: i, windowSecs: 55, headline: `run ${i}`, causes: [] });
      await store.run();
    }
    expect(store.history()).toHaveLength(MAX_DIAGNOSIS_HISTORY);
    expect(store.history()[0]!.headline).toBe(`run ${MAX_DIAGNOSIS_HISTORY + 1}`);
    expect(store.history()[0]).toBe(store.diagnosis());
  });
  it('a failed run keeps the history', async () => {
    const { store, mock } = setup();
    await store.run();
    vi.spyOn(mock, 'whySlow').mockRejectedValue({ code: 'busy', message: 'b' });
    await store.run();
    expect(store.history()).toHaveLength(1);
  });
  it('run failure keeps the error and clears running', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'whySlow').mockRejectedValue({ code: 'agentUnavailable', message: 'down' });
    await store.run();
    expect(store.error()).toEqual({ code: 'agentUnavailable', message: 'down' });
    expect(store.running()).toBe(false);
    expect(store.diagnosis()).toBeNull();
  });
  it('ensure runs only when there is no diagnosis and nothing running', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'whySlow');
    await store.ensure();
    await store.ensure();
    expect(spy).toHaveBeenCalledTimes(1);
  });
  it('loads anomalies or reports the failure', async () => {
    const { store, mock } = setup();
    await store.loadAnomalies();
    expect(store.anomaliesLoaded()).toBe(true);
    expect(store.anomalies().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'anomalies').mockRejectedValue({ code: 'timeout', message: 't' });
    await store.loadAnomalies();
    expect(store.anomaliesError()?.code).toBe('timeout');
  });
});
