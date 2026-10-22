import { TestBed } from '@angular/core/testing';
import type { UsageCounters } from '../../core/api.types';
import { render, settle } from '../../../testing/render';
import { CacheTiles } from './cache-tiles';
import { cacheHitRate, cacheHitRateText, tokensSaved } from './usage-format';

const counters = (over: Partial<UsageCounters> = {}): UsageCounters => ({
  requests: 0,
  promptTokens: 0,
  completionTokens: 0,
  busyMs: 0,
  errors: 0,
  peakMemoryBytes: 0,
  cacheHits: 0,
  cacheMisses: 0,
  prefixTokens: 0,
  savedTokens: 0,
  ...over,
});

describe('cache figures', () => {
  it('computes the hit rate, and has none before a request was eligible', () => {
    expect(cacheHitRate(counters())).toBeNull();
    expect(cacheHitRateText(counters())).toBe('—');
    expect(cacheHitRate(counters({ cacheHits: 1, cacheMisses: 3 }))).toBe(0.25);
    expect(cacheHitRateText(counters({ cacheHits: 2, cacheMisses: 1 }))).toBe('67 %');
  });

  it('adds reused prompt tokens and tokens skipped by hits', () => {
    expect(tokensSaved(counters({ prefixTokens: 1000, savedTokens: 234 }))).toBe(1234);
  });
});

describe('CacheTiles', () => {
  async function mount(totals: UsageCounters) {
    const r = await render(CacheTiles);
    r.fixture.componentRef.setInput('totals', totals);
    await settle(r.fixture);
    return r.el;
  }

  it('shows the rate and the saved tokens with their split', async () => {
    const el = await mount(counters({ cacheHits: 30, cacheMisses: 70, prefixTokens: 12_000, savedTokens: 3_000 }));
    const tiles = Array.from(el.querySelectorAll('.tile'));
    expect(tiles[0]!.textContent).toContain('Cache hit rate');
    expect(tiles[0]!.querySelector('.value')!.textContent).toBe('30 %');
    expect(tiles[0]!.textContent).toContain('30 hits, 70 misses');
    expect(tiles[1]!.textContent).toContain('Tokens saved');
    expect(tiles[1]!.querySelector('.value')!.textContent).toBe('15k');
    expect(tiles[1]!.textContent).toContain('12k prompt reused, 3k from cache hits');
  });

  it('shows dashes and a plain sentence when nothing was cached yet', async () => {
    const el = await mount(counters());
    const values = Array.from(el.querySelectorAll('.value')).map((v) => v.textContent);
    expect(values).toEqual(['—', '—']);
    expect(el.textContent).toContain('No cache-eligible requests yet');
    expect(el.textContent).toContain('Nothing reused yet');
    TestBed.resetTestingModule();
  });
});
