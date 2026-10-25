import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { makeMock, flush } from '../../testing/test-helpers';
import { AvatarsStore } from './avatars.store';

describe('AvatarsStore', () => {
  it('asks the engine once per identity and keeps the data url', async () => {
    const backend = makeMock();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    const store = TestBed.inject(AvatarsStore);
    expect(store.urlOf({ watch: 'w-1' })).toBeNull();
    store.request({ watch: 'w-1' });
    store.request({ watch: 'w-1' });
    store.request({ preset: 'github' });
    await flush(5);
    expect(store.urlOf({ watch: 'w-1' })).toMatch(/^data:image\//);
    expect(backend.calls.filter((c) => c === 'watchAvatar:w-1')).toHaveLength(1);
  });

  it('treats a failed lookup as no picture', async () => {
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: makeMock() }] });
    const store = TestBed.inject(AvatarsStore);
    store.request({ watch: 'w-missing' });
    await flush(5);
    expect(store.urlOf({ watch: 'w-missing' })).toBeNull();
  });

  it('does nothing without a backend', () => {
    TestBed.configureTestingModule({});
    const store = TestBed.inject(AvatarsStore);
    store.request({ preset: 'x' });
    expect(store.urlOf({ preset: 'x' })).toBeNull();
  });
});
