import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { QuitStore, QUIT_KEEPS, QUIT_PLAIN } from './quit.store';

describe('QuitStore', () => {
  function make() {
    const mock = new MockBackend();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    return { mock, store: TestBed.inject(QuitStore) };
  }

  it('says Quit Dyno until the engine says the agent keeps measuring', async () => {
    const { mock, store } = make();
    expect(store.title()).toBe(QUIT_PLAIN);
    mock.keepsMeasuring = true;
    await store.refresh();
    expect(store.title()).toBe(QUIT_KEEPS);
    mock.keepsMeasuring = false;
    await store.refresh();
    expect(store.title()).toBe(QUIT_PLAIN);
  });

  it('keeps the last answer when the engine cannot be asked', async () => {
    const { mock, store } = make();
    mock.keepsMeasuring = true;
    await store.refresh();
    mock.quitKeepsMeasuring = () => Promise.reject(new Error('gone'));
    await store.refresh();
    expect(store.title()).toBe(QUIT_KEEPS);
  });
});
