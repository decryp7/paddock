import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { flush, makeMock } from '../../../testing/test-helpers';
import { EditorStore, PREVIEW_DEBOUNCE_MS, SCHEDULE_DEBOUNCE_MS, STEPS } from './editor.store';

function setup() {
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, EditorStore] });
  return { backend, store: TestBed.inject(EditorStore) };
}

const github = (store: ReturnType<typeof setup>['store']) => store.kinds().value!.find((k) => k.id === 'github')!;

describe('EditorStore', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('loads the catalog and exposes the selected kind and missing fields', async () => {
    const { store } = setup();
    expect(store.kind()).toBeNull();
    await store.loadCatalog();
    expect(store.kinds().value!.length).toBeGreaterThan(5);
    expect(store.presets().value!.length).toBeGreaterThan(0);
    store.pickKind(github(store));
    expect(store.step()).toBe('configure');
    expect(store.kind()?.id).toBe('github');
    expect(store.missing().map((f) => f.key)).toContain('repo');
    expect(store.stepIndex()).toBe(1);
    expect(store.draft().schedule).toEqual({ mode: 'every', secs: 6 * 3600 });
  });

  it('keeps the catalog error', async () => {
    const { backend, store } = setup();
    vi.spyOn(backend, 'sourceKinds').mockRejectedValue({ code: 'unavailable', message: 'down', problems: [] });
    await store.loadCatalog();
    expect(store.kinds().error?.message).toBe('down');
  });

  it('walks the steps and suggests a name when reaching review without one', async () => {
    const { store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.setValue('repo', 'owner/tool');
    store.next();
    expect(store.step()).toBe('versions');
    store.next();
    store.next();
    store.next();
    expect(store.step()).toBe('review');
    expect(store.draft().name).toBe('tool');
    store.next();
    expect(store.step()).toBe('review');
    store.back();
    expect(store.step()).toBe('notify');
    store.setStep('source');
    store.back();
    expect(store.step()).toBe('source');
    expect(STEPS).toHaveLength(5);
  });

  it('keeps a typed name when reaching review', async () => {
    const { store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.setValue('repo', 'owner/tool');
    store.update((d) => ({ ...d, name: 'Mine' }));
    store.setStep('notify');
    store.next();
    expect(store.draft().name).toBe('Mine');
  });

  it('tests the source after edits settle, once per burst, and clears old problems', async () => {
    vi.useFakeTimers();
    const { backend, store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.setValue('repo', 'owner/tool');
    store.setValue('repo', 'owner/tool2');
    expect(backend.calls.filter((c) => c === 'testSource')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS + 1);
    expect(backend.calls.filter((c) => c === 'testSource')).toHaveLength(1);
    expect(store.preview().value?.releases.length).toBeGreaterThan(0);
  });

  it('does not test while required fields are missing or on other steps', async () => {
    vi.useFakeTimers();
    const { backend, store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.setValue('other', 'x');
    store.setSecret('token', 'abc');
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS * 2);
    store.setStep('notify');
    store.setValue('repo', 'a/b');
    await vi.advanceTimersByTimeAsync(PREVIEW_DEBOUNCE_MS * 2);
    expect(backend.calls.filter((c) => c === 'testSource')).toHaveLength(0);
    expect(store.secrets()).toEqual({ token: 'abc' });
  });

  it('tests immediately on entering versions when nothing is previewed, and ignores stale answers', async () => {
    const { backend, store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.update((d) => ({ ...d, source: { ...d.source, values: { repo: 'a/b' } } }));
    const real = backend.testSource.bind(backend);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    let n = 0;
    vi.spyOn(backend, 'testSource').mockImplementation(async (d, s, jobId) => {
      const mine = ++n;
      if (mine === 1) await gate;
      return { ...(await real(d, s, jobId)), tookMs: mine };
    });
    store.setStep('versions');
    const second = store.runPreview();
    await second;
    release();
    await flush();
    expect(store.preview().value?.tookMs).toBe(2);
  });

  it('shows a test failure and recovers', async () => {
    const { backend, store } = setup();
    await store.loadCatalog();
    store.pickKind(github(store));
    store.setValue('repo', 'a/b');
    vi.spyOn(backend, 'testSource').mockRejectedValueOnce({ code: 'internal', message: 'oops', problems: [] });
    await store.runPreview();
    expect(store.preview().error?.message).toBe('oops');
    await store.runPreview();
    expect(store.preview().error).toBeNull();
  });

  it('picks a preset with the user interval and a fresh id', async () => {
    const { store } = setup();
    await store.loadCatalog();
    const preset = store.presets().value![0]!;
    store.pickPreset(preset);
    expect(store.draft().id).toBeNull();
    expect(store.draft().name).toBe(preset.name);
    expect(store.step()).toBe('configure');
    expect(store.draft().schedule).toEqual({ mode: 'every', secs: 6 * 3600 });
    store.pickPreset({ ...preset, draft: { ...preset.draft, source: { kind: 'unknown', values: {} } } });
    expect(store.kind()).toBeNull();
  });

  it('loads a watch for editing with its stored secrets, and clears a secret', async () => {
    const { backend, store } = setup();
    await store.loadCatalog();
    await backend.saveWatch({ ...(await backend.getWatch('w-2')).spec }, { token: 'x' });
    await store.loadWatch('w-2');
    expect(store.editing()).toBe(true);
    expect(store.step()).toBe('configure');
    expect(store.draft().name).toBe('Lumen CLI');
    expect(store.secretsSet()).toEqual(['token']);
    await flush();
    expect(store.preview().value).not.toBeNull();
    store.clearSecret('token');
    expect(store.secretsSet()).toEqual([]);
    expect(store.secrets()).toEqual({ token: '' });
  });

  it('reports a watch that cannot be loaded and does not preview it', async () => {
    const { backend, store } = setup();
    await store.loadWatch('nope');
    expect(store.loadedDraft().error?.code).toBe('notFound');
    expect(backend.calls).not.toContain('testSource');
  });

  describe('discovering feeds', () => {
    it('finds feeds and builds a feed watch from one', async () => {
      const { store } = setup();
      await store.loadCatalog();
      await store.discover(' https://site.example ');
      expect(store.feeds()).toHaveLength(2);
      expect(store.discovering()).toBe(false);
      store.pickFeed(store.feeds()![0]!);
      expect(store.draft()).toMatchObject({ name: 'Releases', source: { kind: 'feed', values: { url: 'https://site.example/releases.atom' } } });
      expect(store.step()).toBe('configure');
    });
    it('reports a bad address with its problems', async () => {
      const { store } = setup();
      await store.discover('ftp://x');
      expect(store.discoverError()).toBe('That is not a web address. Start with http:// or https://');
      expect(store.feeds()).toBeNull();
    });
    it('ignores a picked feed before the catalog is loaded', () => {
      const { store } = setup();
      store.pickFeed({ url: 'https://x.example/f', title: 'F' });
      expect(store.step()).toBe('source');
    });
  });

  describe('imports', () => {
    it('imports a list and saves every draft, reporting the ones the engine refuses', async () => {
      const { backend, store } = setup();
      await store.importList('winget', 'Alpha\nAcme Installer\nBeta');
      expect(store.imported()?.drafts).toHaveLength(3);
      const result = await store.saveImported();
      expect(result.saved).toBe(2);
      expect(result.failed).toEqual(['Acme Installer: The watch cannot be saved yet.']);
      expect((await backend.listWatches()).map((w) => w.name)).toEqual(expect.arrayContaining(['Alpha', 'Beta']));
    });
    it('saves nothing before an import', async () => {
      expect(await setup().store.saveImported()).toEqual({ saved: 0, failed: [] });
    });
    it('keeps the import error', async () => {
      const { backend, store } = setup();
      vi.spyOn(backend, 'importList').mockRejectedValue(new Error('unreadable'));
      await store.importList('opml', '');
      expect(store.importError()).toBe('unreadable');
      expect(store.importing()).toBe(false);
    });
  });

  describe('save', () => {
    it('saves and returns the summary', async () => {
      const { store } = setup();
      await store.loadCatalog();
      store.pickKind(github(store));
      store.setValue('repo', 'a/b');
      store.update((d) => ({ ...d, name: 'Saved one' }));
      const saved = await store.save();
      expect(saved?.name).toBe('Saved one');
      expect(store.saving()).toBe(false);
      expect(store.problems()).toEqual([]);
    });
    it('keeps the engine problems when refused, or the message when there are none', async () => {
      const { backend, store } = setup();
      expect(await store.save()).toBeNull();
      expect(store.problems()).toContain('Give the watch a name.');
      vi.spyOn(backend, 'saveWatch').mockRejectedValue(new Error('disk full'));
      await store.save();
      expect(store.problems()).toEqual(['disk full']);
      store.update((d) => d);
      expect(store.problems()).toEqual([]);
    });
  });

  describe('schedule', () => {
    it('asks the engine for the preview once per burst of edits and keeps the draft in step', async () => {
      vi.useFakeTimers();
      const { backend, store } = setup();
      await store.loadCatalog();
      store.pickKind(github(store));
      store.setSchedule({ mode: 'every', secs: 900 });
      store.setSchedule({ mode: 'times', days: ['mon'], times: ['09:00'], zone: null });
      expect(store.draft().schedule.mode).toBe('times');
      expect(backend.calls.filter((c) => c === 'previewSchedule')).toHaveLength(0);
      await vi.advanceTimersByTimeAsync(SCHEDULE_DEBOUNCE_MS + 1);
      expect(backend.calls.filter((c) => c === 'previewSchedule')).toHaveLength(1);
      expect(store.schedulePreview().value?.description).toBe('Mon at 09:00');
      expect(store.schedulePreview().error).toBeNull();
    });

    it('keeps the engine\'s complaint, and the last good preview next to it', async () => {
      vi.useFakeTimers();
      const { store } = setup();
      await store.loadCatalog();
      store.pickKind(github(store));
      await store.runSchedulePreview();
      const good = store.schedulePreview().value;
      expect(good?.description).toBe('Every 6 hours');
      store.setSchedule({ mode: 'cron', expr: '* *', zone: null });
      await vi.advanceTimersByTimeAsync(SCHEDULE_DEBOUNCE_MS + 1);
      expect(store.schedulePreview().error?.problems[0]).toContain('five fields');
      expect(store.schedulePreview().value).toBe(good);
    });

    it('ignores an answer that arrives after a newer request', async () => {
      const { backend, store } = setup();
      await store.loadCatalog();
      const slow = new Promise<never>((_, reject) => setTimeout(() => reject({ code: 'internal', message: 'late', problems: [] }), 5));
      vi.spyOn(backend, 'previewSchedule').mockReturnValueOnce(slow);
      const first = store.runSchedulePreview();
      await store.runSchedulePreview();
      await first;
      expect(store.schedulePreview().error).toBeNull();
      expect(store.schedulePreview().value).not.toBeNull();
    });

    it('is fetched when the notify or review step opens, but only once', async () => {
      const { backend, store } = setup();
      await store.loadCatalog();
      store.pickKind(github(store));
      expect(store.schedulePreview().value).toBeNull();
      store.setStep('notify');
      await flush();
      store.setStep('review');
      await flush();
      expect(backend.calls.filter((c) => c === 'previewSchedule')).toHaveLength(1);
      store.pickKind(github(store));
      expect(store.schedulePreview().value).toBeNull();
    });
  });
});
