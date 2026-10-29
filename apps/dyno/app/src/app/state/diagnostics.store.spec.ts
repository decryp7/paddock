import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { DiagnosticsStore, fileName } from './diagnostics.store';

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(DiagnosticsStore), notifier: TestBed.inject(Notifier) };
}

describe('fileName', () => {
  it('takes the last segment for either separator', () => {
    expect(fileName('/tmp/a/b.zip')).toBe('b.zip');
    expect(fileName('C:\\tmp\\b.zip')).toBe('b.zip');
    expect(fileName('/')).toBe('/');
  });
});

describe('DiagnosticsStore', () => {
  it('loads the preview', async () => {
    const { store } = setup();
    await store.loadPreview();
    expect(store.preview()!.files.length).toBeGreaterThan(0);
    expect(store.previewError()).toBeNull();
  });

  it('keeps the engine message when the preview fails', async () => {
    const { store } = setup(['diagnosticsPreview']);
    await store.loadPreview();
    expect(store.preview()).toBeNull();
    expect(store.previewError()?.message).toBeTruthy();
  });

  it('saves, remembers the result and toasts the file name, count and size', async () => {
    const { store, mock, notifier } = setup();
    await store.save();
    expect(mock.savedBundles).toHaveLength(1);
    expect(store.saved()?.path).toBe(mock.savedBundles[0]!.path);
    expect(store.saving()).toBe(false);
    expect(notifier.recent()[0]!.text).toMatch(/^Saved dyno-diagnostics\.zip \(11 files, [\d.]+ KB\)$/);
  });

  it('shows nothing when the save dialog is cancelled', async () => {
    const { store, mock, notifier } = setup();
    mock.saveDiagnostics = () => Promise.resolve(null);
    await store.save();
    expect(store.saved()).toBeNull();
    expect(store.saving()).toBe(false);
    expect(store.saveError()).toBeNull();
    expect(notifier.recent()).toEqual([]);
  });

  it('reports a failed save inline and clears it on the next try', async () => {
    const { store, mock } = setup();
    mock.saveDiagnostics = () => Promise.reject({ code: 'accessDenied', message: 'The folder is read-only.' });
    await store.save();
    expect(store.saveError()?.message).toBe('The folder is read-only.');
    expect(store.saving()).toBe(false);
    mock.saveDiagnostics = () => Promise.resolve(null);
    await store.save();
    expect(store.saveError()).toBeNull();
  });

  it('ignores a second save while one is running', async () => {
    const { store, mock } = setup();
    const first = store.save();
    const second = store.save();
    await Promise.all([first, second]);
    expect(mock.savedBundles).toHaveLength(1);
  });
});
