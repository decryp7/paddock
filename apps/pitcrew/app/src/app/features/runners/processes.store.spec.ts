import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { ClockService } from '../../core/clock.service';
import { flush, makeMock } from '../../../testing/test-helpers';
import { NoticeLog, recordShell } from '../../../testing/test-helpers';
import { PROCESS_POLL_MS, ProcessesStore } from './processes.store';

function setup() {
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [...recordShell(), { provide: BACKEND, useValue: mock }, ProcessesStore] });
  return { mock, store: TestBed.inject(ProcessesStore) };
}

describe('ProcessesStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    vi.useRealTimers();
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads a runner’s process list', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    expect(s.store.list().value?.entries.length).toBeGreaterThan(3);
    expect(s.store.runner()).toBe('build-win');
  });

  it('keeps the list while refreshing the same runner and drops it for another', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    const first = s.store.list().value;
    const p = s.store.load('build-win');
    expect(s.store.list()).toMatchObject({ loading: true, value: first });
    await p;
    const q = s.store.load('ui-tests');
    expect(s.store.list()).toMatchObject({ loading: true, value: null });
    await q;
  });

  it('refreshes silently, without a loading flash', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    const p = s.store.load('build-win', true);
    expect(s.store.list().loading).toBe(false);
    await p;
  });

  it('records errors and retries', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.retry();
    expect(s.store.runner()).toBeNull();
    await s.store.load('nope');
    expect(s.store.list().error?.code).toBe('unknownRunner');
    await s.store.load('build-win');
    expect(s.store.list().error).toBeNull();
    await s.store.retry();
    expect(s.store.list().value).not.toBeNull();
  });

  it('polls every 5 s while the window is visible and stops when destroyed', async () => {
    const s = setup();
    mock = s.mock;
    const spy = vi.spyOn(mock, 'runnerProcesses');
    vi.useFakeTimers();
    s.store.watch('build-win');
    s.store.watch('build-win');
    await vi.advanceTimersByTimeAsync(PROCESS_POLL_MS * 2 + 10);
    expect(spy.mock.calls.length).toBeGreaterThanOrEqual(4);
    const n = spy.mock.calls.length;
    TestBed.resetTestingModule();
    await vi.advanceTimersByTimeAsync(PROCESS_POLL_MS * 2);
    expect(spy.mock.calls.length).toBe(n);
  });

  it('does not poll while the window is hidden', async () => {
    const s = setup();
    mock = s.mock;
    const spy = vi.spyOn(mock, 'runnerProcesses');
    vi.useFakeTimers();
    s.store.watch('build-win');
    await vi.advanceTimersByTimeAsync(10);
    spy.mockClear();
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
    expect(TestBed.inject(ClockService).visible()).toBe(false);
    await vi.advanceTimersByTimeAsync(PROCESS_POLL_MS * 2);
    expect(spy).not.toHaveBeenCalled();
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
    s.store.stopPolling();
  });

  it('stops a process, tells the user and reloads', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    const e = s.store.list().value!.entries.find((x) => x.leftover?.rule === '*GradleDaemon*')!;
    expect(await s.store.stop(e)).toBe(true);
    expect(s.store.list().value!.entries.some((x) => x.pid === e.pid)).toBe(false);
    expect(s.store.stopped()).toContain('Stopped java.exe');
    expect(TestBed.inject(NoticeLog).notices).toHaveLength(0);
    expect(s.store.busy()).toBeNull();
  });

  it('shows the engine’s text when a stop is refused', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    const runner = s.store.list().value!.entries.find((x) => x.isRunner)!;
    expect(await s.store.stop(runner)).toBe(false);
    expect(s.store.actionError()).toContain('runner process itself');
    expect(TestBed.inject(NoticeLog).notices[0]!.level).toBe('error');
    s.store.dismissError();
    expect(s.store.actionError()).toBeNull();
    expect(await s.store.stop(runner, true)).toBe(true);
  });

  it('keeps a leftover longer, and reports a refusal', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('build-win');
    const e = s.store.list().value!.entries.find((x) => x.leftover?.fate.kind === 'stopsAt')!;
    const at = (e.leftover!.fate as { at: number }).at;
    expect(await s.store.keep(e, { amount: 30, unit: 'minutes' })).toBe(true);
    const after = s.store.list().value!.entries.find((x) => x.pid === e.pid)!;
    expect((after.leftover!.fate as { at: number }).at).toBe(at + 30 * 60_000);
    const never = s.store.list().value!.entries.find((x) => x.leftover?.fate.kind === 'never')!;
    expect(await s.store.keep(never, { amount: 1, unit: 'hours' })).toBe(false);
    expect(s.store.actionError()).toContain('not a leftover');
    expect(await s.store.keep({ ...e, startedAt: null }, { amount: 1, unit: 'hours' })).toBe(false);
    await flush();
  });

  it('does nothing before a runner is loaded', async () => {
    const s = setup();
    mock = s.mock;
    const e = { pid: 1, startedAt: 1 } as never;
    expect(await s.store.stop(e)).toBe(false);
    expect(await s.store.keep(e, { amount: 1, unit: 'hours' })).toBe(false);
  });
});
