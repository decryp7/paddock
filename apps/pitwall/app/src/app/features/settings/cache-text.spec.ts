import type { CacheKindInfo } from '../../core/api.types';
import { expiryText, oldestText, sizeText, ttlOptions, TTL_CHOICES } from './cache-text';

const MiB = 1000 ** 2;
const kind = (over: Partial<CacheKindInfo> = {}): CacheKindInfo => ({
  storage: 'disk',
  entries: 3,
  bytes: 12 * MiB,
  maxBytes: 512 * MiB,
  hits: 5,
  oldestAt: 1_000_000,
  expiresAfterSecs: 7 * 86_400,
  sliding: false,
  diskError: null,
  ...over,
});

describe('cache text', () => {
  it('offers the six expiry choices in seconds, never as the last', () => {
    expect(TTL_CHOICES.map((c) => c.secs)).toEqual([3600, 86_400, 604_800, 2_592_000, 7_776_000, 0]);
    const emb = ttlOptions(90 * 86_400, true);
    expect(emb.map((o) => o.value)).toEqual(['3600', '86400', '604800', '2592000', '7776000', '0']);
    expect(emb[0]!.label).toBe('1 hour since last use');
    expect(ttlOptions(3600, false)[0]!.label).toBe('1 hour after it is stored');
    expect(emb.at(-1)!.label).toBe('Never (size cap only)');
  });

  it('keeps a value from the settings file that is not a standard choice', () => {
    const o = ttlOptions(5000, false);
    expect(o.at(-1)).toEqual({ value: '5000', label: '5,000 seconds after it is stored' });
  });

  it('words the expiry, size and oldest entry', () => {
    expect(expiryText(kind())).toBe('7 days after it is stored');
    expect(expiryText(kind({ expiresAfterSecs: 90 * 86_400, sliding: true }))).toBe('90 days since last use');
    expect(expiryText(kind({ expiresAfterSecs: null }))).toBe('Never (size cap only)');
    expect(expiryText(kind({ expiresAfterSecs: 5000 }))).toContain('seconds');
    expect(sizeText(kind())).toBe('12.0 MB of 512 MB');
    expect(oldestText(kind({ oldestAt: null }), 2_000_000)).toBe('none');
    expect(oldestText(kind(), 2_000_000)).toContain('ago');
  });
});
