import { TestBed } from '@angular/core/testing';
import type { RequestRecord } from '../../core/api.types';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { NO_FILTER, RequestsStore, filterRequests } from './requests.store';

function setup(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [RequestsStore, { provide: BACKEND, useValue: backend }] });
  return TestBed.inject(RequestsStore);
}

const rec = (p: Partial<RequestRecord>): RequestRecord => ({
  id: 'r',
  at: 0,
  appId: 'debrief',
  alias: null,
  api: 'openai',
  endpoint: '/v1/chat/completions',
  modelId: 'qwen3-8b',
  chosenBy: 'explicit',
  priority: 'interactive',
  status: 200,
  stream: true,
  queuedMs: 0,
  ttftMs: 100,
  totalMs: 500,
  coldStartMs: null,
  promptTokens: 10,
  completionTokens: 5,
  error: null,
  cache: null,
  cachedTokens: 0,
  hasContent: true,
  ...p,
});

describe('filterRequests', () => {
  const list = [rec({ id: 'a' }), rec({ id: 'b', appId: 'spotter', endpoint: '/v1/embeddings', modelId: 'nomic-embed' }), rec({ id: 'c', status: 429 }), rec({ id: 'd', status: 500, modelId: null })];

  it('passes everything without a filter', () => {
    expect(filterRequests(list, NO_FILTER).length).toBe(4);
  });

  it('finds a request by the preset it asked for', () => {
    const withAlias = [rec({ id: 'a', alias: 'debrief-explain' }), rec({ id: 'b' })];
    expect(filterRequests(withAlias, { ...NO_FILTER, text: 'EXPLAIN' }).map((r) => r.id)).toEqual(['a']);
  });

  it('filters by app, status and plain text over endpoint and model', () => {
    const ids = (f: Partial<typeof NO_FILTER>) => filterRequests(list, { ...NO_FILTER, ...f }).map((r) => r.id);
    expect(ids({ app: 'spotter' })).toEqual(['b']);
    expect(ids({ status: 'ok' })).toEqual(['a', 'b']);
    expect(ids({ status: 'errors' })).toEqual(['c', 'd']);
    expect(ids({ text: 'EMBED' })).toEqual(['b']);
    expect(ids({ text: 'qwen' })).toEqual(['a', 'c']);
    expect(ids({ text: '  chat ', status: 'errors' })).toEqual(['c', 'd']);
    expect(ids({ text: '.*' })).toEqual([]);
  });
});

describe('RequestsStore', () => {
  it('loads requests and counts errors', async () => {
    const store = setup();
    await store.load();
    expect(store.loaded()).toBe(true);
    expect(store.records().length).toBe(400);
    expect(store.visible().length).toBe(400);
    expect(store.errorCount()).toBe(store.records().filter((r) => r.status >= 400).length);
    store.setFilter({ status: 'errors' });
    expect(store.visible().every((r) => r.status >= 400)).toBe(true);
    store.setFilter({ app: 'spotter' });
    expect(store.filter()).toEqual({ app: 'spotter', status: 'errors', text: '' });
  });

  it('keeps a typed error when loading fails', async () => {
    const store = setup(new MockBackend({ failing: ['requests'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loaded()).toBe(false);
  });

  it('fetches content only when asked, and drops it when another request is selected', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'requestContent');
    const store = setup(backend);
    await store.load();
    await store.showContent();
    expect(spy).not.toHaveBeenCalled();
    const withContent = store.records().find((r) => r.hasContent)!;
    store.select(withContent.id);
    expect(store.selected()?.id).toBe(withContent.id);
    expect(store.content()).toBeNull();
    expect(spy).not.toHaveBeenCalled();
    const done = store.showContent();
    expect(store.content()?.loading).toBe(true);
    await done;
    expect(spy).toHaveBeenCalledWith(withContent.id);
    expect(store.content()?.data?.request).toContain('messages');
    store.hideContent();
    expect(store.content()).toBeNull();
    await store.showContent();
    store.select(store.records()[1]!.id);
    expect(store.content()).toBeNull();
  });

  it('reports content that was not kept as an error', async () => {
    const store = setup();
    await store.load();
    store.select(store.records().find((r) => !r.hasContent)!.id);
    await store.showContent();
    expect(store.content()?.error?.code).toBe('notFound');
  });

  it('ignores content that arrives after the selection changed', async () => {
    const backend = new MockBackend({ latencyMs: 5 });
    const store = setup(backend);
    await store.load();
    const [a, b] = store.records().filter((r) => r.hasContent);
    store.select(a!.id);
    const late = store.showContent();
    store.select(b!.id);
    await late;
    expect(store.content()).toBeNull();
    vi.spyOn(backend, 'requestContent').mockRejectedValueOnce({ code: 'internal', message: 'x' });
    const lateErr = store.showContent();
    store.select(a!.id);
    await lateErr;
    expect(store.content()).toBeNull();
  });
});
