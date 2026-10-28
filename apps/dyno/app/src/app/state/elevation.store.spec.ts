import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ElevationStore } from './elevation.store';
import { LiveStore } from './live.store';

function make() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const store = TestBed.inject(ElevationStore);
  const live = TestBed.inject(LiveStore);
  const notifier = TestBed.inject(Notifier);
  const tick = () => live.receive(mock.liveUpdate('summary'));
  return { mock, store, notifier, tick };
}

describe('ElevationStore', () => {
  it('follows the agent: elevated only when the live update says so', async () => {
    const { store, tick } = make();
    expect(store.elevated()).toBe(false);
    await store.runAsAdministrator();
    tick();
    expect(store.elevated()).toBe(true);
    await store.goBackToNormal();
    tick();
    expect(store.elevated()).toBe(false);
    expect(store.busy()).toBe(false);
  });

  it('a cancelled prompt leaves normal rights with a calm message, not an error', async () => {
    const { mock, store, notifier, tick } = make();
    mock.elevationDeclined = true;
    await store.runAsAdministrator();
    tick();
    expect(store.elevated()).toBe(false);
    const toast = notifier.toasts()[0]!;
    expect(toast.tone).toBe('ok');
    expect(toast.text).toContain('normal rights');
  });

  it('a failure is reported as an error and the store is usable again', async () => {
    const { mock, store, notifier } = make();
    mock.runAsAdministrator = () => Promise.reject({ code: 'accessDenied', message: 'No permission prompt is available.' });
    await store.runAsAdministrator();
    expect(notifier.toasts()[0]).toMatchObject({ tone: 'bad', text: 'No permission prompt is available.' });
    expect(store.busy()).toBe(false);
  });
});
