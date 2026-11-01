import { MockBackend } from './mock-backend';
import { demoApps, demoReplyWords, demoRequests, demoSettings, demoUsageSeries, wordsThatFit } from './mock-data';

describe('mock cache data', () => {
  const now = 1_800_000_000_000;

  it('defaults the cache settings: embeddings on disk 512 MiB for 90 days, answers 256 MiB for 7 days, keep warm 30 days', () => {
    expect(demoSettings().cache).toEqual({
      prefixAffinity: true,
      promptCacheMib: null,
      embeddings: true,
      embeddingsStorage: 'disk',
      embeddingsMaxMib: 512,
      embeddingsTtlSecs: 90 * 86_400,
      responsesStorage: 'disk',
      responsesMaxMib: 256,
      responsesTtlSecs: 7 * 86_400,
      keepWarmDays: 30,
    });
  });

  it('opts two suite apps into cached answers and leaves the rest off', () => {
    const on = demoApps(now).filter((a) => a.permissions.cacheResponses);
    expect(on.map((a) => a.id)).toEqual(['debrief', 'spotter']);
    expect(on.every((a) => a.suite && a.today.cacheHits > 0)).toBe(true);
  });

  it('has cache hits only for apps that opted in, so no screen contradicts the switch', () => {
    for (const a of demoApps(now)) if (!a.permissions.cacheResponses) expect(a.today.cacheHits, a.id).toBe(0);
  });

  it('answers a cache hit without a queue or a cold start', () => {
    const hits = demoRequests(now).filter((r) => r.cache === 'hit');
    expect(hits.length).toBeGreaterThan(0);
    for (const r of hits) {
      expect(r.queuedMs).toBe(0);
      expect(r.coldStartMs).toBeNull();
      expect(r.totalMs).toBeGreaterThanOrEqual(r.queuedMs);
    }
  });

  it('has hit, miss and bypass outcomes and reused prompt tokens in the request log', () => {
    const reqs = demoRequests(now);
    const outcomes = new Set(reqs.map((r) => r.cache));
    expect(outcomes).toContain('hit');
    expect(outcomes).toContain('miss');
    expect(outcomes).toContain('bypass');
    expect(outcomes).toContain(null);
    expect(reqs.some((r) => r.cachedTokens > 0)).toBe(true);
    expect(reqs.filter((r) => r.cache === 'hit').every((r) => r.cachedTokens === 0)).toBe(true);
  });

  it('sums cache counters in usage series totals', () => {
    const s = demoUsageSeries('7d', now, null);
    expect(s.totals.cacheHits).toBe(s.total.reduce((n, c) => n + c.cacheHits, 0));
    expect(s.totals.cacheHits).toBeGreaterThan(0);
    expect(s.totals.cacheMisses).toBeGreaterThan(0);
    expect(s.totals.prefixTokens).toBeGreaterThan(0);
    expect(s.totals.savedTokens).toBeGreaterThan(0);
    const quiet = demoUsageSeries('24h', now, 'pitcrew');
    expect(quiet.totals.cacheHits + quiet.totals.cacheMisses).toBe(0);
  });
});

describe('mock cache commands', () => {
  it('reports both caches from the settings and counts what is stored', async () => {
    const mock = new MockBackend({ now: () => 1_800_000_000_000 });
    const info = await mock.cacheInfo();
    expect(info.embeddings).toMatchObject({ storage: 'disk', sliding: true, expiresAfterSecs: 90 * 86_400, diskError: null });
    expect(info.responses).toMatchObject({ sliding: false, expiresAfterSecs: 7 * 86_400, maxBytes: 256 * 1024 ** 2 });
    expect(info.responses.entries).toBeGreaterThan(0);
    expect(info.nextSweepAt).toBeGreaterThan(1_800_000_000_000);
    const s = await mock.settings();
    await mock.saveSettings({ ...s, cache: { ...s.cache, responsesTtlSecs: 0, responsesStorage: 'memory' } });
    expect((await mock.cacheInfo()).responses).toMatchObject({ expiresAfterSecs: null, storage: 'memory' });
  });

  it('clears by scope and returns how many entries went', async () => {
    const mock = new MockBackend();
    const before = await mock.cacheInfo();
    expect(await mock.clearCache({ kind: 'app', appId: 'debrief' })).toBe(40);
    expect((await mock.cacheInfo()).responses.entries).toBe(before.responses.entries - 40);
    expect(await mock.clearCache({ kind: 'embeddings' })).toBe(before.embeddings.entries);
    const rest = (await mock.cacheInfo()).responses.entries;
    expect(await mock.clearCache({ kind: 'all' })).toBe(rest);
    const empty = await mock.cacheInfo();
    expect(empty.responses.oldestAt).toBeNull();
    expect(await mock.clearCache({ kind: 'responses' })).toBe(0);
  });

  it('fails like any command when told to', async () => {
    const mock = new MockBackend({ failing: ['cacheInfo', 'clearCache'] });
    await expect(mock.cacheInfo()).rejects.toBeDefined();
    await expect(mock.clearCache({ kind: 'all' })).rejects.toBeDefined();
  });
});

describe('the playground demo reply', () => {
  it('is whole sentences, shorter for the small model', () => {
    const big = demoReplyWords('qwen3.5-9b').join('');
    const small = demoReplyWords('qwen3.5-4b').join('');
    expect(big.endsWith('.')).toBe(true);
    expect(small.endsWith('.')).toBe(true);
    expect(small.length).toBeLessThan(big.length);
    expect(big.startsWith(small.slice(0, 40))).toBe(true);
  });

  it('cuts at a sentence end that fits, or plainly when none does', () => {
    const words = demoReplyWords('qwen3.5-9b');
    expect(wordsThatFit(words, 100_000)).toBe(words.length);
    const n = wordsThatFit(words, 60);
    expect(words.slice(0, n).join('').trim().endsWith('.')).toBe(true);
    expect(n).toBeLessThan(words.length);
    expect(wordsThatFit(words, 5)).toBe(3);
    expect(wordsThatFit(words, 0)).toBe(1);
  });
});
