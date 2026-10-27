import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { LogsStore, PAGE, VIEW_CAP } from './logs.store';

function setup() {
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, LogsStore] });
  return { mock, store: TestBed.inject(LogsStore) };
}

const lines = (from: number, n: number) => Array.from({ length: n }, (_, i) => ({ seq: from + i, text: `l${from + i}` }));

describe('LogsStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('opens a runner and loads the newest page', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.open('build-win');
    expect(s.store.runner()).toBe('build-win');
    expect(s.store.lines()).toHaveLength(PAGE);
    expect(s.store.loading()).toBe(false);
    expect(s.store.following()).toBe(true);
  });

  it('records an open error', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.open('nope');
    expect(s.store.error()?.code).toBe('unknownRunner');
    expect(s.store.loading()).toBe(false);
  });

  it('polls for newer lines only and caps the view', async () => {
    const s = setup();
    mock = s.mock;
    expect(await s.store.poll()).toBe(0);
    await s.store.open('build-win');
    const last = s.store.lines().at(-1)!.seq;
    expect(await s.store.poll()).toBe(0);
    vi.spyOn(mock, 'logs').mockResolvedValueOnce(lines(last + 1, 3));
    expect(await s.store.poll()).toBe(3);
    expect(s.store.lines().at(-1)?.seq).toBe(last + 3);
    vi.spyOn(mock, 'logs').mockResolvedValueOnce(lines(last + 4, VIEW_CAP));
    await s.store.poll();
    expect(s.store.lines()).toHaveLength(VIEW_CAP);
  });

  it('does not poll while loading and survives a poll error', async () => {
    const s = setup();
    mock = s.mock;
    const opening = s.store.open('build-win');
    expect(await s.store.poll()).toBe(0);
    await opening;
    vi.spyOn(mock, 'logs').mockRejectedValueOnce({ code: 'internal', message: 'bad' });
    expect(await s.store.poll()).toBe(0);
    expect(s.store.error()?.message).toBe('bad');
  });

  it('ignores poll results for a runner that was switched away from', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.open('build-win');
    let release!: () => void;
    vi.spyOn(mock, 'logs').mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      return lines(99_999, 2);
    });
    const polling = s.store.poll();
    await s.store.open('ui-tests');
    release();
    expect(await polling).toBe(0);
    expect(s.store.lines().some((l) => l.seq === 99_999)).toBe(false);
  });

  it('searches, clears and handles blanks and errors', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.search('info');
    expect(s.store.results()).toBeNull();
    await s.store.open('build-win');
    await s.store.search('level=info');
    expect(s.store.results()?.length).toBeGreaterThan(0);
    expect(s.store.searching()).toBe(false);
    await s.store.search('   ');
    expect(s.store.results()).toBeNull();
    await s.store.search('level=info');
    s.store.clearSearch();
    expect(s.store.query()).toBe('');
    expect(s.store.results()).toBeNull();
    vi.spyOn(mock, 'searchLogs').mockRejectedValueOnce({ code: 'internal', message: 'bad search' });
    await s.store.search('x');
    expect(s.store.error()?.message).toBe('bad search');
    expect(s.store.searching()).toBe(false);
  });

  it('drops a stale search answer', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.open('build-win');
    let release!: () => void;
    vi.spyOn(mock, 'searchLogs').mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      return lines(1, 1);
    });
    const first = s.store.search('abc');
    await s.store.search('level=info');
    release();
    await first;
    expect(s.store.query()).toBe('level=info');
    expect(s.store.results()?.length).toBeGreaterThan(1);
  });

  it('toggles following', () => {
    const s = setup();
    mock = s.mock;
    s.store.setFollowing(false);
    expect(s.store.following()).toBe(false);
  });

  it('drops a stale open answer', async () => {
    const s = setup();
    mock = s.mock;
    let release!: () => void;
    vi.spyOn(mock, 'logs').mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      return lines(1, 1);
    });
    const first = s.store.open('build-win');
    await s.store.open('ui-tests');
    release();
    await first;
    expect(s.store.runner()).toBe('ui-tests');
    expect(s.store.lines().length).toBeGreaterThan(1);
  });
});
