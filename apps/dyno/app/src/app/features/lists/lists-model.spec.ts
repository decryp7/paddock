import type { ListStatus, SubscriptionStatus } from '../../core/api.types';
import { countsLine, everyHours, sourceText, updateBlockedReason, updateLine, urlOf } from './lists-model';

const counts = (c: Partial<SubscriptionStatus> = {}): SubscriptionStatus => ({ lastFetchedMs: null, lastAttemptMs: null, entries: 0, duplicates: 0, covered: 0, skipped: 0, error: null, note: null, ...c });
const list = (c: Partial<SubscriptionStatus> = {}, next: number | null = null): ListStatus => ({ id: 'l', name: 'L', enabled: true, source: { url: { url: 'https://x.example/l', schedule: { automatic: { every: 24 } } } }, counts: counts(c), nextAllowedMs: next, text: null });

describe('lists model', () => {
  it('words the counts, leaving out what is zero', () => {
    expect(countsLine(counts({ entries: 12_400, duplicates: 300, covered: 40 }))).toBe('12,400 entries · 300 duplicates removed · 40 covered by a wider range');
    expect(countsLine(counts({ entries: 1 }))).toBe('1 entry');
    expect(countsLine(counts({ entries: 2, duplicates: 1, skipped: 1 }))).toBe('2 entries · 1 duplicate removed · 1 line not taken');
  });

  it('says what a list is and where a subscription points', () => {
    expect(sourceText('manual')).toBe('Typed');
    expect(sourceText({ file: { name: 'bad.txt' } })).toBe('Imported from bad.txt');
    expect(sourceText(list().source)).toBe('Subscription');
    expect(urlOf(list().source)).toBe('https://x.example/l');
    expect(urlOf('manual')).toBeNull();
    expect(everyHours(list().source)).toBe(24);
    expect(everyHours({ url: { url: 'https://x.example/l', schedule: 'manual' } })).toBeNull();
  });

  it('tells the state of a subscription in words', () => {
    const now = 10_000_000;
    expect(updateLine(list(), true, now)).toBe('Updating…');
    expect(updateLine(list({ error: 'The server answered 404. Check the address of the list.' }), false, now)).toContain('404');
    expect(updateLine(list({ note: 'The new list has 3 entries instead of 1,200; kept the previous one.' }), false, now)).toContain('kept the previous one');
    expect(updateLine(list(), false, now)).toBe('Not fetched yet.');
    expect(updateLine(list({ lastFetchedMs: now - 5 * 60_000 }), false, now)).toBe('Last updated 5 min ago.');
    expect(updateLine(list({ lastFetchedMs: now - 3 * 3_600_000 }), false, now)).toBe('Last updated 3 h ago.');
  });

  it('explains the hourly floor and allows the update after it', () => {
    const now = 10_000_000;
    expect(updateBlockedReason(list({}, now + 60_000), now)).toContain('Next update allowed at');
    expect(updateBlockedReason(list({}, now - 1), now)).toBeNull();
    expect(updateBlockedReason(list(), now)).toBeNull();
  });
});
