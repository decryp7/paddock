import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { PowerStore } from './power.store';

const NOW = 1_700_000_000_000;
const spec = { scope: 'system', until: { for: { minutes: 30 } } } as const;

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(PowerStore) };
}

describe('PowerStore', () => {
  it('loads the agent view', async () => {
    const { store } = setup();
    await store.load();
    expect(store.view()?.awake).toBe('off');
    expect(store.error()).toBeNull();
  });

  it('starts a hold, remembers what was asked and follows the pushed views', async () => {
    const { store, mock } = setup();
    expect(await store.start(spec)).toBe(true);
    expect(store.view()?.awake).toMatchObject({ holding: { ends: NOW + 30 * 60_000 } });
    expect(store.lastSpec()).toEqual(spec);
    expect(store.busy()).toBe(false);
    // Another window stops it: the push reaches this store.
    await mock.powerStopKeepAwake();
    expect(store.view()?.awake).toBe('off');
  });

  it('stops a hold without forgetting the last request', async () => {
    const { store } = setup();
    await store.start(spec);
    expect(await store.stop()).toBe(true);
    expect(store.view()?.awake).toBe('off');
    expect(store.lastSpec()).toEqual(spec);
  });

  it('keeps the engine message when a start is refused, and the earlier view', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'powerKeepAwake').mockRejectedValueOnce({ code: 'badRequest', message: 'Keep awake is turned off in Settings.' });
    expect(await store.start(spec)).toBe(false);
    expect(store.actionError()?.message).toBe('Keep awake is turned off in Settings.');
    expect(store.view()?.awake).toBe('off');
    expect(store.busy()).toBe(false);
  });

  it('keeps the error when the first read fails and recovers on retry', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'powerState').mockRejectedValueOnce({ code: 'agentUnavailable', message: 'The agent is not running.' });
    await store.load();
    expect(store.error()?.message).toBe('The agent is not running.');
    expect(store.view()).toBeNull();
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.view()).not.toBeNull();
  });

  it('shows the states the mock can start in', async () => {
    for (const variant of ['ended', 'waiting'] as const) {
      TestBed.resetTestingModule();
      const { store } = setup({ power: variant });
      await store.load();
      expect(store.view()?.awake).toHaveProperty(variant);
    }
    TestBed.resetTestingModule();
    const { store } = setup({ power: 'unavailable' });
    await store.load();
    expect(store.view()?.caps.keepAwake).toHaveProperty('cantTell');
    await store.start(spec);
    expect(store.view()?.awake).toHaveProperty('ended');
  });
});
