import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { patchState, signalStore, withMethods } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from './request-status';

const Probe = signalStore(
  { protectedState: true },
  withRequestStatus({ retry: false, kind: 'permanent', why: 'a probe of the union' }),
  withMethods((store) => ({
    start: () => patchState(store, setLoading()),
    done: () => patchState(store, setLoaded()),
    fail: (m: string) => patchState(store, setError(m)),
  })),
);

describe('withRequestStatus (rule 30)', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), Probe] }));

  it('starts idle and moves through one union', () => {
    const store = TestBed.inject(Probe);
    expect([store.requestStatus(), store.isLoading(), store.isLoaded(), store.requestError()]).toEqual(['idle', false, false, null]);
    store.start();
    expect([store.requestStatus(), store.isLoading(), store.isLoaded(), store.requestError()]).toEqual(['loading', true, false, null]);
    store.fail('the engine is gone');
    expect([store.requestStatus(), store.isLoading(), store.isLoaded(), store.requestError()]).toEqual([{ error: 'the engine is gone' }, false, false, 'the engine is gone']);
    store.done();
    expect([store.requestStatus(), store.isLoading(), store.isLoaded(), store.requestError()]).toEqual(['loaded', false, true, null]);
  });
});

describe('errorMessage', () => {
  it('reads every shape a rejection takes', () => {
    expect(errorMessage(new Error('boom'), 'x')).toBe('boom');
    expect(errorMessage('plain', 'x')).toBe('plain');
    expect(errorMessage({ code: 'unavailable', message: 'starting' }, 'x')).toBe('starting');
    expect(errorMessage(undefined, 'fallback')).toBe('fallback');
    expect(errorMessage('', 'fallback')).toBe('fallback');
    expect(errorMessage({ message: 3 }, 'fallback')).toBe('fallback');
  });
});
