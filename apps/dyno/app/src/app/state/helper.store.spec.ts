import { TestBed } from '@angular/core/testing';
import type { HelperStatus } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { HELPER_POLL_MS, HelperStore } from './helper.store';

function setup(latencyMs = 0) {
  const mock = new MockBackend({ latencyMs, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(HelperStore) };
}

const status = (state: HelperStatus['state'], extra: Partial<HelperStatus> = {}): HelperStatus => ({ state, message: state, version: null, prompt: 'p', recent: [], error: null, ...extra });

describe('HelperStore', () => {
  it('starts unknown, loads the status, and is ready only when the helper answers and is current', async () => {
    const { store, mock } = setup();
    expect(store.status()).toBeNull();
    expect(store.ready()).toBe(false);
    await store.load();
    expect(store.status()?.state).toBe('notInstalled');
    expect(store.ready()).toBe(false);
    await mock.installHelper();
    await store.load();
    expect(store.ready()).toBe(true);
    vi.spyOn(mock, 'helperStatus').mockRejectedValue({ code: 'agentUnavailable', message: 'down' });
    await store.load();
    expect(store.error()?.message).toBe('down');
  });

  it('install asks once, resolves true when the helper is ready and never touches the helper on its own', async () => {
    const { store, mock } = setup();
    const install = vi.spyOn(mock, 'installHelper');
    await store.load();
    expect(install).not.toHaveBeenCalled();
    expect(await store.install()).toBe(true);
    expect(install).toHaveBeenCalledTimes(1);
    expect(store.ready()).toBe(true);
    expect(store.working()).toBe(false);
  });

  it('while the system prompt is open it polls until the state changes', async () => {
    vi.useFakeTimers();
    try {
      const { store, mock } = setup();
      vi.spyOn(mock, 'installHelper').mockResolvedValue(status('installing'));
      const polls = [status('installing'), status('installing'), status('ready', { version: '1.0.0' })];
      const poll = vi.spyOn(mock, 'helperStatus').mockImplementation(() => Promise.resolve(polls.shift()!));
      const done = store.install();
      await vi.advanceTimersByTimeAsync(HELPER_POLL_MS * 3 + 10);
      expect(await done).toBe(true);
      expect(poll).toHaveBeenCalledTimes(3);
      expect(store.status()?.version).toBe('1.0.0');
    } finally {
      vi.useRealTimers();
    }
  });

  it('a declined prompt ends as not installed with the reason, and a failing call is an error', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'installHelper').mockResolvedValue(status('notInstalled', { error: 'The permission prompt was closed, so nothing was installed.' }));
    expect(await store.install()).toBe(false);
    expect(store.status()?.error).toContain('prompt was closed');
    vi.spyOn(mock, 'installHelper').mockRejectedValue({ code: 'unsupported', message: 'Not here.' });
    expect(await store.install()).toBe(false);
    expect(store.error()?.message).toBe('Not here.');
    expect(store.working()).toBe(false);
  });

  it('uninstall puts the state back and reports failures', async () => {
    const { store, mock } = setup();
    await store.install();
    expect(await store.uninstall()).toBe(true);
    expect(store.status()?.state).toBe('notInstalled');
    vi.spyOn(mock, 'uninstallHelper').mockRejectedValue({ code: 'internal', message: 'no' });
    expect(await store.uninstall()).toBe(false);
    expect(store.error()?.message).toBe('no');
  });
});
