import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { flush, makeMock } from '../../../testing/test-helpers';
import { ToastStore } from '../../state/toast.store';
import { PREVIEW_DEBOUNCE_MS, RunnerConfigStore } from './runner-config.store';

function setup() {
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, RunnerConfigStore] });
  return { mock, store: TestBed.inject(RunnerConfigStore) };
}

describe('RunnerConfigStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    vi.useRealTimers();
    mock.dispose();
    TestBed.resetTestingModule();
  });

  async function loaded(runner = 'ui-tests') {
    const s = setup();
    mock = s.mock;
    await s.store.load(runner);
    return s.store;
  }

  /** Waits out the debounce and the preview call. */
  async function previewed(store: { review: () => Promise<void> }) {
    await store.review();
  }

  it('loads the file and copies the settings into the form', async () => {
    const store = await loaded();
    expect(store.file()?.text).toContain('capacity: 1');
    expect(store.settings()?.capacity).toBe(1);
    expect(store.mode()).toBe('form');
    expect(store.dirty()).toBe(false);
    expect(store.canSave()).toBe(false);
    expect(store.labelCheck()).toMatchObject({ kind: 'verified' });
  });

  it('records a load error and keeps the old value on a failed reload', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('nope');
    expect(s.store.reply().error?.code).toBe('unknownRunner');
    expect(s.store.settings()).toBeNull();
    await s.store.load('ui-tests');
    vi.spyOn(mock, 'runnerConfig').mockRejectedValueOnce({ code: 'internal', message: 'x' });
    await s.store.reload();
    expect(s.store.reply().error?.message).toBe('x');
    expect(s.store.file()).not.toBeNull();
  });

  it('ignores a reload when no runner was loaded and a slow answer for another runner', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.reload();
    expect(s.store.runner()).toBeNull();
    let release!: () => void;
    const real = mock.runnerConfig.bind(mock);
    vi.spyOn(mock, 'runnerConfig').mockImplementationOnce(async (r) => {
      await new Promise<void>((res) => (release = res));
      return real(r);
    });
    const slow = s.store.load('ui-tests');
    await s.store.load('build-win');
    release();
    await slow;
    expect(s.store.runner()).toBe('build-win');
    expect(s.store.file()?.path).toContain('build-win');
  });

  it('asks the engine to check an edit after the debounce, and only the last edit', async () => {
    const store = await loaded();
    const spy = vi.spyOn(mock, 'previewRunnerConfig');
    vi.useFakeTimers();
    store.patch({ capacity: 2 });
    store.patch({ capacity: 3 });
    expect(store.previewing()).toBe(true);
    expect(store.dirty()).toBe(true);
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 10);
    vi.useRealTimers();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]![1]).toMatchObject({ mode: 'form', settings: { capacity: 3 } });
    await flush();
    expect(store.preview()?.changedKeys).toEqual(['runner.capacity']);
    expect(store.canSave()).toBe(true);
  });

  it('shows the engine’s errors and blocks saving', async () => {
    const store = await loaded();
    store.patch({ capacity: 0 });
    await previewed(store);
    expect(store.hasErrors()).toBe(true);
    expect(store.issues()[0]).toMatchObject({ path: 'runner.capacity', severity: 'error' });
    expect(store.canSave()).toBe(false);
    expect(await store.save()).toBe(false);
  });

  it('reports a preview that failed and discards a stale answer', async () => {
    const store = await loaded();
    vi.spyOn(mock, 'previewRunnerConfig').mockRejectedValueOnce({ code: 'timeout', message: 'slow' });
    store.patch({ capacity: 2 });
    await previewed(store);
    expect(store.previewError()?.message).toBe('slow');
    expect(store.canSave()).toBe(false);
    let release!: () => void;
    const real = mock.previewRunnerConfig.bind(mock);
    vi.spyOn(mock, 'previewRunnerConfig').mockImplementationOnce(async (r, e) => {
      await new Promise<void>((res) => (release = res));
      return real(r, e);
    });
    store.patch({ capacity: 5 });
    const first = store.review();
    store.patch({ capacity: 6 });
    await store.review();
    release();
    await first;
    expect((store.preview()?.text ?? '')).toContain('capacity: 6');
  });

  it('does nothing without a file or a mode that matches', async () => {
    const s = setup();
    mock = s.mock;
    s.store.patch({ capacity: 2 });
    s.store.setRaw('x');
    await s.store.review();
    expect(await s.store.save()).toBe(false);
    await s.store.load('ui-tests');
    s.store.setRaw('ignored in form mode');
    expect(s.store.rawText()).toContain('capacity');
    s.store.setMode('form');
    expect(s.store.mode()).toBe('form');
  });

  it('shows the current text in Advanced, and goes back to the form while the text is untouched', async () => {
    const store = await loaded();
    store.patch({ capacity: 4 });
    await previewed(store);
    store.setMode('raw');
    expect(store.rawText()).toContain('capacity: 4');
    expect(store.rawEdited()).toBe(false);
    store.patch({ capacity: 9 });
    expect(store.settings()?.capacity).toBe(4);
    store.setMode('form');
    expect(store.mode()).toBe('form');
    expect(store.dirty()).toBe(true);
    store.discard();
    store.setMode('raw');
    expect(store.rawText()).toBe(store.file()!.text);
    store.setMode('form');
    expect(store.dirty()).toBe(false);
    expect(store.preview()).toBeNull();
  });

  it('keeps raw edits until saved or discarded and stops the form from being used', async () => {
    const store = await loaded();
    store.setMode('raw');
    store.setRaw(`# hi\n${store.rawText()}`);
    expect(store.rawEdited()).toBe(true);
    expect(store.dirty()).toBe(true);
    store.setMode('form');
    expect(store.mode()).toBe('raw');
    await previewed(store);
    expect(store.canSave()).toBe(true);
    store.setRaw('runner:\n\tcapacity: 1');
    await previewed(store);
    expect(store.issues()[0]).toMatchObject({ severity: 'error', line: 2 });
    store.discard();
    expect(store.mode()).toBe('form');
    expect(store.rawEdited()).toBe(false);
  });

  it('saves against the file’s fingerprint, then reloads and keeps the result notice', async () => {
    const store = await loaded('build-win');
    const toasts = TestBed.inject(ToastStore);
    const before = store.file()!.fingerprint;
    store.patch({ capacity: 2 });
    await previewed(store);
    const spy = vi.spyOn(mock, 'saveRunnerConfig');
    expect(await store.save()).toBe(true);
    expect(spy.mock.calls[0]![2]).toBe(before);
    expect(store.saved()).toMatchObject({ restart: 'afterJob' });
    expect(store.file()!.fingerprint).not.toBe(before);
    expect(store.file()!.restartPending).toBe(true);
    expect(store.dirty()).toBe(false);
    expect(toasts.toasts()[0]!.title).toBe('Configuration saved');
    store.dismissSaved();
    expect(store.saved()).toBeNull();
    store.patch({ capacity: 3 });
    await previewed(store);
    await store.save();
    expect(store.saved()).not.toBeNull();
    store.patch({ capacity: 4 });
    expect(store.saved()).toBeNull();
  });

  it('shows no changed keys in the toast for a comment-only save', async () => {
    const store = await loaded();
    store.setMode('raw');
    store.setRaw(`# note\n${store.rawText()}`);
    await previewed(store);
    await store.save();
    expect(store.saved()?.restart).toBe('notNeeded');
    expect(TestBed.inject(ToastStore).toasts()[0]!.body).toBeNull();
  });

  it('turns a stale base into a conflict that Reload clears', async () => {
    const store = await loaded();
    store.patch({ capacity: 2 });
    await previewed(store);
    const other = await mock.runnerConfig('ui-tests');
    await mock.saveRunnerConfig('ui-tests', { mode: 'raw', text: `# other\n${other.text}` }, other.fingerprint);
    expect(await store.save()).toBe(false);
    expect(store.conflict()).toContain('changed on disk');
    expect(store.saveError()).toBeNull();
    await store.reload();
    expect(store.conflict()).toBeNull();
    expect(store.file()!.text).toContain('# other');
    expect(store.dirty()).toBe(false);
  });

  it('treats a message about a change on disk as a conflict whatever the code', async () => {
    const store = await loaded();
    store.patch({ capacity: 2 });
    await previewed(store);
    vi.spyOn(mock, 'saveRunnerConfig').mockRejectedValueOnce({ code: 'internal', message: 'the file has changed since you opened it' });
    await store.save();
    expect(store.conflict()).not.toBeNull();
  });

  it('shows other save errors and ignores a second save while one runs', async () => {
    const store = await loaded();
    store.patch({ capacity: 2 });
    await previewed(store);
    let release!: () => void;
    vi.spyOn(mock, 'saveRunnerConfig').mockImplementationOnce(() => new Promise((_, rej) => (release = () => rej({ code: 'permissionDenied', message: 'read-only file' }))));
    const first = store.save();
    expect(await store.save()).toBe(false);
    release();
    expect(await first).toBe(false);
    expect(store.saveError()?.message).toBe('read-only file');
    expect(store.conflict()).toBeNull();
    expect(store.saving()).toBe(false);
  });

  it('tolerates a history that cannot load', async () => {
    const s = setup();
    mock = s.mock;
    vi.spyOn(mock, 'activity').mockRejectedValue(new Error('no'));
    await s.store.load('ui-tests');
    await flush();
    expect(s.store.labelCheck()).toBeNull();
    await s.store.refreshLabelCheck();
  });

  it('refreshes the label check without a runner as a no-op', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.refreshLabelCheck();
    expect(s.store.labelCheck()).toBeNull();
  });

  it('stops its timer when destroyed', async () => {
    const store = await loaded();
    vi.useFakeTimers();
    store.patch({ capacity: 2 });
    TestBed.resetTestingModule();
    const spy = vi.spyOn(mock, 'previewRunnerConfig');
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS * 2);
    expect(spy).not.toHaveBeenCalled();
  });
});
